//! Shared argument types for CLI commands.
//!
//! These types are defined in the library so both `output` and `main` can use them.

/// Node kind filter for `atlas nodes`.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum NodeKindArg {
    /// Vision nodes from the dream manifest.
    Vision,
    /// PRD nodes from the autobuilder directory.
    Prd,
    /// Repo nodes from REPOS.md.
    Repo,
}

/// Output format.
#[derive(Clone, Debug, clap::ValueEnum)]
pub enum FormatArg {
    /// Human-readable text (one node per line).
    Text,
    /// Machine-readable JSON.
    Json,
}
