//! `atlas doctor` — read-only corpus divergence lint.
//!
//! Reports five classes of divergence that are invisible to manual inspection
//! across a large corpus. **Never writes to any file.** All filesystem
//! existence checks for `shipped_repo_gone` go through env-overridable roots
//! via [`crate::parsers::Sources`], so tests never touch `~/wintermute`.
//!
//! ## Finding classes
//!
//! | class                | severity | definition                                                        |
//! |----------------------|----------|-------------------------------------------------------------------|
//! | `prd_no_vision`      | warn     | PRD whose `Vision:` is empty or names a missing vision doc        |
//! | `vision_no_prd`      | info     | vision with empty `prds_drafted` and no PRD pointing at it       |
//! | `repo_no_prd`        | warn     | REPOS.md entry with no PRD mapping to it                         |
//! | `shipped_repo_gone`  | warn     | manifest-shipped PRD whose `output_repo_path` does not exist     |
//! | `fulfilled_unmarked` | info     | vision marked `active` whose every drafted PRD is shipped        |
//!
//! ## Exit codes
//!
//! - `0` — no findings
//! - `1` — only info-level findings (`vision_no_prd`, `fulfilled_unmarked`)
//! - `2` — any warn-level finding (`prd_no_vision`, `repo_no_prd`, `shipped_repo_gone`)

use std::collections::HashSet;

use serde::Serialize;

use crate::graph::Graph;
use crate::model::PrdStatus;

// ── Finding types ─────────────────────────────────────────────────────────────

/// Severity level for a finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Advisory: worth fixing but not urgent.
    Info,
    /// Should be investigated: indicates drift.
    Warn,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
        }
    }
}

/// A divergence class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingClass {
    /// PRD's `Vision:` field is empty or names a non-existent vision.
    PrdNoVision,
    /// Vision has no PRDs drafted and no PRD points at it.
    VisionNoPrd,
    /// REPOS.md entry has no originating PRD.
    RepoNoPrd,
    /// Build manifest marks a PRD shipped but `output_repo_path` is gone.
    ShippedRepoGone,
    /// Vision is `active` but all its drafted PRDs are shipped.
    FulfilledUnmarked,
}

impl std::fmt::Display for FindingClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrdNoVision => write!(f, "prd_no_vision"),
            Self::VisionNoPrd => write!(f, "vision_no_prd"),
            Self::RepoNoPrd => write!(f, "repo_no_prd"),
            Self::ShippedRepoGone => write!(f, "shipped_repo_gone"),
            Self::FulfilledUnmarked => write!(f, "fulfilled_unmarked"),
        }
    }
}

impl FindingClass {
    /// Severity for this class.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        match self {
            Self::PrdNoVision | Self::RepoNoPrd | Self::ShippedRepoGone => Severity::Warn,
            Self::VisionNoPrd | Self::FulfilledUnmarked => Severity::Info,
        }
    }
}

/// A single divergence finding.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    /// Divergence class.
    pub class: FindingClass,
    /// Offending node identifier (filename, slug, or repo name).
    pub node: String,
    /// One-line human-readable description.
    pub detail: String,
    /// Source file where the offending entry was found.
    pub source: String,
}

impl Finding {
    /// Severity (derived from class).
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.class.severity()
    }
}

/// Filter for which class to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassFilter {
    /// Report all classes.
    All,
    /// Report only this class.
    Only(FindingClass),
}

// ── Runner ────────────────────────────────────────────────────────────────────

