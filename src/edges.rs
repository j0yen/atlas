//! Edge parsers for the atlas dependency graph.
//!
//! Two sources are supported:
//!
//! 1. **Frontmatter edges** — `**Depends on:** PRD-foo.md, PRD-bar.md` lines
//!    in each PRD file.  These are authoritative.
//! 2. **Gossip edges** — `Order:` blocks in `notes/gossip.md` that name
//!    PRD slugs connected with `→`, `->`, or `depends on`.  Best-effort.
//!
//! Deduplication rule: if both sources declare the same `(from, to)` pair,
//! only the frontmatter edge is kept.  A gossip edge that duplicates a
//! frontmatter edge is silently dropped (not counted in `unresolved`).
//!
//! Unresolved endpoints (a named PRD that does not exist in the known
//! filename set) are dropped and counted.

use crate::model::{Edge, EdgeKind};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::path::Path;

/// Result of parsing all edges from both sources.
#[derive(Debug, Default)]
pub struct EdgeSet {
    /// All resolved edges (deduplicated).
    pub edges: Vec<Edge>,
    /// Count of edge endpoints that could not be resolved to a known PRD.
    pub unresolved: usize,
}

/// Parse frontmatter `**Depends on:**` edges from PRD files.
///
/// Each PRD file is scanned for a `**Depends on:** <list>` line.  The list
/// may be comma-separated and may contain `none` (which produces no edges).
/// Each non-`none` item is resolved against `known_filenames`; unresolvable
/// items increment `unresolved`.
///
/// Lines inside fenced code blocks are skipped.
///
/// # Errors
///
/// Returns an error if the autobuilder directory cannot be read.
pub fn parse_frontmatter_edges<S: BuildHasher>(
    autobuilder_dir: &Path,
    known_filenames: &HashSet<String, S>,
) -> anyhow::Result<(Vec<Edge>, usize)> {
    let mut edges = Vec::new();
    let mut unresolved = 0usize;

    let dir_entries = std::fs::read_dir(autobuilder_dir).map_err(|e| {
        anyhow::anyhow!("reading autobuilder dir {}: {}", autobuilder_dir.display(), e)
    })?;

    for dir_entry in dir_entries {
        let dir_entry = dir_entry?;
        let fname = dir_entry.file_name();
        let filename = fname.to_string_lossy().to_string();

        if !filename.starts_with("PRD-")
            || !Path::new(&filename)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        // Skip archived PRDs.
        if dir_entry
            .path()
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "PRDs-archive")
        {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(dir_entry.path()) else {
            continue;
        };

        let file_edges = extract_depends_on_edges(&filename, &content, known_filenames);
        for result in file_edges {
            match result {
                Ok(edge) => edges.push(edge),
                Err(()) => unresolved += 1,
            }
        }
    }

    Ok((edges, unresolved))
}

/// Extract `**Depends on:**` edges from a single PRD's content.
///
/// Returns a vec of `Ok(Edge)` for resolved deps and `Err(())` for each
/// unresolved name.
fn extract_depends_on_edges<S: BuildHasher>(
    filename: &str,
    content: &str,
    known_filenames: &HashSet<String, S>,
) -> Vec<Result<Edge, ()>> {
    let mut results = Vec::new();
    let mut in_code_block = false;

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Match `**Depends on:** <value>` (bold key, colon, space, value).
        let Some(raw_value) = extract_bold_field(trimmed, "Depends on:") else {
            continue;
        };

        // Strip trailing inline comment.
        let value = raw_value
            .find(" # ")
            .map_or(raw_value, |pos| &raw_value[..pos])
            .trim();

        // Split comma-separated list.
        for part in value.split(',') {
            let dep = part.trim();
            if dep.is_empty() || dep.eq_ignore_ascii_case("none") {
                continue;
            }
            // Normalise: strip optional trailing parenthetical like "(the node model)".
            let dep_name = dep
                .split_once(" (")
                .map_or(dep, |(name, _)| name.trim());

            // Ensure it looks like a PRD filename.
            let dep_filename = normalise_prd_filename(dep_name);

            if known_filenames.contains(&dep_filename) {
                results.push(Ok(Edge {
                    from: filename.to_string(),
                    to: dep_filename,
                    kind: EdgeKind::Frontmatter,
                    source: format!("{filename}:{}", line_no + 1),
                }));
            } else {
                results.push(Err(()));
            }
        }
    }

    results
}

