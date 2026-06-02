//! atlas — the autobuilder corpus as a queryable graph.
//!
//! Reads PRDs, visions, manifests, and REPOS.md from the local wintermute
//! ecosystem and exposes them as a typed in-memory graph of nodes.
//!
//! **Read-only invariant:** atlas never writes any file in the corpus.

use anyhow::Result;
use atlas::args::{FormatArg, NodeKindArg};
use atlas::doctor::{self, ClassFilter, FindingClass};
use atlas::graph::Graph;
use atlas::output;
use atlas::parsers::Sources;
use atlas::render::RenderOptions;
use clap::{Parser, Subcommand};
use std::io::Write as _;

/// atlas — query the autobuilder corpus as a typed node graph.
#[derive(Parser, Debug)]
#[command(
    name = "atlas",
    version,
    about = "Query the autobuilder corpus as a typed node graph",
    long_about = "Reads PRDs, visions, manifests, and REPOS.md and exposes them as a queryable graph.\n\nRead-only: atlas never writes any file in the corpus."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List all nodes, optionally filtered by kind.
    Nodes {
        /// Filter by node kind.
        #[arg(long)]
        kind: Option<NodeKindArg>,
        /// Output format.
        #[arg(long, default_value = "text")]
        format: FormatArg,
    },
    /// Show all PRDs owned by a vision slug.
    Show {
        /// Vision slug to show (e.g. "atlas").
        vision: String,
        /// Output format.
        #[arg(long, default_value = "text")]
        format: FormatArg,
    },
    /// Show dependencies for a PRD (what it waits on, what waits on it).
    Deps {
        /// PRD filename (e.g. "PRD-atlas-edges.md") or slug (e.g. "atlas-edges").
        prd: String,
        /// Output format.
        #[arg(long, default_value = "text")]
        format: FormatArg,
    },
    /// List PRDs that are blocked by at least one un-shipped dependency.
    Blocked {
        /// Output format.
        #[arg(long, default_value = "text")]
        format: FormatArg,
    },
    /// Lint the corpus for structural divergences.
    ///
    /// Reports five divergence classes: `prd_no_vision`, `vision_no_prd`,
    /// `repo_no_prd`, `shipped_repo_gone`, `fulfilled_unmarked`.
    /// Exit code reflects severity: 0=clean, 1=info-only, 2=warn.
    Doctor {
        /// Output format.
        #[arg(long, default_value = "text")]
        format: FormatArg,
        /// Restrict output to a single divergence class.
        #[arg(long, value_name = "CLASS")]
        class: Option<DoctorClassArg>,
    },
    /// Render the vision→PRD→repo graph as DOT, Mermaid, or a terminal tree.
    Graph {
        /// Output format: dot, mermaid, or tree.
        #[arg(long, default_value = "tree")]
        format: GraphFormatArg,
        /// Restrict graph to a single vision slug.
        #[arg(long, value_name = "SLUG")]
        vision: Option<String>,
        /// Only include shipped PRDs and their repos.
        #[arg(long)]
        shipped_only: bool,
    },
}

/// Divergence class filter for `atlas doctor`.
#[derive(Clone, Debug, clap::ValueEnum)]
enum DoctorClassArg {
    /// PRD has no valid Vision: reference.
    PrdNoVision,
    /// Vision has no drafted PRDs.
    VisionNoPrd,
    /// REPOS.md entry has no originating PRD.
    RepoNoPrd,
    /// Manifest-shipped PRD's repo path is gone.
    ShippedRepoGone,
    /// Active vision whose every drafted PRD is shipped.
    FulfilledUnmarked,
}

impl From<&DoctorClassArg> for FindingClass {
    fn from(arg: &DoctorClassArg) -> Self {
        match arg {
            DoctorClassArg::PrdNoVision => Self::PrdNoVision,
            DoctorClassArg::VisionNoPrd => Self::VisionNoPrd,
            DoctorClassArg::RepoNoPrd => Self::RepoNoPrd,
            DoctorClassArg::ShippedRepoGone => Self::ShippedRepoGone,
            DoctorClassArg::FulfilledUnmarked => Self::FulfilledUnmarked,
        }
    }
}

/// Graph render format.
#[derive(Clone, Debug, clap::ValueEnum)]
enum GraphFormatArg {
    /// Graphviz DOT digraph.
    Dot,
    /// Mermaid graph TD block.
    Mermaid,
    /// Terminal tree (no Graphviz required).
    Tree,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let sources = Sources::from_env()?;
    let graph = Graph::load(&sources);

    match &cli.command {
        Command::Nodes { kind, format } => {
            output::nodes(&graph, kind.as_ref(), matches!(format, FormatArg::Json))?;
        }
        Command::Show { vision, format } => {
            let json = matches!(format, FormatArg::Json);
            output::show(&graph, vision, json)?;
        }
        Command::Deps { prd, format } => {
            let json = matches!(format, FormatArg::Json);
            output::deps(&graph, prd, json)?;
        }
        Command::Blocked { format } => {
            let json = matches!(format, FormatArg::Json);
            output::blocked(&graph, json)?;
        }
        Command::Doctor { format, class } => {
            let filter = class
                .as_ref()
                .map_or(ClassFilter::All, |c| ClassFilter::Only(FindingClass::from(c)));
            let findings = doctor::run(&graph, &filter);
            let exit = doctor::exit_code(&findings);
            let json = matches!(format, FormatArg::Json);
            if json {
                doctor::render_json(&findings, &mut std::io::stdout().lock())?;
            } else {
                doctor::render_text(&findings, &mut std::io::stdout().lock())?;
            }
            if exit != 0 {
                std::process::exit(exit);
            }
        }
        Command::Graph {
            format,
            vision,
            shipped_only,
        } => {
            let opts = RenderOptions {
                vision_filter: vision.clone(),
                shipped_only: *shipped_only,
            };
            let rendered = match format {
                GraphFormatArg::Dot => atlas::render::render_dot(&graph, &opts)?,
                GraphFormatArg::Mermaid => atlas::render::render_mermaid(&graph, &opts)?,
                GraphFormatArg::Tree => atlas::render::render_tree(&graph, &opts)?,
            };
            std::io::stdout().lock().write_all(rendered.as_bytes())?;
        }
    }

    Ok(())
}