/// Run the doctor lint over `graph` and return all findings (unsorted, order
/// within a class is deterministic per the order nodes were loaded).
///
/// `filter` restricts which class is emitted; use [`ClassFilter::All`] for the
/// full report.
///
/// The function is pure: it never writes any file.
#[must_use]
pub fn run(graph: &Graph, filter: &ClassFilter) -> Vec<Finding> {
    let mut findings = Vec::new();

    if matches!(filter, ClassFilter::All)
        || matches!(filter, ClassFilter::Only(FindingClass::PrdNoVision))
    {
        findings.extend(check_prd_no_vision(graph));
    }
    if matches!(filter, ClassFilter::All)
        || matches!(filter, ClassFilter::Only(FindingClass::VisionNoPrd))
    {
        findings.extend(check_vision_no_prd(graph));
    }
    if matches!(filter, ClassFilter::All)
        || matches!(filter, ClassFilter::Only(FindingClass::RepoNoPrd))
    {
        findings.extend(check_repo_no_prd(graph));
    }
    if matches!(filter, ClassFilter::All)
        || matches!(filter, ClassFilter::Only(FindingClass::ShippedRepoGone))
    {
        findings.extend(check_shipped_repo_gone(graph));
    }
    if matches!(filter, ClassFilter::All)
        || matches!(filter, ClassFilter::Only(FindingClass::FulfilledUnmarked))
    {
        findings.extend(check_fulfilled_unmarked(graph));
    }

    findings
}

/// Compute the exit code from a set of findings.
///
/// - `0` → no findings
/// - `1` → only info-level
/// - `2` → any warn-level
#[must_use]
pub fn exit_code(findings: &[Finding]) -> i32 {
    if findings.is_empty() {
        return 0;
    }
    if findings.iter().any(|f| f.severity() == Severity::Warn) {
        2
    } else {
        1
    }
}

// ── Individual checks ─────────────────────────────────────────────────────────

