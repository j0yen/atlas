//! Output formatting for atlas commands.

use anyhow::Result;
use std::io::{self, Write};

use crate::args::NodeKindArg;
use crate::graph::Graph;
use crate::model::{Edge, PrdNode, PrdStatus, RepoNode, VisionNode};

/// Output for `atlas nodes [--kind ...] [--format ...]`.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn nodes(graph: &Graph, kind: Option<&NodeKindArg>, json: bool) -> Result<()> {
    if json {
        nodes_json(graph, kind)
    } else {
        nodes_text(graph, kind)
    }
}

fn nodes_text(graph: &Graph, kind: Option<&NodeKindArg>) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let emit_visions = matches!(kind, None | Some(NodeKindArg::Vision));
    let emit_prds = matches!(kind, None | Some(NodeKindArg::Prd));
    let emit_repos = matches!(kind, None | Some(NodeKindArg::Repo));

    if emit_visions {
        for v in &graph.visions {
            writeln!(out, "vision  {slug}  [{status}]  prds_drafted={n}",
                slug = v.slug,
                status = v.status,
                n = v.prds_drafted.len()
            )?;
        }
    }
    if emit_prds {
        for p in &graph.prds {
            let repo = if p.repo_url.is_empty() {
                String::new()
            } else {
                format!("  repo={}", p.repo_url)
            };
            writeln!(
                out,
                "prd     {filename}  [{status}]{repo}",
                filename = p.filename,
                status = p.status,
            )?;
        }
    }
    if emit_repos {
        for r in &graph.repos {
            writeln!(out, "repo    {name}  {url}", name = r.name, url = r.url)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct NodesJsonOutput<'a> {
    visions: &'a [VisionNode],
    prds: &'a [PrdNode],
    repos: &'a [RepoNode],
}

fn nodes_json(graph: &Graph, kind: Option<&NodeKindArg>) -> Result<()> {
    let empty_visions: Vec<VisionNode> = Vec::new();
    let empty_prds: Vec<PrdNode> = Vec::new();
    let empty_repos: Vec<RepoNode> = Vec::new();

    let output = NodesJsonOutput {
        visions: match kind {
            None | Some(NodeKindArg::Vision) => &graph.visions,
            _ => &empty_visions,
        },
        prds: match kind {
            None | Some(NodeKindArg::Prd) => &graph.prds,
            _ => &empty_prds,
        },
        repos: match kind {
            None | Some(NodeKindArg::Repo) => &graph.repos,
            _ => &empty_repos,
        },
    };
    let json = serde_json::to_string_pretty(&output)?;
    writeln!(io::stdout().lock(), "{json}")?;
    Ok(())
}

/// Output for `atlas show <vision-slug> [--format ...]`.
///
/// Exits with code 2 if the vision slug is unknown.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn show(graph: &Graph, vision_slug: &str, json: bool) -> Result<()> {
    // Find the vision.
    let Some(vision) = graph.visions.iter().find(|v| v.slug == vision_slug) else {
        // Write error to stderr and exit 2.
        let mut stderr = io::stderr();
        writeln!(stderr, "atlas: unknown vision slug '{vision_slug}'")?;
        writeln!(stderr, "Run 'atlas nodes --kind vision' to see available slugs.")?;
        std::process::exit(2);
    };

    // Find PRDs owned by this vision.
    let owned_prds: Vec<&PrdNode> = graph
        .prds
        .iter()
        .filter(|p| p.vision == vision_slug)
        .collect();

    if json {
        show_json(vision, &owned_prds)
    } else {
        show_text(vision, &owned_prds)
    }
}

fn show_text(vision: &VisionNode, prds: &[&PrdNode]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "vision: {slug}  [{status}]", slug = vision.slug, status = vision.status)?;
    if !vision.seed.is_empty() {
        writeln!(out, "  seed: {}", vision.seed)?;
    }
    writeln!(out, "  prds ({n}):", n = prds.len())?;
    for p in prds {
        let repo = if p.repo_url.is_empty() {
            String::new()
        } else {
            format!("  → {}", p.repo_url)
        };
        writeln!(
            out,
            "    {filename}  [{status}]{repo}",
            filename = p.filename,
            status = p.status,
        )?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct ShowJsonOutput<'a> {
    vision: &'a VisionNode,
    prds: Vec<&'a PrdNode>,
}

fn show_json(vision: &VisionNode, prds: &[&PrdNode]) -> Result<()> {
    let output = ShowJsonOutput {
        vision,
        prds: prds.to_vec(),
    };
    let json = serde_json::to_string_pretty(&output)?;
    writeln!(io::stdout().lock(), "{json}")?;
    Ok(())
}

// ── deps ─────────────────────────────────────────────────────────────────────

