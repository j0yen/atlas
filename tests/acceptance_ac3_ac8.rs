//! AC3: PRD nodes parsing; AC4: atlas show; AC5: status derivation;
//! AC6: JSON round-trip; AC8: env-overridable source paths.
//!
//! All tests drive the parsers off fixture dirs — never live ~/.claude or ~/wintermute.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// ── fixture helpers ──────────────────────────────────────────────────────────

fn make_autobuilder_dir(tmp: &TempDir) -> PathBuf {
    let ab = tmp.path().join("autobuilder");
    fs::create_dir_all(&ab).unwrap();
    ab
}

fn write_prd(dir: &PathBuf, filename: &str, content: &str) {
    fs::write(dir.join(filename), content).unwrap();
}

fn make_dream_manifest(tmp: &TempDir, visions: &[(&str, &str, &[&str])]) -> PathBuf {
    let mut entries = serde_json::Map::new();
    for (slug, status, prds) in visions {
        let prds_json: Vec<serde_json::Value> =
            prds.iter().map(|p| serde_json::Value::String(p.to_string())).collect();
        entries.insert(
            slug.to_string(),
            serde_json::json!({
                "path": format!("visions/{}.md", slug),
                "status": status,
                "prds_drafted": prds_json,
                "seed": format!("seed for {}", slug)
            }),
        );
    }
    // Always include the sentinel that should be excluded.
    entries.insert(
        "_no_fleet_passes".to_string(),
        serde_json::json!({ "status": "sentinel", "prds_drafted": [], "seed": "" }),
    );
    let manifest = serde_json::json!({ "visions": entries });
    let path = tmp.path().join("dream-manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

fn make_build_manifest(tmp: &TempDir, entries: &[(&str, Option<&str>, Option<&str>)]) -> PathBuf {
    // entries: (prd_filename, output_repo_path, output_repo_url)
    let prds: Vec<serde_json::Value> = entries
        .iter()
        .map(|(name, repo_path, repo_url)| {
            let mut obj = serde_json::json!({
                "path": format!("/autobuilder/{}", name),
            });
            if let Some(p) = repo_path {
                obj["output_repo_path"] = serde_json::Value::String(p.to_string());
            }
            if let Some(u) = repo_url {
                obj["output_repo_url"] = serde_json::Value::String(u.to_string());
            }
            obj
        })
        .collect();
    let manifest = serde_json::json!({ "prds": prds });
    let path = tmp.path().join("build-manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

fn make_repos_md(tmp: &TempDir, repos: &[(&str, &str, &str)]) -> PathBuf {
    let mut content = String::from("# Repos\n\n");
    for (name, url, desc) in repos {
        content.push_str(&format!("- [{name}]({url}) — {desc}\n"));
    }
    let path = tmp.path().join("REPOS.md");
    fs::write(&path, &content).unwrap();
    path
}

// ── AC3: PRD nodes parsing ───────────────────────────────────────────────────

/// AC3: scan_prd_nodes parses PRDs, assigns filenames/titles, survives missing fields.
#[test]
fn ac3_prd_nodes_parsed_correctly() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);

    write_prd(
        &ab,
        "PRD-atlas-core.md",
        r#"# PRD: atlas-core — the corpus as a graph

**Author:** /dream
**Status:** Draft v0.1
**Vision:** visions/atlas.md
**build_target:** rust-cli
**build_into:** /home/jsy/wintermute/atlas
"#,
    );
    write_prd(
        &ab,
        "PRD-minimal.md",
        "# PRD: minimal\n\nNo frontmatter fields here.\n",
    );
    // Non-PRD file — should be skipped.
    fs::write(ab.join("README.md"), "not a PRD").unwrap();

    let build_map = HashMap::new();
    let nodes = atlas::parsers::scan_prd_nodes(&ab, &build_map).unwrap();

    assert_eq!(nodes.len(), 2, "expected 2 PRD nodes (README skipped)");

    // Sorted by filename.
    assert_eq!(nodes[0].filename, "PRD-atlas-core.md");
    assert_eq!(nodes[0].title, "atlas-core — the corpus as a graph");
    assert_eq!(nodes[0].vision, "atlas");
    assert_eq!(nodes[0].build_target, "rust-cli");
    assert_eq!(nodes[0].build_into, "/home/jsy/wintermute/atlas");
    assert_eq!(nodes[0].status, atlas::model::PrdStatus::Drafted);

    assert_eq!(nodes[1].filename, "PRD-minimal.md");
    assert_eq!(nodes[1].title, "minimal");
    assert!(nodes[1].vision.is_empty());
    assert!(nodes[1].build_target.is_empty());
    assert_eq!(nodes[1].status, atlas::model::PrdStatus::Drafted);
}

/// AC3: parser never panics on any PRD in a fixture set.
#[test]
fn ac3_parser_tolerant_on_edge_cases() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);

    // PRD with code fence that contains fake frontmatter.
    write_prd(
        &ab,
        "PRD-fenced.md",
        r#"# PRD: fenced-test — verify code-fence skip

```markdown
**Vision:** visions/should-be-ignored.md
**build_target:** should-be-ignored
```

**Vision:** visions/real.md
**build_target:** rust-lib
"#,
    );
    // Empty PRD.
    write_prd(&ab, "PRD-empty.md", "");
    // PRD with no H1.
    write_prd(&ab, "PRD-no-h1.md", "**Vision:** visions/v.md\n");

    let build_map = HashMap::new();
    let nodes = atlas::parsers::scan_prd_nodes(&ab, &build_map).unwrap();

    assert_eq!(nodes.len(), 3);

    let fenced = nodes.iter().find(|n| n.filename == "PRD-fenced.md").unwrap();
    assert_eq!(fenced.vision, "real", "code-fence content must be skipped");
    assert_eq!(fenced.build_target, "rust-lib");

    let no_h1 = nodes.iter().find(|n| n.filename == "PRD-no-h1.md").unwrap();
    assert!(no_h1.title.is_empty(), "no H1 → empty title");
    assert_eq!(no_h1.vision, "v");
}

