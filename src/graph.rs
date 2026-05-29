//! In-memory graph loaded from the corpus sources.

use std::collections::HashSet;

use crate::edges::{load_edges, EdgeIndex, EdgeSet};
use crate::model::{PrdNode, RepoNode, VisionNode};
use crate::parsers::{
    dream_entries_to_vision_nodes, parse_build_manifest, parse_dream_manifest, parse_repos_md,
    scan_prd_nodes, Sources,
};

/// The loaded graph: all typed nodes and edges in memory.
#[derive(Debug)]
pub struct Graph {
    /// Vision nodes from the dream manifest.
    pub visions: Vec<VisionNode>,
    /// PRD nodes from the autobuilder directory + build manifest.
    pub prds: Vec<PrdNode>,
    /// Repo nodes from REPOS.md.
    pub repos: Vec<RepoNode>,
    /// Dependency edges (frontmatter + gossip, deduplicated).
    pub edge_set: EdgeSet,
    /// Fast-lookup index for edges.
    pub edge_index: EdgeIndex,
}

impl Graph {
    /// Load the graph from the given source paths.
    ///
    /// Missing or unreadable source files produce partial results rather than
    /// hard errors, so `atlas` remains usable even when some sources are absent.
    #[must_use]
    pub fn load(sources: &Sources) -> Self {
        // Load build manifest (needed for PRD status derivation).
        let build_map = if sources.build_manifest.exists() {
            parse_build_manifest(&sources.build_manifest).unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        // Load dream manifest → vision nodes.
        let visions = if sources.dream_manifest.exists() {
            let entries = parse_dream_manifest(&sources.dream_manifest).unwrap_or_default();
            dream_entries_to_vision_nodes(entries, &sources.autobuilder_dir)
        } else {
            Vec::new()
        };

        // Scan PRD files → PRD nodes.
        let prds = if sources.autobuilder_dir.exists() {
            scan_prd_nodes(&sources.autobuilder_dir, &build_map).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Load REPOS.md → repo nodes.
        let repos = if sources.repos_md.exists() {
            parse_repos_md(&sources.repos_md).unwrap_or_default()
        } else {
            Vec::new()
        };

        // Build known-filenames set for edge resolution.
        let known_filenames: HashSet<String> =
            prds.iter().map(|p| p.filename.clone()).collect();

        // Load edges (tolerant: partial results on error).
        let gossip_path = if sources.gossip_file.exists() {
            Some(sources.gossip_file.as_path())
        } else {
            None
        };
        let edge_set = if sources.autobuilder_dir.exists() {
            load_edges(&sources.autobuilder_dir, gossip_path, &known_filenames)
                .unwrap_or_default()
        } else {
            EdgeSet::default()
        };
        let edge_index = EdgeIndex::build(&edge_set);

        Self {
            visions,
            prds,
            repos,
            edge_set,
            edge_index,
        }
    }
}