/// Parse gossip `Order:` edges from a gossip file.
///
/// Scans the gossip file for `Order:` lines and subsequent continuation lines
/// that contain `→`, `->`, or `depends on`.  Extracts named PRD slugs (slugs
/// are turned into `PRD-<slug>.md` filenames) and resolved against
/// `known_filenames`.  Unresolvable names increment `unresolved`.
///
/// # Errors
///
/// Returns an error if the gossip file cannot be read.
pub fn parse_gossip_edges<S1: BuildHasher, S2: BuildHasher>(
    gossip_path: &Path,
    known_filenames: &HashSet<String, S1>,
    frontmatter_pairs: &HashSet<(String, String), S2>,
) -> anyhow::Result<(Vec<Edge>, usize)> {
    let content = std::fs::read_to_string(gossip_path).map_err(|e| {
        anyhow::anyhow!("reading gossip file {}: {}", gossip_path.display(), e)
    })?;

    let mut edges = Vec::new();
    let mut unresolved = 0usize;

    // We collect line numbers for provenance.
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let Some(line_ref) = lines.get(i) else { break };
        let line = line_ref.trim();

        // Look for `Order:` keyword (possibly followed by content on the same line).
        if !line.starts_with("Order:") {
            i += 1;
            continue;
        }

        let order_line_no = i + 1; // 1-based

        // Collect the Order block: the `Order:` line plus continuation lines
        // that contain arrows / `depends on` / slug-like tokens, until we hit
        // an empty line or a new section marker.
        let mut block_lines: Vec<(usize, &str)> = Vec::new();

        // The Order: line itself may carry content after the colon.
        let inline = line.strip_prefix("Order:").unwrap_or("").trim();
        if !inline.is_empty() {
            block_lines.push((order_line_no, inline));
        }

        // Gather continuation lines.
        let mut j = i + 1;
        while j < lines.len() {
            let Some(cont_ref) = lines.get(j) else { break };
            let cont = cont_ref.trim();
            // Stop at blank line or new section / comment marker.
            if cont.is_empty() || cont.starts_with("##") || cont.starts_with("//") {
                break;
            }
            // Only include lines that have arrow-like or dependency tokens.
            if has_arrow_token(cont) || cont.starts_with("Order:") {
                if cont.starts_with("Order:") {
                    break; // New Order: block; reparse from top.
                }
                block_lines.push((j + 1, cont));
            } else {
                // Non-arrow continuation is part of the block but may still have
                // slug-like tokens in parenthetical; skip for simplicity.
                break;
            }
            j += 1;
        }

        // Extract ordered slug pairs from the block.
        let block_edges =
            extract_edges_from_order_block(&block_lines, known_filenames, frontmatter_pairs);
        for result in block_edges {
            match result {
                Ok((edge, line_no)) => {
                    let source_file = gossip_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("gossip.md");
                    edges.push(Edge {
                        from: edge.0,
                        to: edge.1,
                        kind: EdgeKind::Gossip,
                        source: format!("{source_file}:{line_no}"),
                    });
                }
                Err(()) => unresolved += 1,
            }
        }

        i += 1;
    }

    Ok((edges, unresolved))
}

/// Check whether a line contains an arrow-like token (gossip convention).
fn has_arrow_token(line: &str) -> bool {
    line.contains('→')
        || line.contains("->")
        || line.contains("depends on")
        || line.contains("after")
}

/// Extract `(from_filename, to_filename, source_line_no)` pairs from an
/// `Order:` block's lines.  Returns `Ok((from, to), lineno)` on resolution,
/// Resolved gossip edge: `(from, to)` filenames and 1-based source line.
type OrderEdgeResult = Result<((String, String), usize), ()>;

