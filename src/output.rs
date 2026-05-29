//! Output formatting for atlas commands.

use anyhow::Result;
use std::io::{self, Write};

use crate::args::NodeKindArg;
use crate::graph::Graph;
use crate::model::{PrdNode, RepoNode, VisionNode};

/// Output for `atlas nodes [--kind ...] [--format ...]`.
pub fn nodes(graph: &Graph, kind: Option<&NodeKindArg>, json: bool) -> Result<()> {
    if json {
        nodes_json(graph, kind)
    } else {
        nodes_text(graph, kind)
    }
}

fn nodes_text(graph: &Graph, kind: Option<&NodeKindArg>) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let emit_visions = matches!(kind, None | Some(NodeKindArg::Vision));
    let emit_prds = matches!(kind, None | Some(NodeKindArg::Prd));
    let emit_repos = matches!(kind, None | Some(NodeKindArg::Repo));

    if emit_visions {
        for v in &graph.visions {
            writeln!(out, "vision  {slug}  [{status}]  prds_drafted={n}",
                slug = v.slug,
                status = v.status,
                n = v.prds_drafted.len()
            )?;
        }
    }
    if emit_prds {
        for p in &graph.prds {
            let repo = if p.repo_url.is_empty() {
                String::new()
            } else {
                format!("  repo={}", p.repo_url)
            };
            writeln!(
                out,
                "prd     {filename}  [{status}]{repo}",
                filename = p.filename,
                status = p.status,
            )?;
        }
    }
    if emit_repos {
        for r in &graph.repos {
            writeln!(out, "repo    {name}  {url}", name = r.name, url = r.url)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct NodesJsonOutput<'a> {
    visions: &'a [VisionNode],
    prds: &'a [PrdNode],
    repos: &'a [RepoNode],
}

fn nodes_json(graph: &Graph, kind: Option<&NodeKindArg>) -> Result<()> {
    let empty_visions: Vec<VisionNode> = Vec::new();
    let empty_prds: Vec<PrdNode> = Vec::new();
    let empty_repos: Vec<RepoNode> = Vec::new();

    let output = NodesJsonOutput {
        visions: match kind {
            None | Some(NodeKindArg::Vision) => &graph.visions,
            _ => &empty_visions,
        },
        prds: match kind {
            None | Some(NodeKindArg::Prd) => &graph.prds,
            _ => &empty_prds,
        },
        repos: match kind {
            None | Some(NodeKindArg::Repo) => &graph.repos,
            _ => &empty_repos,
        },
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Output for `atlas show <vision-slug> [--format ...]`.
///
/// Exits with code 2 if the vision slug is unknown.
pub fn show(graph: &Graph, vision_slug: &str, json: bool) -> Result<()> {
    // Find the vision.
    let vision = graph.visions.iter().find(|v| v.slug == vision_slug);
    if vision.is_none() {
        // Write error to stderr and exit 2.
        eprintln!("atlas: unknown vision slug '{vision_slug}'");
        eprintln!("Run 'atlas nodes --kind vision' to see available slugs.");
        std::process::exit(2);
    }
    let vision = vision.expect("checked above");

    // Find PRDs owned by this vision.
    let owned_prds: Vec<&PrdNode> = graph
        .prds
        .iter()
        .filter(|p| p.vision == vision_slug)
        .collect();

    if json {
        show_json(vision, &owned_prds)
    } else {
        show_text(vision, &owned_prds)
    }
}

fn show_text(vision: &VisionNode, prds: &[&PrdNode]) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(out, "vision: {slug}  [{status}]", slug = vision.slug, status = vision.status)?;
    if !vision.seed.is_empty() {
        writeln!(out, "  seed: {}", vision.seed)?;
    }
    writeln!(out, "  prds ({n}):", n = prds.len())?;
    for p in prds {
        let repo = if p.repo_url.is_empty() {
            String::new()
        } else {
            format!("  → {}", p.repo_url)
        };
        writeln!(
            out,
            "    {filename}  [{status}]{repo}",
            filename = p.filename,
            status = p.status,
        )?;
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct ShowJsonOutput<'a> {
    vision: &'a VisionNode,
    prds: Vec<&'a PrdNode>,
}

fn show_json(vision: &VisionNode, prds: &[&PrdNode]) -> Result<()> {
    let output = ShowJsonOutput {
        vision,
        prds: prds.to_vec(),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
