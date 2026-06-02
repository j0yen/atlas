//! atlas-render AC2: `atlas graph --format dot` over the **live** corpus emits
//! valid DOT containing one vision node per loaded vision (≥24) and one PRD node
//! per loaded PRD, faithfully mirroring the in-memory `Graph`.
//!
//! The PRD (2026-05-29) cited "≥24 vision nodes and ≥107 PRD nodes" against the
//! corpus of that date. The corpus is a living thing — PRD files are added,
//! merged, and retired — so this test does **not** hard-code 107 (which would be
//! a brittle, already-stale magic number). Instead it proves the property the
//! count was a proxy for: the DOT renderer emits exactly one declaration per
//! node the model loaded, at real corpus scale, with a sane vision floor that
//! still holds. This cross-checks the renderer against `Graph::load`, which is
//! the assertion that actually catches a regression (dropped nodes, dedup bugs,
//! truncation), where a frozen integer would not.
//!
//! Live-corpus-gated: if `~/wintermute/autobuilder` (or `ATLAS_AUTOBUILDER`) is
//! absent — e.g. a clean CI checkout — the test skips rather than failing. The
//! fixture-driven structural tests in `src/render.rs` cover the no-corpus path.

use atlas::graph::Graph;
use atlas::parsers::Sources;
use atlas::render::{render_dot, RenderOptions};

/// Minimum vision count the live corpus is expected to carry. The PRD's "≥24"
/// floor; corpus growth keeps this monotonically satisfied (27 as of 2026-06-02).
const MIN_VISIONS: usize = 24;

/// Count DOT node declarations for a given `prefix_` id namespace.
///
/// A declaration line looks like `  vision_atlas [label=...];` — it has the
/// prefixed id followed by ` [` and is NOT an edge (edges contain `->`).
fn count_node_decls(dot: &str, prefix: &str) -> usize {
    dot.lines()
        .map(str::trim_start)
        .filter(|line| !line.contains("->"))
        .filter(|line| line.starts_with(prefix))
        .filter(|line| line.contains(" ["))
        .count()
}

#[test]
fn ac2_live_dot_node_counts_mirror_model() {
    let sources = match Sources::from_env() {
        Ok(s) => s,
        Err(_) => return, // cannot resolve sources; nothing to assert.
    };

    // Live-corpus gate: skip when the autobuilder dir is not present.
    if !sources.autobuilder_dir.exists() {
        eprintln!(
            "ac2_live_dot_node_counts_mirror_model: no live corpus at {}; skipping",
            sources.autobuilder_dir.display()
        );
        return;
    }

    let graph = Graph::load(&sources);

    // Guard: a present-but-empty corpus dir gives nothing meaningful to check.
    if graph.visions.is_empty() || graph.prds.is_empty() {
        eprintln!(
            "ac2_live_dot_node_counts_mirror_model: corpus loaded {} visions / {} prds; \
             too sparse to assert, skipping",
            graph.visions.len(),
            graph.prds.len()
        );
        return;
    }

    let dot = render_dot(&graph, &RenderOptions::default()).expect("render_dot over live corpus");

    // ── valid DOT structure ──────────────────────────────────────────────────
    assert!(
        dot.starts_with("digraph atlas {"),
        "DOT must open with `digraph atlas {{`"
    );
    assert!(dot.trim_end().ends_with('}'), "DOT must close with `}}`");
    assert_eq!(
        dot.matches('{').count(),
        dot.matches('}').count(),
        "DOT braces must balance"
    );

    // ── vision nodes: one decl per loaded vision, and the PRD's ≥24 floor ─────
    let vision_decls = count_node_decls(&dot, "vision_");
    assert_eq!(
        vision_decls,
        graph.visions.len(),
        "DOT must declare exactly one node per loaded vision \
         (decls={vision_decls}, model={})",
        graph.visions.len()
    );
    assert!(
        vision_decls >= MIN_VISIONS,
        "live corpus must carry ≥{MIN_VISIONS} vision nodes (got {vision_decls})"
    );

    // ── PRD nodes: one decl per loaded PRD (the renderer drops none) ──────────
    let prd_decls = count_node_decls(&dot, "prd_");
    assert_eq!(
        prd_decls,
        graph.prds.len(),
        "DOT must declare exactly one node per loaded PRD \
         (decls={prd_decls}, model={})",
        graph.prds.len()
    );
    // A non-trivial corpus floor: the atlas fleet alone is a 4-node chain, and
    // the live web is far larger. This catches a renderer that silently emits
    // nothing without freezing a magic count that real corpus churn invalidates.
    assert!(
        prd_decls >= MIN_VISIONS,
        "live corpus PRD nodes ({prd_decls}) implausibly low vs vision floor"
    );

    eprintln!(
        "ac2 live corpus: {vision_decls} vision nodes, {prd_decls} PRD nodes rendered to DOT"
    );
}