/// `Err(())` for unresolved endpoint.
fn extract_edges_from_order_block<S1: BuildHasher, S2: BuildHasher>(
    block_lines: &[(usize, &str)],
    known_filenames: &HashSet<String, S1>,
    frontmatter_pairs: &HashSet<(String, String), S2>,
) -> Vec<OrderEdgeResult> {
    let mut results = Vec::new();

    // Flatten all lines into a single token stream, remembering the line each
    // slug came from so we can attach provenance.
    // Strategy: split each line on `→`, `->`, `,`, `||`; each token may be a
    // PRD slug.  We emit an edge for each consecutive (left, right) pair.

    for &(line_no, line) in block_lines {
        // Normalize arrows to a common separator.
        let normalised = line
            .replace('→', " ARROW ")
            .replace("->", " ARROW ")
            .replace("||", " ARROW ");

        let parts: Vec<&str> = normalised.split("ARROW").collect();
        if parts.len() < 2 {
            continue;
        }

        for window in parts.windows(2) {
            let left = window.first().and_then(|s| extract_slug_from_token(s));
            let right = window.get(1).and_then(|s| extract_slug_from_token(s));

            // In an Order block `A → B` means "A runs first, then B".
            // Semantically B *depends on* A, so the dependency edge is
            // from=B (the dependent) to=A (the prerequisite).
            let (Some(prereq_slug), Some(dependent_slug)) = (left, right) else {
                continue;
            };

            let from_file = normalise_prd_filename(&dependent_slug);
            let to_file = normalise_prd_filename(&prereq_slug);

            // Drop if endpoint is unknown.
            if !known_filenames.contains(&from_file) || !known_filenames.contains(&to_file) {
                results.push(Err(()));
                continue;
            }

            // Drop if already covered by frontmatter.
            if frontmatter_pairs.contains(&(from_file.clone(), to_file.clone())) {
                continue;
            }

            results.push(Ok(((from_file, to_file), line_no)));
        }
    }

    results
}

/// Extract the first slug-like token from a noisy gossip fragment.
///
/// A slug is a kebab-case word, possibly with digits.  We strip parentheticals,
/// commentary keywords (FIRST, INDEPENDENT, etc.), and whitespace.
fn extract_slug_from_token(raw: &str) -> Option<String> {
    // Strip parenthetical comments, strip trailing noise.
    let cleaned = raw
        .split('(')
        .next()
        .unwrap_or(raw)
        .trim()
        // Strip surrounding braces used in gossip like `{ foo, bar }`
        .trim_matches(|c: char| c == '{' || c == '}')
        .trim();

    // Split on whitespace; look for a token that looks like a PRD slug.
    for token in cleaned.split_whitespace() {
        let tok = token
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
            .trim();
        if tok.is_empty() {
            continue;
        }
        // Skip all-uppercase noise words.
        if tok.chars().all(|c| c.is_ascii_uppercase() || c == '-') {
            continue;
        }
        // A slug looks like `word(-word)*` (kebab-case, no dots or slashes).
        if tok.contains('.') || tok.contains('/') {
            continue;
        }
        // Must have at least one lowercase letter.
        if !tok.chars().any(|c| c.is_ascii_lowercase()) {
            continue;
        }
        return Some(tok.to_lowercase());
    }
    None
}

/// Normalise a dep name to a PRD filename.
///
/// If it already ends in `.md` and starts with `PRD-`, return as-is.
/// If it looks like a slug (no extension), prepend `PRD-` and append `.md`.
fn normalise_prd_filename(name: &str) -> String {
    let trimmed = name.trim();
    let has_md_ext = std::path::Path::new(trimmed)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    if trimmed.starts_with("PRD-") && has_md_ext {
        return trimmed.to_string();
    }
    if trimmed.starts_with("PRD-") {
        return format!("{trimmed}.md");
    }
    format!("PRD-{trimmed}.md")
}

/// Extract the value from a `**Key:** value` bold-frontmatter line.
fn extract_bold_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("**{key}**");
    let rest = line.strip_prefix(&prefix)?.trim_start_matches(':').trim();
    Some(rest)
}