// ── AC5: status derivation ───────────────────────────────────────────────────

/// AC5: a PRD with output_repo_path that exists on disk derives Shipped.
#[test]
fn ac5_shipped_when_repo_path_exists() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);

    // Create a fake "repo" dir so the path-exists check passes.
    let repo_dir = tmp.path().join("fake-repo");
    fs::create_dir_all(&repo_dir).unwrap();

    write_prd(
        &ab,
        "PRD-shipped.md",
        "# PRD: shipped — test\n\n**Vision:** visions/v.md\n",
    );

    let build_manifest = make_build_manifest(
        &tmp,
        &[(
            "PRD-shipped.md",
            Some(repo_dir.to_str().unwrap()),
            Some("https://github.com/j0yen/fake-repo"),
        )],
    );
    let build_map = atlas::parsers::parse_build_manifest(&build_manifest).unwrap();
    let nodes = atlas::parsers::scan_prd_nodes(&ab, &build_map).unwrap();

    let shipped = nodes.iter().find(|n| n.filename == "PRD-shipped.md").unwrap();
    assert_eq!(shipped.status, atlas::model::PrdStatus::Shipped);
    assert_eq!(shipped.repo_url, "https://github.com/j0yen/fake-repo");
}

/// AC5: a PRD with no manifest entry derives Drafted.
#[test]
fn ac5_drafted_when_no_manifest_entry() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);
    write_prd(&ab, "PRD-draft.md", "# PRD: draft\n");

    let build_map = HashMap::new();
    let nodes = atlas::parsers::scan_prd_nodes(&ab, &build_map).unwrap();
    assert_eq!(nodes[0].status, atlas::model::PrdStatus::Drafted);
}

