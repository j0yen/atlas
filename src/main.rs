//! atlas — the autobuilder corpus as a queryable graph.
//!
//! Reads PRDs, visions, manifests, and REPOS.md from the local wintermute
//! ecosystem and exposes them as a typed in-memory graph of nodes.
//!
//! **Read-only invariant:** atlas never writes any file in the corpus.

use anyhow::Result;
use atlas::args::{FormatArg, NodeKindArg};
use atlas::graph::Graph;
use atlas::output;
use atlas::parsers::Sources;
use clap::{Parser, Subcommand};

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
    }

    Ok(())
}