/// Output for `atlas deps <prd> [--format ...]`.
///
/// Exits with code 2 if the PRD is not found in the graph.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn deps(graph: &Graph, prd_arg: &str, json: bool) -> Result<()> {
    // Normalise: accept slug ("atlas-edges") or filename ("PRD-atlas-edges.md").
    let filename = normalise_prd_arg(prd_arg);

    // Verify the PRD exists.
    if !graph.prds.iter().any(|p| p.filename == filename) {
        let mut stderr = io::stderr();
        writeln!(stderr, "atlas: unknown PRD '{filename}'")?;
        writeln!(
            stderr,
            "Run 'atlas nodes --kind prd' to see available PRDs."
        )?;
        std::process::exit(2);
    }

    let out_edges = graph.edge_index.out_edges(&graph.edge_set.edges, &filename);
    let in_edges = graph.edge_index.in_edges(&graph.edge_set.edges, &filename);

    if json {
        deps_json(&filename, &out_edges, &in_edges, graph.edge_set.unresolved)
    } else {
        deps_text(&filename, &out_edges, &in_edges)
    }
}

fn deps_text(filename: &str, out_edges: &[&Edge], in_edges: &[&Edge]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "prd: {filename}")?;

    writeln!(out, "depends on ({}):", out_edges.len())?;
    if out_edges.is_empty() {
        writeln!(out, "  (none)")?;
    } else {
        for e in out_edges {
            writeln!(out, "  {} ({})", e.to, e.kind)?;
        }
    }

    writeln!(out, "required by ({}):", in_edges.len())?;
    if in_edges.is_empty() {
        writeln!(out, "  (none)")?;
    } else {
        for e in in_edges {
            writeln!(out, "  {} ({})", e.from, e.kind)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct DepsJsonOutput<'a> {
    prd: &'a str,
    depends_on: Vec<&'a Edge>,
    required_by: Vec<&'a Edge>,
    unresolved: usize,
}

fn deps_json(
    filename: &str,
    out_edges: &[&Edge],
    in_edges: &[&Edge],
    unresolved: usize,
) -> Result<()> {
    let output = DepsJsonOutput {
        prd: filename,
        depends_on: out_edges.to_vec(),
        required_by: in_edges.to_vec(),
        unresolved,
    };
    let json = serde_json::to_string_pretty(&output)?;
    writeln!(io::stdout().lock(), "{json}")?;
    Ok(())
}

// ── blocked ───────────────────────────────────────────────────────────────────

/// Output for `atlas blocked [--format ...]`.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn blocked(graph: &Graph, json: bool) -> Result<()> {
    // A PRD is blocked if ≥1 of its out-edges points to a PRD that is not shipped.
    let mut blocked_prds: Vec<BlockedEntry> = Vec::new();

    for prd in &graph.prds {
        let out_edges = graph
            .edge_index
            .out_edges(&graph.edge_set.edges, &prd.filename);
        let blocking: Vec<&Edge> = out_edges
            .into_iter()
            .filter(|e| {
                // Find the dependency's status.
                graph
                    .prds
                    .iter()
                    .find(|p| p.filename == e.to)
                    .is_some_and(|dep| dep.status != PrdStatus::Shipped)
            })
            .collect();

        if !blocking.is_empty() {
            blocked_prds.push(BlockedEntry {
                prd: prd.filename.clone(),
                blocking_deps: blocking.iter().map(|e| (*e).clone()).collect(),
            });
        }
    }

    if json {
        blocked_json(&blocked_prds)
    } else {
        blocked_text(&blocked_prds)
    }
}

#[derive(serde::Serialize)]
struct BlockedEntry {
    prd: String,
    blocking_deps: Vec<Edge>,
}

fn blocked_text(entries: &[BlockedEntry]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if entries.is_empty() {
        writeln!(out, "(no PRDs are blocked)")?;
        return Ok(());
    }

    for entry in entries {
        writeln!(out, "blocked: {}", entry.prd)?;
        for dep in &entry.blocking_deps {
            writeln!(out, "  waiting on {} ({})", dep.to, dep.kind)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct BlockedJsonOutput<'a> {
    blocked: &'a [BlockedEntry],
}

fn blocked_json(entries: &[BlockedEntry]) -> Result<()> {
    let output = BlockedJsonOutput { blocked: entries };
    let json = serde_json::to_string_pretty(&output)?;
    writeln!(io::stdout().lock(), "{json}")?;
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Normalise a CLI PRD argument to a filename.
///
/// Accepts either `PRD-atlas-edges.md` (full filename) or `atlas-edges` (slug).
fn normalise_prd_arg(arg: &str) -> String {
    let trimmed = arg.trim();
    let has_md_ext = std::path::Path::new(trimmed)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    if trimmed.starts_with("PRD-") && has_md_ext {
        trimmed.to_string()
    } else if trimmed.starts_with("PRD-") {
        format!("{trimmed}.md")
    } else {
        format!("PRD-{trimmed}.md")
    }
}