/// Class `prd_no_vision`: PRDs whose `vision` field is empty or names a vision
/// that does not exist in the graph.
fn check_prd_no_vision(graph: &Graph) -> Vec<Finding> {
    let known_vision_slugs: HashSet<&str> =
        graph.visions.iter().map(|v| v.slug.as_str()).collect();

    graph
        .prds
        .iter()
        .filter_map(|prd| {
            if prd.vision.is_empty() {
                Some(Finding {
                    class: FindingClass::PrdNoVision,
                    node: prd.filename.clone(),
                    detail: "PRD has no Vision: field".to_string(),
                    source: prd.filename.clone(),
                })
            } else if !known_vision_slugs.contains(prd.vision.as_str()) {
                Some(Finding {
                    class: FindingClass::PrdNoVision,
                    node: prd.filename.clone(),
                    detail: format!(
                        "Vision: '{}' not found in dream manifest",
                        prd.vision
                    ),
                    source: prd.filename.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Class `vision_no_prd`: visions that have an empty `prds_drafted` list AND
/// no PRD in the graph has its `vision` field pointing at that slug.
fn check_vision_no_prd(graph: &Graph) -> Vec<Finding> {
    // Build set of vision slugs that at least one PRD references.
    let referenced_slugs: HashSet<&str> = graph
        .prds
        .iter()
        .filter(|p| !p.vision.is_empty())
        .map(|p| p.vision.as_str())
        .collect();

    graph
        .visions
        .iter()
        .filter_map(|v| {
            let no_prds_drafted = v.prds_drafted.is_empty();
            let not_referenced = !referenced_slugs.contains(v.slug.as_str());
            if no_prds_drafted && not_referenced {
                Some(Finding {
                    class: FindingClass::VisionNoPrd,
                    node: v.slug.clone(),
                    detail: "vision has no drafted PRDs and no PRD references it".to_string(),
                    source: v.path.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Class `repo_no_prd`: repos in REPOS.md whose name does not match any PRD's
/// title, slug, `build_into` basename, or `output_repo_path` basename.
fn check_repo_no_prd(graph: &Graph) -> Vec<Finding> {
    graph
        .repos
        .iter()
        .filter_map(|repo| {
            let has_matching_prd = graph.prds.iter().any(|prd| repo_matches_prd(repo, prd));
            if has_matching_prd {
                None
            } else {
                Some(Finding {
                    class: FindingClass::RepoNoPrd,
                    node: repo.name.clone(),
                    detail: format!(
                        "repo '{}' has no originating PRD (no PRD title/path maps to it)",
                        repo.name
                    ),
                    source: "REPOS.md".to_string(),
                })
            }
        })
        .collect()
}

/// Determine whether a repo entry plausibly originates from a given PRD.
///
/// Checks:
/// 1. Repo name appears in the PRD title (case-insensitive).
/// 2. PRD `build_into` basename matches repo name.
/// 3. PRD `repo_path` basename matches repo name.
fn repo_matches_prd(repo: &crate::model::RepoNode, prd: &crate::model::PrdNode) -> bool {
    let repo_lower = repo.name.to_lowercase();

    // 1. Repo name is a substring of the PRD title.
    if prd.title.to_lowercase().contains(&repo_lower) {
        return true;
    }
    // Also check in the PRD filename slug (PRD-atlas-core → "atlas-core" ~ "atlas").
    let prd_slug = prd
        .filename
        .strip_prefix("PRD-")
        .and_then(|s| s.strip_suffix(".md"))
        .unwrap_or("");
    if prd_slug.starts_with(&repo_lower) || prd_slug.contains(&repo_lower) {
        return true;
    }

    // 2. PRD's build_into basename.
    if !prd.build_into.is_empty() {
        let into_base = std::path::Path::new(&prd.build_into)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if into_base.eq_ignore_ascii_case(&repo.name) {
            return true;
        }
    }

    // 3. PRD's shipped repo_path basename.
    if !prd.repo_path.is_empty() {
        let path_base = std::path::Path::new(&prd.repo_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if path_base.eq_ignore_ascii_case(&repo.name) {
            return true;
        }
    }

    false
}

/// Class `shipped_repo_gone`: PRDs the build manifest marks as shipped whose
/// `output_repo_path` does not exist on disk.
///
/// Only fires when `output_repo_path` is non-empty AND the path is absent —
/// PRDs with no `output_repo_path` are not flagged (they may not have a local
/// repo even when shipped, e.g. pre-`/build` shipped entries).
fn check_shipped_repo_gone(graph: &Graph) -> Vec<Finding> {
    graph
        .prds
        .iter()
        .filter_map(|prd| {
            // Only fire on PRDs that are in-flight or "shipped" by manifest status,
            // but have a non-empty repo_path that doesn't exist.
            // We fire when: repo_path is non-empty AND the path is missing on disk.
            // This catches: manifest says shipped, path recorded, but path is gone.
            if prd.repo_path.is_empty() {
                return None;
            }
            // If status is already Shipped (path exists), no finding.
            if prd.status == PrdStatus::Shipped {
                return None;
            }
            // At this point: repo_path is non-empty AND path does NOT exist
            // (otherwise status would be Shipped). This is the divergence.
            Some(Finding {
                class: FindingClass::ShippedRepoGone,
                node: prd.filename.clone(),
                detail: format!(
                    "output_repo_path '{}' does not exist on disk",
                    prd.repo_path
                ),
                source: prd.filename.clone(),
            })
        })
        .collect()
}

/// Class `fulfilled_unmarked`: visions marked `active` in the dream manifest
/// whose every PRD listed in `prds_drafted` derives status `Shipped`.
///
/// A vision with zero `prds_drafted` is NOT reported (nothing to fulfill).
/// A vision with at least one un-shipped PRD is NOT reported.
fn check_fulfilled_unmarked(graph: &Graph) -> Vec<Finding> {
    graph
        .visions
        .iter()
        .filter_map(|v| {
            // Only active visions.
            if v.status != "active" {
                return None;
            }
            // Must have at least one drafted PRD.
            if v.prds_drafted.is_empty() {
                return None;
            }
            // Check every drafted PRD slug → derive status.
            let all_shipped = v.prds_drafted.iter().all(|slug| {
                let filename = format!("PRD-{slug}.md");
                graph
                    .prds
                    .iter()
                    .find(|p| p.filename == filename)
                    .is_some_and(|p| p.status == PrdStatus::Shipped)
            });
            if all_shipped {
                Some(Finding {
                    class: FindingClass::FulfilledUnmarked,
                    node: v.slug.clone(),
                    detail: format!(
                        "vision '{}' is active but all {} drafted PRDs are shipped",
                        v.slug,
                        v.prds_drafted.len()
                    ),
                    source: v.path.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

// ── Output helpers ────────────────────────────────────────────────────────────

/// Render findings as human-readable text to the given writer.
///
/// When there are no findings, prints a clean-bill line.
///
/// # Errors
///
/// Returns an error if writing to `out` fails.
pub fn render_text(findings: &[Finding], out: &mut impl std::io::Write) -> std::io::Result<()> {
    if findings.is_empty() {
        writeln!(out, "atlas doctor: no divergences found — corpus is clean")?;
        return Ok(());
    }
    for f in findings {
        writeln!(
            out,
            "{sev}  {class}  {node}  {detail}  (source: {source})",
            sev = f.severity(),
            class = f.class,
            node = f.node,
            detail = f.detail,
            source = f.source,
        )?;
    }
    Ok(())
}

/// Render findings as a JSON array to the given writer.
///
/// Each element has `class`, `node`, `detail`, `source`.
///
/// # Errors
///
/// Returns an error if serialization or writing fails.
pub fn render_json(findings: &[Finding], out: &mut impl std::io::Write) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(findings)?;
    writeln!(out, "{json}")?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::model::{PrdNode, PrdStatus, RepoNode, VisionNode};
    use crate::edges::{EdgeIndex, EdgeSet};

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn empty_graph() -> Graph {
        Graph {
            visions: Vec::new(),
            prds: Vec::new(),
            repos: Vec::new(),
            edge_set: EdgeSet::default(),
            edge_index: EdgeIndex::default(),
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

    fn prd_with_path(filename: &str, status: PrdStatus, repo_path: &str) -> PrdNode {
        PrdNode {
            filename: filename.to_string(),
            title: filename.to_string(),
            vision: String::new(),
            build_target: "rust-cli".to_string(),
            build_into: String::new(),
            status,
            repo_url: String::new(),
            repo_path: repo_path.to_string(),
        }
    }

    fn vision(slug: &str, status: &str, prds_drafted: &[&str]) -> VisionNode {
        VisionNode {
            slug: slug.to_string(),
            path: format!("visions/{slug}.md"),
            status: status.to_string(),
            prds_drafted: prds_drafted.iter().map(|s| s.to_string()).collect(),
            seed: String::new(),
        }
    }

    fn repo(name: &str) -> RepoNode {
        RepoNode {
            name: name.to_string(),
            url: format!("https://github.com/j0yen/{name}"),
            description: format!("the {name} tool"),
        }
    }

    // ── prd_no_vision: positive fixture (triggers the finding) ────────────────

    #[test]
    fn prd_no_vision_fires_when_vision_empty() {
        let mut g = empty_graph();
        g.prds.push(prd("PRD-foo.md", "", PrdStatus::Drafted));
        let findings = check_prd_no_vision(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::PrdNoVision);
        assert_eq!(findings[0].node, "PRD-foo.md");
    }

    #[test]
    fn prd_no_vision_fires_when_vision_missing_from_graph() {
        let mut g = empty_graph();
        g.prds.push(prd("PRD-foo.md", "ghost-vision", PrdStatus::Drafted));
        // No visions in graph.
        let findings = check_prd_no_vision(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::PrdNoVision);
    }

    // ── prd_no_vision: negative fixture (must NOT fire) ──────────────────────

    #[test]
    fn prd_no_vision_absent_when_vision_exists() {
        let mut g = empty_graph();
        g.visions.push(vision("atlas", "active", &[]));
        g.prds.push(prd("PRD-atlas-core.md", "atlas", PrdStatus::Drafted));
        let findings = check_prd_no_vision(&g);
        assert!(findings.is_empty(), "known vision must not fire prd_no_vision");
    }

    // ── vision_no_prd: positive fixture ──────────────────────────────────────

    #[test]
    fn vision_no_prd_fires_when_no_prds_and_not_referenced() {
        let mut g = empty_graph();
        g.visions.push(vision("empty-vision", "active", &[]));
        // No PRDs reference "empty-vision".
        let findings = check_vision_no_prd(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::VisionNoPrd);
        assert_eq!(findings[0].node, "empty-vision");
    }

    // ── vision_no_prd: negative fixture ──────────────────────────────────────

    #[test]
    fn vision_no_prd_absent_when_prd_references_vision() {
        let mut g = empty_graph();
        g.visions.push(vision("atlas", "active", &[]));
        g.prds.push(prd("PRD-atlas-core.md", "atlas", PrdStatus::Drafted));
        let findings = check_vision_no_prd(&g);
        assert!(
            findings.is_empty(),
            "vision referenced by a PRD must not fire vision_no_prd"
        );
    }

    // ── repo_no_prd: positive fixture ─────────────────────────────────────────

    #[test]
    fn repo_no_prd_fires_when_no_matching_prd() {
        let mut g = empty_graph();
        g.repos.push(repo("orphan-tool"));
        // No PRDs that map to "orphan-tool".
        let findings = check_repo_no_prd(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::RepoNoPrd);
        assert_eq!(findings[0].node, "orphan-tool");
    }

    // ── repo_no_prd: negative fixture ────────────────────────────────────────

    #[test]
    fn repo_no_prd_absent_when_prd_build_into_matches() {
        let mut g = empty_graph();
        g.repos.push(repo("atlas"));
        let mut p = prd("PRD-atlas-core.md", "atlas", PrdStatus::Shipped);
        p.build_into = "/home/jsy/wintermute/atlas".to_string();
        g.prds.push(p);
        g.visions.push(vision("atlas", "active", &[]));
        let findings = check_repo_no_prd(&g);
        assert!(
            findings.is_empty(),
            "repo with matching build_into must not fire repo_no_prd"
        );
    }

    // ── shipped_repo_gone: positive fixture ──────────────────────────────────

    #[test]
    fn shipped_repo_gone_fires_when_path_non_empty_and_not_on_disk() {
        let mut g = empty_graph();
        // A PRD with a repo_path that doesn't exist → status is InFlight, not Shipped.
        // (derive_prd_status in parsers.rs sets Shipped only when path exists.)
        g.prds.push(prd_with_path(
            "PRD-lost.md",
            PrdStatus::InFlight,
            "/nonexistent/path/to/repo",
        ));
        let findings = check_shipped_repo_gone(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::ShippedRepoGone);
        assert_eq!(findings[0].node, "PRD-lost.md");
    }

    // ── shipped_repo_gone: negative fixture ──────────────────────────────────

    #[test]
    fn shipped_repo_gone_absent_when_path_exists_on_disk() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut g = empty_graph();
        // Path exists → derive_prd_status returns Shipped → no finding.
        g.prds.push(prd_with_path(
            "PRD-ok.md",
            PrdStatus::Shipped,
            tmp.path().to_str().unwrap(),
        ));
        let findings = check_shipped_repo_gone(&g);
        assert!(
            findings.is_empty(),
            "existing path must not fire shipped_repo_gone"
        );
    }

    #[test]
    fn shipped_repo_gone_absent_when_path_empty() {
        let mut g = empty_graph();
        // No repo_path set → no finding.
        g.prds.push(prd("PRD-no-path.md", "", PrdStatus::InFlight));
        let findings = check_shipped_repo_gone(&g);
        assert!(findings.is_empty(), "empty repo_path must not fire finding");
    }

    // ── fulfilled_unmarked: positive fixture ─────────────────────────────────

    #[test]
    fn fulfilled_unmarked_fires_when_all_prds_shipped() {
        let mut g = empty_graph();
        g.visions.push(vision("fleet", "active", &["fleet-core", "fleet-edges"]));
        g.prds.push(prd("PRD-fleet-core.md", "fleet", PrdStatus::Shipped));
        g.prds.push(prd("PRD-fleet-edges.md", "fleet", PrdStatus::Shipped));
        let findings = check_fulfilled_unmarked(&g);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].class, FindingClass::FulfilledUnmarked);
        assert_eq!(findings[0].node, "fleet");
    }

    // ── fulfilled_unmarked: negative — one PRD not shipped ───────────────────

    #[test]
    fn fulfilled_unmarked_absent_when_one_prd_not_shipped() {
        let mut g = empty_graph();
        g.visions.push(vision("fleet", "active", &["fleet-core", "fleet-orphans"]));
        g.prds.push(prd("PRD-fleet-core.md", "fleet", PrdStatus::Shipped));
        g.prds.push(prd("PRD-fleet-orphans.md", "fleet", PrdStatus::InFlight));
        let findings = check_fulfilled_unmarked(&g);
        assert!(
            findings.is_empty(),
            "vision with an un-shipped PRD must not fire fulfilled_unmarked"
        );
    }

    // ── fulfilled_unmarked: negative — vision not active ─────────────────────

    #[test]
    fn fulfilled_unmarked_absent_when_vision_not_active() {
        let mut g = empty_graph();
        g.visions.push(vision("done-vision", "complete", &["done-prd"]));
        g.prds.push(prd("PRD-done-prd.md", "done-vision", PrdStatus::Shipped));
        let findings = check_fulfilled_unmarked(&g);
        assert!(
            findings.is_empty(),
            "non-active vision must not fire fulfilled_unmarked"
        );
    }

    // ── exit_code ─────────────────────────────────────────────────────────────

    #[test]
    fn exit_code_zero_on_empty() {
        assert_eq!(exit_code(&[]), 0);
    }

    #[test]
    fn exit_code_one_on_info_only() {
        let f = Finding {
            class: FindingClass::FulfilledUnmarked,
            node: "x".to_string(),
            detail: "d".to_string(),
            source: "s".to_string(),
        };
        assert_eq!(exit_code(&[f]), 1);
    }

    #[test]
    fn exit_code_two_on_warn() {
        let f = Finding {
            class: FindingClass::ShippedRepoGone,
            node: "x".to_string(),
            detail: "d".to_string(),
            source: "s".to_string(),
        };
        assert_eq!(exit_code(&[f]), 2);
    }

    // ── render_text: clean-bill on empty ──────────────────────────────────────

    #[test]
    fn render_text_clean_bill_when_no_findings() {
        let mut buf = Vec::new();
        render_text(&[], &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("no divergences"), "clean-bill text expected");
    }

    // ── render_json: valid JSON array ─────────────────────────────────────────

    #[test]
    fn render_json_emits_valid_array() {
        let findings = vec![Finding {
            class: FindingClass::PrdNoVision,
            node: "PRD-foo.md".to_string(),
            detail: "PRD has no Vision: field".to_string(),
            source: "PRD-foo.md".to_string(),
        }];
        let mut buf = Vec::new();
        render_json(&findings, &mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let obj = &arr[0];
        assert_eq!(obj["class"], "prd_no_vision");
        assert_eq!(obj["node"], "PRD-foo.md");
        assert!(obj.get("detail").is_some());
        assert!(obj.get("source").is_some());
    }

    // ── run: class filter ─────────────────────────────────────────────────────

    #[test]
    fn run_filter_only_prd_no_vision() {
        let mut g = empty_graph();
        g.prds.push(prd("PRD-foo.md", "", PrdStatus::Drafted));
        g.visions.push(vision("empty-vis", "active", &[]));
        // Both prd_no_vision and vision_no_prd would fire on AllClasses.
        // With filter=Only(PrdNoVision) we only get prd_no_vision.
        let findings = run(&g, &ClassFilter::Only(FindingClass::PrdNoVision));
        assert!(
            findings.iter().all(|f| f.class == FindingClass::PrdNoVision),
            "class filter must restrict output to the requested class"
        );
        assert!(!findings.is_empty(), "filter must still return matching findings");
    }
}
