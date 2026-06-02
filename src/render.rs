//! Rendering the atlas graph as Graphviz DOT, Mermaid, or a terminal tree.
//!
//! All renderers are pure functions over the in-memory graph — no I/O, no
//! corpus writes.  Output is deterministic: nodes are sorted by (vision slug,
//! prd filename) and edges by (from, to) before rendering so that re-running
//! over an unchanged corpus yields byte-identical output.

use crate::graph::Graph;
use crate::model::{EdgeKind, PrdStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as FmtWrite;
use std::io::Write;

/// Render format requested by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderFormat {
    /// Graphviz DOT digraph.
    Dot,
    /// Mermaid `graph TD` block.
    Mermaid,
    /// Terminal tree (no external tool required).
    Tree,
}

/// Options controlling what the renderer includes.
#[derive(Clone, Debug, Default)]
pub struct RenderOptions {
    /// When `Some(slug)`, restrict output to this vision and its PRDs/repos.
    pub vision_filter: Option<String>,
    /// When `true`, only include `Shipped` PRDs (and their repos).
    pub shipped_only: bool,
}

// ── node-ID helpers ──────────────────────────────────────────────────────────

/// Convert an arbitrary string to a safe DOT/Mermaid node identifier.
///
/// Keeps ASCII alphanumeric and `_`; replaces everything else with `_`.
fn safe_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Mermaid node ID (no hyphens allowed in bare identifiers).
fn mermaid_id(s: &str) -> String {
    safe_id(s)
}

// ── status helpers ───────────────────────────────────────────────────────────

const fn dot_prd_attrs(status: &PrdStatus) -> &'static str {
    match status {
        PrdStatus::Drafted => "shape=box, style=filled, fillcolor=\"#eeeeee\"",
        PrdStatus::InFlight => "shape=box, style=filled, fillcolor=\"#fff3cd\"",
        PrdStatus::Shipped => "shape=box, style=filled, fillcolor=\"#d4edda\"",
    }
}

const fn dot_edge_style(kind: &EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Frontmatter => "",
        EdgeKind::Gossip => " [style=dashed]",
    }
}

fn mermaid_prd_shape(status: &PrdStatus, id: &str, label: &str) -> String {
    match status {
        PrdStatus::InFlight | PrdStatus::Drafted => format!("{id}[\"{label}\"]"),
        PrdStatus::Shipped => format!("{id}([\"{label}\"])"),
    }
}