/// Build the full edge set from both sources.
///
/// This is the main entry point used by `Graph::load`.
///
/// # Errors
///
/// Propagates I/O errors from reading the autobuilder dir or gossip file.
pub fn load_edges<S: BuildHasher>(
    autobuilder_dir: &Path,
    gossip_path: Option<&Path>,
    known_filenames: &HashSet<String, S>,
) -> anyhow::Result<EdgeSet> {
    let (fm_edges, fm_unresolved) =
        parse_frontmatter_edges(autobuilder_dir, known_filenames)?;

    // Build set of (from, to) pairs already covered by frontmatter.
    let frontmatter_pairs: HashSet<(String, String)> = fm_edges
        .iter()
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();

    let (gossip_edges, gossip_unresolved) = if let Some(gp) = gossip_path {
        if gp.exists() {
            parse_gossip_edges(gp, known_filenames, &frontmatter_pairs)?
        } else {
            (Vec::new(), 0)
        }
    } else {
        (Vec::new(), 0)
    };

    let mut all_edges = fm_edges;
    all_edges.extend(gossip_edges);

    Ok(EdgeSet {
        edges: all_edges,
        unresolved: fm_unresolved + gossip_unresolved,
    })
}

/// Index edges by `from` and `to` for fast lookup.
#[derive(Debug, Default)]
pub struct EdgeIndex {
    /// Maps `from` filename → list of edges going out of that PRD.
    by_from: HashMap<String, Vec<usize>>,
    /// Maps `to` filename → list of edges coming into that PRD.
    by_to: HashMap<String, Vec<usize>>,
}

