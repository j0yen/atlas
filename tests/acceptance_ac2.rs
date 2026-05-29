//! AC2: atlas nodes --kind vision lists vision slugs excluding _no_fleet_passes sentinel.

use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn write_dream_manifest(dir: &TempDir, visions: &[(&str, &str)]) -> std::path::PathBuf {
    let mut entries = serde_json::Map::new();
    for (slug, status) in visions {
        entries.insert(
            slug.to_string(),
            serde_json::json!({
                "path": format!("visions/{}.md", slug),
                "status": status,
                "prds_drafted": [],
                "seed": "test seed"
            }),
        );
    }
    // Include the sentinel that should be excluded.
    entries.insert(
        "_no_fleet_passes".to_string(),
        serde_json::json!({
            "status": "sentinel",
            "prds_drafted": [],
            "seed": ""
        }),
    );
    let manifest = serde_json::json!({ "visions": entries });
    let path = dir.path().join("dream-manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

fn write_build_manifest(dir: &TempDir) -> std::path::PathBuf {
    let manifest = serde_json::json!({ "prds": [] });
    let path = dir.path().join("build-manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path
}

fn write_repos_md(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path().join("REPOS.md");
    fs::write(&path, "# Repos\n").unwrap();
    path
}

fn write_autobuilder_dir(dir: &TempDir) -> std::path::PathBuf {
    let ab = dir.path().join("autobuilder");
    fs::create_dir_all(&ab).unwrap();
    ab
}

#[test]
fn acceptance_ac2_vision_nodes_list_excludes_sentinel() {
    let tmp = TempDir::new().unwrap();
    let dream = write_dream_manifest(
        &tmp,
        &[
            ("atlas", "active"),
            ("recall", "active"),
            ("docket", "active"),
        ],
    );
    let build = write_build_manifest(&tmp);
    let repos = write_repos_md(&tmp);
    let ab = write_autobuilder_dir(&tmp);

    // Use the library parsers directly.
    let entries = atlas_parsers::parse_dream_manifest(&dream).unwrap();
    // Sentinel must be excluded.
    assert!(
        !entries.contains_key("_no_fleet_passes"),
        "_no_fleet_passes sentinel must be excluded"
    );
    // Real visions must be present.
    assert!(entries.contains_key("atlas"), "atlas vision missing");
    assert!(entries.contains_key("recall"), "recall vision missing");
    assert!(entries.contains_key("docket"), "docket vision missing");
    assert_eq!(entries.len(), 3, "expected exactly 3 real visions");

    // Convert to nodes and verify.
    let nodes = atlas_parsers::dream_entries_to_vision_nodes(entries, &ab);
    assert_eq!(nodes.len(), 3);
    for node in &nodes {
        assert_ne!(node.slug, "_no_fleet_passes");
    }

    // Suppress unused-variable warnings for paths only used for env setup.
    let _ = build;
    let _ = repos;
}

// Re-export parsers for test use.
mod atlas_parsers {
    pub use atlas::parsers::*;
}