const fn status_glyph(status: &PrdStatus) -> char {
    match status {
        PrdStatus::Drafted => '○',
        PrdStatus::InFlight => '◑',
        PrdStatus::Shipped => '●',
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Filter visions according to `opts.vision_filter`.
fn filtered_vision_slugs(graph: &Graph, opts: &RenderOptions) -> BTreeSet<String> {
    graph
        .visions
        .iter()
        .filter(|v| {
            opts.vision_filter
                .as_deref()
                .is_none_or(|f| v.slug == f)
        })
        .map(|v| v.slug.clone())
        .collect()
}

/// Build sorted (`vision_slug`, filename) pairs for PRDs in scope.
fn scoped_prd_list(
    graph: &Graph,
    vision_slugs: &BTreeSet<String>,
    opts: &RenderOptions,
) -> Vec<(String, String)> {
    // A PRD is in scope when it is owned by an in-scope vision, OR — when no
    // explicit `--vision` filter is active — regardless of whether its `vision`
    // field resolves to a known manifest slug. Some live PRDs carry a `vision`
    // value that is a raw Markdown link or carries a trailing annotation (e.g.
    // `[visions/foo.md](visions/foo.md)` or `bar.md (Fleet 4)`) that never
    // matches a manifest slug; such PRDs must still appear as nodes in the whole
    // -web view rather than vanish. They simply get no vision→PRD ownership edge
    // (the edge loops match on `p.vision == slug`). Under an active vision
    // filter we keep the strict scoping so `--vision X` stays exact.
    let unfiltered = opts.vision_filter.is_none();
    let mut list: Vec<_> = graph
        .prds
        .iter()
        .filter(|p| unfiltered || vision_slugs.contains(&p.vision) || p.vision.is_empty())
        .filter(|p| !opts.shipped_only || p.status == PrdStatus::Shipped)
        .map(|p| (p.vision.clone(), p.filename.clone()))
        .collect();
    list.sort();
    list
}

// ── DOT renderer ─────────────────────────────────────────────────────────────

/// Render the graph as a Graphviz DOT digraph.
///
/// # Errors
///
/// Returns an error only if `fmt::write!` fails (infallible in practice for
/// `String`, but we propagate the trait error to keep the signature uniform).
#[allow(clippy::too_many_lines)]
pub fn render_dot(graph: &Graph, opts: &RenderOptions) -> anyhow::Result<String> {
    let mut out = String::new();

    let vision_slugs = filtered_vision_slugs(graph, opts);
    let prd_list = scoped_prd_list(graph, &vision_slugs, opts);
    let scoped_filenames: BTreeSet<String> = prd_list.iter().map(|(_, f)| f.clone()).collect();

    writeln!(out, "digraph atlas {{")?;
    writeln!(out, "  rankdir=TB;")?;
    writeln!(out, "  node [fontname=\"Helvetica\"];")?;
    writeln!(out, "  edge [fontname=\"Helvetica\"];")?;
    writeln!(out)?;

    // Vision nodes (sorted by slug — BTreeSet gives us that).
    writeln!(out, "  // Vision nodes")?;
    for slug in &vision_slugs {
        if let Some(v) = graph.visions.iter().find(|v| &v.slug == slug) {
            let id = format!("vision_{}", safe_id(&v.slug));
            writeln!(
                out,
                "  {id} [label=\"{slug}\", shape=ellipse, style=filled, \
                 fillcolor=\"#cce5ff\"];",
                slug = v.slug,
            )?;
        }
    }
    writeln!(out)?;

    // PRD nodes (sorted by vision slug then filename).
    writeln!(out, "  // PRD nodes")?;
    for (_, filename) in &prd_list {
        if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
            let id = format!("prd_{}", safe_id(&p.filename));
            let attrs = dot_prd_attrs(&p.status);
            // Label: strip "PRD-" prefix and ".md" suffix for readability.
            let label = p
                .filename
                .strip_prefix("PRD-")
                .and_then(|s| s.strip_suffix(".md"))
                .unwrap_or(&p.filename);
            writeln!(out, "  {id} [{attrs}, label=\"{label}\"];")?;
        }
    }
    writeln!(out)?;

    // Repo nodes for shipped PRDs in scope (deduplicated, sorted by URL).
    writeln!(out, "  // Repo nodes")?;
    let mut seen_repo_urls: BTreeSet<String> = BTreeSet::new();
    for (_, filename) in &prd_list {
        if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
            if p.status == PrdStatus::Shipped && !p.repo_url.is_empty() {
                seen_repo_urls.insert(p.repo_url.clone());
            }
        }
    }
    for url in &seen_repo_urls {
        let id = format!("repo_{}", safe_id(url));
        let label = url
            .trim_start_matches("https://github.com/")
            .trim_start_matches("https://");
        writeln!(
            out,
            "  {id} [label=\"{label}\", shape=cylinder, style=filled, \
             fillcolor=\"#f8d7da\"];"
        )?;
    }
    writeln!(out)?;

    // Vision → PRD ownership edges.
    writeln!(out, "  // Vision → PRD edges")?;
    for slug in &vision_slugs {
        let vision_id = format!("vision_{}", safe_id(slug));
        let mut prd_filenames: Vec<String> = graph
            .prds
            .iter()
            .filter(|p| &p.vision == slug && scoped_filenames.contains(&p.filename))
            .map(|p| p.filename.clone())
            .collect();
        prd_filenames.sort();
        for filename in prd_filenames {
            let prd_id = format!("prd_{}", safe_id(&filename));
            writeln!(out, "  {vision_id} -> {prd_id} [style=dotted, color=blue];")?;
        }
    }
    writeln!(out)?;

    // PRD → PRD dependency edges (only those within scope, sorted).
    writeln!(out, "  // PRD → PRD dependency edges")?;
    let mut dep_edges: Vec<_> = graph
        .edge_set
        .edges
        .iter()
        .filter(|e| scoped_filenames.contains(&e.from) && scoped_filenames.contains(&e.to))
        .map(|e| (e.from.clone(), e.to.clone(), e.kind.clone()))
        .collect();
    dep_edges.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

    for (from, to, kind) in &dep_edges {
        let from_id = format!("prd_{}", safe_id(from));
        let to_id = format!("prd_{}", safe_id(to));
        let style = dot_edge_style(kind);
        writeln!(out, "  {from_id} -> {to_id}{style};")?;
    }
    writeln!(out)?;

    // Shipped PRD → Repo edges.
    writeln!(out, "  // Shipped PRD → Repo edges")?;
    let mut repo_edges: Vec<_> = prd_list
        .iter()
        .filter_map(|(_, filename)| {
            graph
                .prds
                .iter()
                .find(|p| &p.filename == filename)
                .filter(|p| p.status == PrdStatus::Shipped && !p.repo_url.is_empty())
                .map(|p| (filename.clone(), p.repo_url.clone()))
        })
        .collect();
    repo_edges.sort();
    for (filename, url) in &repo_edges {
        let prd_id = format!("prd_{}", safe_id(filename));
        let repo_id = format!("repo_{}", safe_id(url));
        writeln!(out, "  {prd_id} -> {repo_id} [color=gray, arrowhead=open];")?;
    }
    writeln!(out)?;

    writeln!(out, "}}")?;
    Ok(out)
}