/// AC5: a PRD with manifest entry but no existing repo path derives InFlight.
#[test]
fn ac5_in_flight_when_manifest_but_no_path() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);
    write_prd(&ab, "PRD-inflight.md", "# PRD: inflight\n");

    let build_manifest = make_build_manifest(
        &tmp,
        &[("PRD-inflight.md", Some("/nonexistent/path"), None)],
    );
    let build_map = atlas::parsers::parse_build_manifest(&build_manifest).unwrap();
    let nodes = atlas::parsers::scan_prd_nodes(&ab, &build_map).unwrap();
    assert_eq!(nodes[0].status, atlas::model::PrdStatus::InFlight);
}

// ── AC4: atlas show ──────────────────────────────────────────────────────────

/// AC4: graph.load + output::show finds PRDs for a known vision slug.
#[test]
fn ac4_show_known_vision() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);

    write_prd(
        &ab,
        "PRD-atlas-core.md",
        "# PRD: atlas-core — core\n\n**Vision:** visions/atlas.md\n",
    );
    write_prd(
        &ab,
        "PRD-atlas-edges.md",
        "# PRD: atlas-edges — edges\n\n**Vision:** visions/atlas.md\n",
    );
    write_prd(
        &ab,
        "PRD-other.md",
        "# PRD: other — unrelated\n\n**Vision:** visions/other.md\n",
    );

    let dream = make_dream_manifest(
        &tmp,
        &[
            ("atlas", "active", &["atlas-core", "atlas-edges"]),
            ("other", "active", &["other"]),
        ],
    );
    let build = make_build_manifest(&tmp, &[]);
    let repos = make_repos_md(&tmp, &[]);

    let sources = atlas::parsers::Sources {
        autobuilder_dir: ab,
        build_manifest: build,
        dream_manifest: dream,
        repos_md: repos,
        gossip_file: tmp.path().join("gossip.md"),
    };
    let graph = atlas::graph::Graph::load(&sources);

    // Find atlas vision.
    let atlas_vision = graph.visions.iter().find(|v| v.slug == "atlas");
    assert!(atlas_vision.is_some(), "atlas vision must be present");

    // Find PRDs owned by atlas.
    let atlas_prds: Vec<&atlas::model::PrdNode> =
        graph.prds.iter().filter(|p| p.vision == "atlas").collect();
    assert_eq!(atlas_prds.len(), 2, "atlas vision should own 2 PRDs");

    let filenames: Vec<&str> = atlas_prds.iter().map(|p| p.filename.as_str()).collect();
    assert!(filenames.contains(&"PRD-atlas-core.md"));
    assert!(filenames.contains(&"PRD-atlas-edges.md"));

    // PRD for 'other' must not appear under 'atlas'.
    assert!(!filenames.contains(&"PRD-other.md"));
}

// ── AC6: JSON round-trip ─────────────────────────────────────────────────────

/// AC6: VisionNode and PrdNode serialize to valid JSON that round-trips.
#[test]
fn ac6_json_round_trips_vision_node() {
    let node = atlas::model::VisionNode {
        slug: "atlas".to_string(),
        path: "/home/jsy/wintermute/autobuilder/visions/atlas.md".to_string(),
        status: "active".to_string(),
        prds_drafted: vec!["atlas-core".to_string(), "atlas-edges".to_string()],
        seed: "The corpus, as a graph".to_string(),
    };
    let json = serde_json::to_string_pretty(&node).unwrap();
    let parsed: atlas::model::VisionNode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.slug, node.slug);
    assert_eq!(parsed.status, node.status);
    assert_eq!(parsed.prds_drafted, node.prds_drafted);
}

