//! In-memory node model for the atlas graph.
//!
//! Nodes are serde-serializable for `--format json`.
//! The graph is read-only: no node carries a mutable reference back to disk.

use serde::{Deserialize, Serialize};

/// A vision node, derived from the dream manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionNode {
    /// Slug identifier (e.g. "atlas").
    pub slug: String,
    /// Absolute path to the vision file on disk.
    pub path: String,
    /// Vision status from the dream manifest (e.g. "active", "complete").
    pub status: String,
    /// PRD slugs this vision has drafted.
    pub prds_drafted: Vec<String>,
    /// Seed description from the dream manifest.
    pub seed: String,
}

/// A PRD node, derived from frontmatter + build manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrdNode {
    /// PRD filename (e.g. "PRD-atlas-core.md").
    pub filename: String,
    /// PRD title from the H1 line.
    pub title: String,
    /// Vision slug this PRD belongs to (may be empty if not declared).
    pub vision: String,
    /// Build target declared in frontmatter (e.g. "rust-cli").
    pub build_target: String,
    /// Build-into path declared in frontmatter (may be empty).
    pub build_into: String,
    /// Derived status.
    pub status: PrdStatus,
    /// Repo URL if shipped (from build manifest).
    pub repo_url: String,
    /// Local repo path if shipped (from build manifest).
    pub repo_path: String,
}

/// Derived status for a PRD node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrdStatus {
    /// No manifest entry or no action taken.
    Drafted,
    /// Manifest entry exists but not yet shipped.
    InFlight,
    /// `output_repo_path` in manifest is non-empty and exists on disk.
    Shipped,
}

impl std::fmt::Display for PrdStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Drafted => write!(f, "drafted"),
            Self::InFlight => write!(f, "in_flight"),
            Self::Shipped => write!(f, "shipped"),
        }
    }
}

/// A repo node, derived from REPOS.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoNode {
    /// Repository name (e.g. "atlas").
    pub name: String,
    /// GitHub URL for the repo (may be empty if not parsed).
    pub url: String,
    /// One-line description from REPOS.md.
    pub description: String,
}