// ── Mermaid renderer ─────────────────────────────────────────────────────────

/// Render the graph as a Mermaid `graph TD` block.
///
/// # Errors
///
/// Returns an error only if `fmt::write!` fails (infallible for `String`).
#[allow(clippy::too_many_lines)]
pub fn render_mermaid(graph: &Graph, opts: &RenderOptions) -> anyhow::Result<String> {
    let mut out = String::new();

    let vision_slugs = filtered_vision_slugs(graph, opts);
    let prd_list = scoped_prd_list(graph, &vision_slugs, opts);
    let scoped_filenames: BTreeSet<String> = prd_list.iter().map(|(_, f)| f.clone()).collect();

    writeln!(out, "graph TD")?;

    // Vision nodes.
    writeln!(out, "  %% Vision nodes")?;
    for slug in &vision_slugs {
        let id = format!("vision_{}", mermaid_id(slug));
        writeln!(out, "  {id}((\"{slug}\"))")?;
    }

    // PRD nodes.
    writeln!(out, "  %% PRD nodes")?;
    for (_, filename) in &prd_list {
        if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
            let id = format!("prd_{}", mermaid_id(&p.filename));
            let label = p
                .filename
                .strip_prefix("PRD-")
                .and_then(|s| s.strip_suffix(".md"))
                .unwrap_or(&p.filename);
            let node_decl = mermaid_prd_shape(&p.status, &id, label);
            writeln!(out, "  {node_decl}")?;
        }
    }

    // Repo nodes (deduplicated, sorted by URL).
    writeln!(out, "  %% Repo nodes")?;
    let mut seen_repos: BTreeSet<String> = BTreeSet::new();
    for (_, filename) in &prd_list {
        if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
            if p.status == PrdStatus::Shipped && !p.repo_url.is_empty() {
                seen_repos.insert(p.repo_url.clone());
            }
        }
    }
    for url in &seen_repos {
        let id = format!("repo_{}", mermaid_id(url));
        let label = url
            .trim_start_matches("https://github.com/")
            .trim_start_matches("https://");
        writeln!(out, "  {id}[(\"{label}\")]")?;
    }

    // Vision → PRD edges.
    writeln!(out, "  %% Vision to PRD edges")?;
    for slug in &vision_slugs {
        let vid = format!("vision_{}", mermaid_id(slug));
        let mut owned: Vec<String> = graph
            .prds
            .iter()
            .filter(|p| &p.vision == slug && scoped_filenames.contains(&p.filename))
            .map(|p| p.filename.clone())
            .collect();
        owned.sort();
        for filename in owned {
            let pid = format!("prd_{}", mermaid_id(&filename));
            writeln!(out, "  {vid} --> {pid}")?;
        }
    }

    // PRD → PRD dependency edges (sorted, styled by kind).
    writeln!(out, "  %% PRD dependency edges")?;
    let mut dep_edges: Vec<_> = graph
        .edge_set
        .edges
        .iter()
        .filter(|e| scoped_filenames.contains(&e.from) && scoped_filenames.contains(&e.to))
        .map(|e| (e.from.clone(), e.to.clone(), e.kind.clone()))
        .collect();
    dep_edges.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));

    for (from, to, kind) in &dep_edges {
        let from_id = format!("prd_{}", mermaid_id(from));
        let to_id = format!("prd_{}", mermaid_id(to));
        let arrow = match kind {
            EdgeKind::Frontmatter => "-->",
            EdgeKind::Gossip => "-.->",
        };
        writeln!(out, "  {from_id} {arrow} {to_id}")?;
    }

    // Shipped PRD → Repo edges.
    writeln!(out, "  %% Shipped PRD to Repo edges")?;
    let mut repo_edges: Vec<_> = prd_list
        .iter()
        .filter_map(|(_, filename)| {
            graph
                .prds
                .iter()
                .find(|p| &p.filename == filename)
                .filter(|p| p.status == PrdStatus::Shipped && !p.repo_url.is_empty())
                .map(|p| (filename.clone(), p.repo_url.clone()))
        })
        .collect();
    repo_edges.sort();
    for (filename, url) in &repo_edges {
        let pid = format!("prd_{}", mermaid_id(filename));
        let rid = format!("repo_{}", mermaid_id(url));
        writeln!(out, "  {pid} --> {rid}")?;
    }

    Ok(out)
}