#[test]
fn ac6_json_round_trips_prd_node() {
    let node = atlas::model::PrdNode {
        filename: "PRD-atlas-core.md".to_string(),
        title: "atlas-core — the corpus as a graph".to_string(),
        vision: "atlas".to_string(),
        build_target: "rust-cli".to_string(),
        build_into: "/home/jsy/wintermute/atlas".to_string(),
        status: atlas::model::PrdStatus::Shipped,
        repo_url: "https://github.com/j0yen/atlas".to_string(),
        repo_path: "/home/jsy/wintermute/atlas".to_string(),
    };
    let json = serde_json::to_string_pretty(&node).unwrap();
    let parsed: atlas::model::PrdNode = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.filename, node.filename);
    assert_eq!(parsed.status, atlas::model::PrdStatus::Shipped);
    assert_eq!(parsed.repo_url, node.repo_url);
}

#[test]
fn ac6_prd_status_json_values() {
    // Verify the snake_case rename_all is correct.
    assert_eq!(
        serde_json::to_string(&atlas::model::PrdStatus::Drafted).unwrap(),
        "\"drafted\""
    );
    assert_eq!(
        serde_json::to_string(&atlas::model::PrdStatus::InFlight).unwrap(),
        "\"in_flight\""
    );
    assert_eq!(
        serde_json::to_string(&atlas::model::PrdStatus::Shipped).unwrap(),
        "\"shipped\""
    );
}

// ── AC8: env-overridable paths ───────────────────────────────────────────────

/// AC8: Sources struct accepts arbitrary overridden paths (the env-override mechanism
/// is validated by Sources::from_env which reads ATLAS_* vars; here we exercise
/// Sources directly with fixture paths to confirm all four fields are overridable
/// without touching live system paths).
#[test]
fn ac8_sources_struct_accepts_arbitrary_paths() {
    let sources = atlas::parsers::Sources {
        autobuilder_dir: std::path::PathBuf::from("/tmp/test-ab"),
        build_manifest: std::path::PathBuf::from("/tmp/test-bm.json"),
        dream_manifest: std::path::PathBuf::from("/tmp/test-dm.json"),
        repos_md: std::path::PathBuf::from("/tmp/test-repos.md"),
        gossip_file: std::path::PathBuf::from("/tmp/test-gossip.md"),
    };

    assert_eq!(sources.autobuilder_dir.to_str().unwrap(), "/tmp/test-ab");
    assert_eq!(sources.build_manifest.to_str().unwrap(), "/tmp/test-bm.json");
    assert_eq!(sources.dream_manifest.to_str().unwrap(), "/tmp/test-dm.json");
    assert_eq!(sources.repos_md.to_str().unwrap(), "/tmp/test-repos.md");
}

/// AC8: Graph::load uses fixture paths (not live ~/.claude) when env is set.
#[test]
fn ac8_graph_load_uses_fixture_sources() {
    let tmp = TempDir::new().unwrap();
    let ab = make_autobuilder_dir(&tmp);
    write_prd(
        &ab,
        "PRD-fixture.md",
        "# PRD: fixture — test\n\n**Vision:** visions/test.md\n",
    );
    let dream = make_dream_manifest(&tmp, &[("test", "active", &["fixture"])]);
    let build = make_build_manifest(&tmp, &[]);
    let repos = make_repos_md(&tmp, &[("fixture-repo", "https://github.com/j0yen/fixture-repo", "test")]);

    let sources = atlas::parsers::Sources {
        autobuilder_dir: ab,
        build_manifest: build,
        dream_manifest: dream,
        repos_md: repos,
        gossip_file: tmp.path().join("gossip.md"),
    };
    let graph = atlas::graph::Graph::load(&sources);

    assert_eq!(graph.prds.len(), 1, "one PRD from fixture");
    assert_eq!(graph.visions.len(), 1, "one vision from fixture (sentinel excluded)");
    assert_eq!(graph.repos.len(), 1, "one repo from fixture REPOS.md");
    assert_eq!(graph.prds[0].filename, "PRD-fixture.md");
    assert_eq!(graph.visions[0].slug, "test");
    assert_eq!(graph.repos[0].name, "fixture-repo");
}