impl EdgeIndex {
    /// Build an index from an [`EdgeSet`].
    #[must_use]
    pub fn build(edge_set: &EdgeSet) -> Self {
        let mut by_from: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_to: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, edge) in edge_set.edges.iter().enumerate() {
            by_from.entry(edge.from.clone()).or_default().push(idx);
            by_to.entry(edge.to.clone()).or_default().push(idx);
        }
        Self { by_from, by_to }
    }

    /// Return edges going OUT of `filename` (what `filename` depends on).
    #[must_use]
    pub fn out_edges<'a>(&'a self, edges: &'a [Edge], filename: &str) -> Vec<&'a Edge> {
        self.by_from
            .get(filename)
            .map(|idxs| idxs.iter().filter_map(|&i| edges.get(i)).collect())
            .unwrap_or_default()
    }

    /// Return edges going INTO `filename` (what depends on `filename`).
    #[must_use]
    pub fn in_edges<'a>(&'a self, edges: &'a [Edge], filename: &str) -> Vec<&'a Edge> {
        self.by_to
            .get(filename)
            .map(|idxs| idxs.iter().filter_map(|&i| edges.get(i)).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_known(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // ── AC fixture helpers ────────────────────────────────────────────────────

    fn write_prd(dir: &Path, filename: &str, depends_on: Option<&str>) {
        let depends_line = match depends_on {
            Some(d) => format!("\n**Depends on:** {d}\n"),
            None => String::new(),
        };
        let content = format!(
            "# PRD: {filename}\n\n**build_target:** rust-extend\n{depends_line}\nBody text.\n"
        );
        fs::write(dir.join(filename), content).unwrap();
    }

    // ── AC: frontmatter-only ──────────────────────────────────────────────────

    #[test]
    fn ac_frontmatter_only_edge() {
        let tmp = TempDir::new().unwrap();
        let known = make_known(&["PRD-foo.md", "PRD-bar.md"]);

        write_prd(tmp.path(), "PRD-foo.md", Some("PRD-bar.md"));
        write_prd(tmp.path(), "PRD-bar.md", None);

        let (edges, unresolved) =
            parse_frontmatter_edges(tmp.path(), &known).unwrap();
        assert_eq!(edges.len(), 1, "expected one edge");
        assert_eq!(edges[0].from, "PRD-foo.md");
        assert_eq!(edges[0].to, "PRD-bar.md");
        assert_eq!(edges[0].kind, EdgeKind::Frontmatter);
        assert_eq!(unresolved, 0);
    }

    // ── AC: frontmatter `none` produces zero out-edges ────────────────────────

    #[test]
    fn ac_frontmatter_none_produces_no_edges() {
        let tmp = TempDir::new().unwrap();
        let known = make_known(&["PRD-docket-core.md"]);

        write_prd(tmp.path(), "PRD-docket-core.md", Some("none"));

        let (edges, unresolved) =
            parse_frontmatter_edges(tmp.path(), &known).unwrap();
        assert!(edges.is_empty(), "none must produce zero edges");
        assert_eq!(unresolved, 0);
    }

    // ── AC: gossip-only edge ──────────────────────────────────────────────────

    #[test]
    fn ac_gossip_only_edge() {
        let tmp = TempDir::new().unwrap();
        let known = make_known(&["PRD-atlas-core.md", "PRD-atlas-edges.md"]);

        let gossip = tmp.path().join("gossip.md");
        fs::write(
            &gossip,
            "Order: atlas-core → atlas-edges\n",
        )
        .unwrap();

        let fp: HashSet<(String, String)> = HashSet::new();
        let (edges, unresolved) =
            parse_gossip_edges(&gossip, &known, &fp).unwrap();
        assert_eq!(edges.len(), 1, "expected one gossip edge");
        // `atlas-core → atlas-edges` means "atlas-core first, atlas-edges second"
        // i.e. atlas-edges *depends on* atlas-core: from=atlas-edges, to=atlas-core.
        assert_eq!(edges[0].from, "PRD-atlas-edges.md");
        assert_eq!(edges[0].to, "PRD-atlas-core.md");
        assert_eq!(edges[0].kind, EdgeKind::Gossip);
        assert_eq!(unresolved, 0);
    }

    // ── AC: both agree → frontmatter wins, no duplicate ──────────────────────

    #[test]
    fn ac_both_agree_frontmatter_wins() {
        let tmp = TempDir::new().unwrap();
        let known = make_known(&["PRD-atlas-core.md", "PRD-atlas-edges.md"]);

        write_prd(
            tmp.path(),
            "PRD-atlas-edges.md",
            Some("PRD-atlas-core.md"),
        );
        write_prd(tmp.path(), "PRD-atlas-core.md", Some("none"));

        let gossip = tmp.path().join("gossip.md");
        fs::write(
            &gossip,
            "Order: atlas-core → atlas-edges\n",
        )
        .unwrap();

        let edge_set = load_edges(tmp.path(), Some(&gossip), &known).unwrap();
        // Exactly one edge; tagged frontmatter.
        assert_eq!(edge_set.edges.len(), 1, "dedup: exactly one edge");
        assert_eq!(edge_set.edges[0].kind, EdgeKind::Frontmatter);
    }

    // ── AC: both disagree endpoint — gossip names unknown PRD ────────────────

    #[test]
    fn ac_both_disagree_endpoint_gossip_unresolved() {
        let tmp = TempDir::new().unwrap();
        // known: only atlas-core; gossip mentions atlas-edges which doesn't exist.
        let known = make_known(&["PRD-atlas-core.md"]);

        let gossip = tmp.path().join("gossip.md");
        fs::write(
            &gossip,
            "Order: atlas-core → atlas-edges\n",
        )
        .unwrap();

        let fp: HashSet<(String, String)> = HashSet::new();
        let (edges, unresolved) =
            parse_gossip_edges(&gossip, &known, &fp).unwrap();
        assert!(edges.is_empty(), "unresolved endpoint must produce no edge");
        assert_eq!(unresolved, 1);
    }

    // ── AC: unresolved endpoint in frontmatter ────────────────────────────────

    #[test]
    fn ac_unresolved_frontmatter_endpoint() {
        let tmp = TempDir::new().unwrap();
        // known: only PRD-foo.md; it depends on non-existent PRD-ghost.md.
        let known = make_known(&["PRD-foo.md"]);

        write_prd(tmp.path(), "PRD-foo.md", Some("PRD-ghost.md"));

        let (edges, unresolved) =
            parse_frontmatter_edges(tmp.path(), &known).unwrap();
        assert!(edges.is_empty());
        assert_eq!(unresolved, 1);
    }

    // ── normalise_prd_filename ────────────────────────────────────────────────

    #[test]
    fn normalise_slug_to_filename() {
        assert_eq!(normalise_prd_filename("atlas-core"), "PRD-atlas-core.md");
        assert_eq!(normalise_prd_filename("PRD-atlas-core.md"), "PRD-atlas-core.md");
        assert_eq!(normalise_prd_filename("PRD-atlas-core"), "PRD-atlas-core.md");
    }
}