// ── Tree renderer ─────────────────────────────────────────────────────────────

/// Render the graph as a terminal tree: vision as root, PRDs indented.
///
/// Dependency arrows noted inline.
///
/// # Errors
///
/// Returns an error only if writing to the output buffer fails.
pub fn render_tree(graph: &Graph, opts: &RenderOptions) -> anyhow::Result<String> {
    let mut buf = Vec::new();

    // Build sorted list of relevant vision slugs.
    let vision_slugs: Vec<String> = {
        let mut slugs: Vec<String> = graph
            .visions
            .iter()
            .filter(|v| {
                opts.vision_filter
                    .as_deref()
                    .is_none_or(|f| v.slug == f)
            })
            .map(|v| v.slug.clone())
            .collect();
        slugs.sort();
        slugs
    };

    // Group PRDs by vision for fast lookup.
    let known: BTreeSet<&str> = graph.visions.iter().map(|v| v.slug.as_str()).collect();
    let mut by_vision: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // PRDs whose `vision` field resolves to no manifest vision (raw link /
    // annotated slug) are bucketed under a synthetic "(unfiled)" root so they
    // are not silently omitted from the whole-web tree view.
    let mut unfiled: Vec<String> = Vec::new();
    for prd in &graph.prds {
        if !opts.shipped_only || prd.status == PrdStatus::Shipped {
            if !prd.vision.is_empty() && !known.contains(prd.vision.as_str()) {
                unfiled.push(prd.filename.clone());
            }
            by_vision
                .entry(prd.vision.clone())
                .or_default()
                .push(prd.filename.clone());
        }
    }
    // Sort each vision's PRD list.
    for filenames in by_vision.values_mut() {
        filenames.sort();
    }
    unfiled.sort();

    // Roots to render: the (possibly filtered) manifest visions, plus an
    // "(unfiled)" pseudo-root for orphan-vision PRDs when no filter is active.
    let mut roots: Vec<String> = vision_slugs;
    if opts.vision_filter.is_none() && !unfiled.is_empty() {
        roots.push("(unfiled)".to_string());
    }

    for slug in &roots {
        if slug == "(unfiled)" {
            writeln!(buf, "vision: (unfiled)  [orphan-vision PRDs]")?;
            let prd_count = unfiled.len();
            for (i, filename) in unfiled.iter().enumerate() {
                let is_last = i + 1 == prd_count;
                let prefix = if is_last { "└──" } else { "├──" };
                if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
                    let glyph = status_glyph(&p.status);
                    let label = p
                        .filename
                        .strip_prefix("PRD-")
                        .and_then(|s| s.strip_suffix(".md"))
                        .unwrap_or(&p.filename);
                    let repo_str = if p.repo_url.is_empty() {
                        String::new()
                    } else {
                        format!("  → {}", p.repo_url)
                    };
                    writeln!(buf, "  {prefix} {glyph} {label}{repo_str}")?;
                }
            }
            writeln!(buf)?;
            continue;
        }
        if let Some(v) = graph.visions.iter().find(|v| &v.slug == slug) {
            writeln!(buf, "vision: {slug}  [{status}]", status = v.status)?;
        } else {
            writeln!(buf, "vision: {slug}")?;
        }

        let empty: Vec<String> = Vec::new();
        let prd_filenames = by_vision.get(slug).unwrap_or(&empty);

        let prd_count = prd_filenames.len();
        for (i, filename) in prd_filenames.iter().enumerate() {
            let is_last = i + 1 == prd_count;
            let prefix = if is_last { "└──" } else { "├──" };

            if let Some(p) = graph.prds.iter().find(|p| &p.filename == filename) {
                let glyph = status_glyph(&p.status);
                let label = p
                    .filename
                    .strip_prefix("PRD-")
                    .and_then(|s| s.strip_suffix(".md"))
                    .unwrap_or(&p.filename);

                // Dependency arrows for this PRD.
                let out_edges =
                    graph
                        .edge_index
                        .out_edges(&graph.edge_set.edges, &p.filename);
                let deps_str = if out_edges.is_empty() {
                    String::new()
                } else {
                    let mut sorted_deps: Vec<&str> =
                        out_edges.iter().map(|e| e.to.as_str()).collect();
                    sorted_deps.sort_unstable();
                    let dep_names: Vec<&str> = sorted_deps
                        .iter()
                        .map(|d| {
                            d.strip_prefix("PRD-")
                                .and_then(|s| s.strip_suffix(".md"))
                                .unwrap_or(d)
                        })
                        .collect();
                    format!("  ← needs: {}", dep_names.join(", "))
                };

                let repo_str = if p.repo_url.is_empty() {
                    String::new()
                } else {
                    format!("  → {}", p.repo_url)
                };

                writeln!(buf, "  {prefix} {glyph} {label}{deps_str}{repo_str}")?;
            }
        }
        writeln!(buf)?;
    }

    Ok(String::from_utf8(buf)?)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges::EdgeSet;
    use crate::model::{Edge, EdgeKind, PrdNode, PrdStatus, RepoNode, VisionNode};

    fn make_graph(
        visions: Vec<VisionNode>,
        prds: Vec<PrdNode>,
        repos: Vec<RepoNode>,
        edges: Vec<Edge>,
    ) -> Graph {
        use crate::edges::EdgeIndex;
        let edge_set = EdgeSet { edges, unresolved: 0 };
        let edge_index = EdgeIndex::build(&edge_set);
        Graph { visions, prds, repos, edge_set, edge_index }
    }

    fn vision(slug: &str) -> VisionNode {
        VisionNode {
            slug: slug.to_string(),
            path: String::new(),
            status: "active".to_string(),
            prds_drafted: Vec::new(),
            seed: String::new(),
        }
    }

    fn prd(filename: &str, vision: &str, status: PrdStatus) -> PrdNode {
        PrdNode {
            filename: filename.to_string(),
            title: filename.to_string(),
            vision: vision.to_string(),
            build_target: "rust-extend".to_string(),
            build_into: String::new(),
            status,
            repo_url: String::new(),
            repo_path: String::new(),
        }
    }

    fn prd_shipped(filename: &str, vision: &str, repo_url: &str) -> PrdNode {
        PrdNode {
            filename: filename.to_string(),
            title: filename.to_string(),
            vision: vision.to_string(),
            build_target: "rust-extend".to_string(),
            build_into: String::new(),
            status: PrdStatus::Shipped,
            repo_url: repo_url.to_string(),
            repo_path: String::new(),
        }
    }

    fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
        Edge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            source: "test:1".to_string(),
        }
    }

    // ── safe_id ──────────────────────────────────────────────────────────────

    #[test]
    fn safe_id_replaces_hyphens_and_dots() {
        assert_eq!(safe_id("PRD-atlas-core.md"), "PRD_atlas_core_md");
        assert_eq!(safe_id("atlas_core"), "atlas_core");
    }

    // ── DOT renderer ─────────────────────────────────────────────────────────

    #[test]
    fn dot_contains_digraph_wrapper() {
        let g = make_graph(vec![], vec![], vec![], vec![]);
        let dot = render_dot(&g, &RenderOptions::default()).unwrap();
        assert!(dot.starts_with("digraph atlas {"), "DOT must start with digraph");
        assert!(dot.ends_with("}\n"), "DOT must end with }}\\n");
    }

    #[test]
    fn dot_includes_vision_and_prd_nodes() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped)],
            vec![],
            vec![],
        );
        let dot = render_dot(&g, &RenderOptions::default()).unwrap();
        assert!(dot.contains("vision_atlas"), "vision node must appear");
        assert!(
            dot.contains("prd_PRD_atlas_core_md"),
            "prd node must appear: {dot}"
        );
    }

    #[test]
    fn dot_dep_edge_frontmatter_solid() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Frontmatter,
            )],
        );
        let dot = render_dot(&g, &RenderOptions::default()).unwrap();
        // Frontmatter edge: no "dashed" style.
        let has_edge = dot.contains("prd_PRD_atlas_render_md -> prd_PRD_atlas_core_md;");
        assert!(has_edge, "frontmatter edge must be present: {dot}");
    }

    #[test]
    fn dot_dep_edge_gossip_dashed() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Gossip,
            )],
        );
        let dot = render_dot(&g, &RenderOptions::default()).unwrap();
        assert!(
            dot.contains("[style=dashed]"),
            "gossip edge must be dashed: {dot}"
        );
    }

    #[test]
    fn dot_shipped_only_excludes_drafted() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::Drafted),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
            ],
            vec![],
            vec![],
        );
        let dot = render_dot(
            &g,
            &RenderOptions {
                shipped_only: true,
                vision_filter: None,
            },
        )
        .unwrap();
        assert!(
            !dot.contains("prd_PRD_atlas_render_md"),
            "drafted PRD must be absent in shipped_only mode"
        );
        assert!(
            dot.contains("prd_PRD_atlas_core_md"),
            "shipped PRD must be present"
        );
    }

    #[test]
    fn dot_vision_filter_scopes_output() {
        let g = make_graph(
            vec![vision("atlas"), vision("vigil")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-vigil-core.md", "vigil", PrdStatus::Shipped),
            ],
            vec![],
            vec![],
        );
        let dot = render_dot(
            &g,
            &RenderOptions {
                vision_filter: Some("atlas".to_string()),
                shipped_only: false,
            },
        )
        .unwrap();
        assert!(
            !dot.contains("vision_vigil"),
            "vigil vision must be absent when filter=atlas"
        );
        assert!(dot.contains("vision_atlas"), "atlas vision must be present");
    }

    // ── Determinism ───────────────────────────────────────────────────────────

    #[test]
    fn dot_output_is_deterministic() {
        let g = make_graph(
            vec![vision("atlas"), vision("vigil")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-vigil-core.md", "vigil", PrdStatus::Drafted),
            ],
            vec![],
            vec![
                edge("PRD-atlas-render.md", "PRD-atlas-core.md", EdgeKind::Frontmatter),
            ],
        );
        let opts = RenderOptions::default();
        let run1 = render_dot(&g, &opts).unwrap();
        let run2 = render_dot(&g, &opts).unwrap();
        assert_eq!(run1, run2, "DOT output must be deterministic");
    }

    #[test]
    fn mermaid_output_is_deterministic() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped)],
            vec![],
            vec![],
        );
        let opts = RenderOptions::default();
        let run1 = render_mermaid(&g, &opts).unwrap();
        let run2 = render_mermaid(&g, &opts).unwrap();
        assert_eq!(run1, run2, "Mermaid output must be deterministic");
    }

    // ── Mermaid renderer ──────────────────────────────────────────────────────

    #[test]
    fn mermaid_starts_with_graph_td() {
        let g = make_graph(vec![], vec![], vec![], vec![]);
        let mmd = render_mermaid(&g, &RenderOptions::default()).unwrap();
        assert!(mmd.starts_with("graph TD"), "Mermaid must start with 'graph TD'");
    }

    #[test]
    fn mermaid_no_unclosed_brackets() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Frontmatter,
            )],
        );
        let mmd = render_mermaid(&g, &RenderOptions::default()).unwrap();
        // Count open and close brackets/parens per line — must balance.
        for line in mmd.lines() {
            // Skip comment lines.
            if line.trim_start().starts_with('%') {
                continue;
            }
            // Skip edge lines (they have `-->` with no bracket issue).
            if line.contains("-->") || line.contains("-.-") {
                continue;
            }
            let opens: usize = line.chars().filter(|&c| c == '(' || c == '[').count();
            let closes: usize = line.chars().filter(|&c| c == ')' || c == ']').count();
            assert_eq!(
                opens, closes,
                "Unbalanced brackets on Mermaid line: {line:?}"
            );
        }
    }

    #[test]
    fn mermaid_gossip_edge_uses_dotted_arrow() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::Drafted),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Gossip,
            )],
        );
        let mmd = render_mermaid(&g, &RenderOptions::default()).unwrap();
        assert!(mmd.contains("-.-"), "gossip edge must use dotted arrow: {mmd}");
    }

    // ── Tree renderer ─────────────────────────────────────────────────────────

    #[test]
    fn tree_includes_vision_as_root() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped)],
            vec![],
            vec![],
        );
        let tree = render_tree(&g, &RenderOptions::default()).unwrap();
        assert!(tree.contains("vision: atlas"), "vision must appear as root");
    }

    #[test]
    fn tree_shows_status_glyphs() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-atlas-edges.md", "atlas", PrdStatus::Drafted),
            ],
            vec![],
            vec![],
        );
        let tree = render_tree(&g, &RenderOptions::default()).unwrap();
        assert!(tree.contains('●'), "shipped glyph must appear");
        assert!(tree.contains('◑'), "in-flight glyph must appear");
        assert!(tree.contains('○'), "drafted glyph must appear");
    }

    #[test]
    fn tree_shows_dependency_arrows() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Frontmatter,
            )],
        );
        let tree = render_tree(&g, &RenderOptions::default()).unwrap();
        assert!(
            tree.contains("atlas-core"),
            "dependency must appear in tree: {tree}"
        );
        assert!(tree.contains("← needs"), "deps annotation must appear: {tree}");
    }

    #[test]
    fn tree_atlas_vision_ac() {
        // AC #4: atlas graph --format tree --vision atlas shows atlas as root
        // with its PRDs, status glyphs, and dep arrows.
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-edges.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-orphans.md", "atlas", PrdStatus::Drafted),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
            ],
            vec![],
            vec![
                edge("PRD-atlas-edges.md", "PRD-atlas-core.md", EdgeKind::Frontmatter),
                edge("PRD-atlas-orphans.md", "PRD-atlas-edges.md", EdgeKind::Frontmatter),
                edge("PRD-atlas-render.md", "PRD-atlas-core.md", EdgeKind::Frontmatter),
            ],
        );
        let tree = render_tree(
            &g,
            &RenderOptions {
                vision_filter: Some("atlas".to_string()),
                shipped_only: false,
            },
        )
        .unwrap();
        // Root
        assert!(tree.contains("vision: atlas"), "atlas vision root present");
        // All four PRDs
        assert!(tree.contains("atlas-core"), "atlas-core present");
        assert!(tree.contains("atlas-edges"), "atlas-edges present");
        assert!(tree.contains("atlas-orphans"), "atlas-orphans present");
        assert!(tree.contains("atlas-render"), "atlas-render present");
        // Dep arrows present
        assert!(tree.contains("← needs"), "dependency arrows present");
    }

    #[test]
    fn tree_shipped_only_excludes_drafted() {
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::Drafted),
            ],
            vec![],
            vec![],
        );
        let tree = render_tree(
            &g,
            &RenderOptions {
                shipped_only: true,
                vision_filter: None,
            },
        )
        .unwrap();
        assert!(
            !tree.contains("atlas-render"),
            "drafted PRD must be absent in shipped_only mode"
        );
        assert!(tree.contains("atlas-core"), "shipped PRD must be present");
    }

    #[test]
    fn tree_shipped_only_no_drafted_label() {
        // AC #5: --shipped-only run contains only shipped PRDs, no drafted node.
        let g = make_graph(
            vec![vision("atlas")],
            vec![
                prd_shipped("PRD-atlas-core.md", "atlas", "https://github.com/j0yen/atlas"),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::Drafted),
            ],
            vec![],
            vec![],
        );
        let dot = render_dot(
            &g,
            &RenderOptions {
                shipped_only: true,
                vision_filter: None,
            },
        )
        .unwrap();
        assert!(
            !dot.contains("atlas-render"),
            "drafted must not appear in shipped-only DOT"
        );
    }

    #[test]
    fn tree_output_is_deterministic() {
        let g = make_graph(
            vec![vision("atlas"), vision("vigil")],
            vec![
                prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped),
                prd("PRD-atlas-render.md", "atlas", PrdStatus::InFlight),
                prd("PRD-vigil-core.md", "vigil", PrdStatus::Drafted),
            ],
            vec![],
            vec![edge(
                "PRD-atlas-render.md",
                "PRD-atlas-core.md",
                EdgeKind::Frontmatter,
            )],
        );
        let opts = RenderOptions::default();
        let run1 = render_tree(&g, &opts).unwrap();
        let run2 = render_tree(&g, &opts).unwrap();
        assert_eq!(run1, run2, "tree output must be deterministic");
    }
}
