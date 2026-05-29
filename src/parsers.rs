//! Source parsers for the atlas graph.
//!
//! Each parser is isolated and tolerant: it never panics on malformed input.
//! Tests drive parsers off fixture directories, never live `~/.claude` or
//! `~/wintermute`.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{PrdNode, PrdStatus, RepoNode, VisionNode};

/// Resolved source paths for all corpus inputs.
#[derive(Debug, Clone)]
pub struct Sources {
    /// Directory containing PRD-*.md files.
    pub autobuilder_dir: PathBuf,
    /// Path to the build manifest JSON.
    pub build_manifest: PathBuf,
    /// Path to the dream manifest JSON.
    pub dream_manifest: PathBuf,
    /// Path to REPOS.md.
    pub repos_md: PathBuf,
}

impl Sources {
    /// Resolve sources from environment variables, falling back to defaults.
    ///
    /// Override any path via:
    /// - `ATLAS_AUTOBUILDER`
    /// - `ATLAS_BUILD_MANIFEST`
    /// - `ATLAS_DREAM_MANIFEST`
    /// - `ATLAS_REPOS`
    ///
    /// # Errors
    ///
    /// Returns an error if the `HOME` environment variable is not set.
    pub fn from_env() -> Result<Self> {
        let home = dirs_home()?;
        Ok(Self {
            autobuilder_dir: env_path_or("ATLAS_AUTOBUILDER", home.join("wintermute/autobuilder")),
            build_manifest: env_path_or(
                "ATLAS_BUILD_MANIFEST",
                home.join(".claude/skills/build/state/manifest.json"),
            ),
            dream_manifest: env_path_or(
                "ATLAS_DREAM_MANIFEST",
                home.join(".claude/skills/dream/state/manifest.json"),
            ),
            repos_md: env_path_or("ATLAS_REPOS", home.join("wintermute/REPOS.md")),
        })
    }
}

fn dirs_home() -> Result<PathBuf> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable not set")
}

fn env_path_or(var: &str, default: PathBuf) -> PathBuf {
    std::env::var(var).map(PathBuf::from).unwrap_or(default)
}

// ── PRD frontmatter parser ──────────────────────────────────────────────────

/// Frontmatter fields extracted from a PRD file.
#[derive(Debug, Default)]
pub struct PrdFrontmatter {
    /// PRD title from the H1 line (e.g. "atlas-core — the corpus, as a graph").
    pub title: String,
    /// Vision slug this PRD belongs to (may be empty if not declared).
    pub vision: String,
    /// Build target declared in frontmatter (e.g. "rust-cli").
    pub build_target: String,
    /// Build-into path declared in frontmatter (may be empty).
    pub build_into: String,
}

/// Parse PRD frontmatter from the file content.
///
/// Uses a tolerant line-scan: missing fields result in empty strings, never panics.
/// Lines inside fenced code blocks (```...```) are skipped.
#[must_use]
pub fn parse_prd_frontmatter(content: &str) -> PrdFrontmatter {
    let mut fm = PrdFrontmatter::default();
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Toggle code-block state.
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        // Extract H1 title: `# PRD: <slug> — <title>`
        if fm.title.is_empty() && trimmed.starts_with("# PRD:") {
            fm.title = trimmed
                .trim_start_matches("# PRD:")
                .trim()
                .to_string();
        }

        // Extract bold frontmatter fields.
        if let Some(val) = extract_bold_field(trimmed, "Vision:") {
            if fm.vision.is_empty() {
                fm.vision = strip_vision_path(val);
            }
        } else if let Some(val) = extract_bold_field(trimmed, "build_target:") {
            if fm.build_target.is_empty() {
                fm.build_target = val.to_string();
            }
        } else if let Some(val) = extract_bold_field(trimmed, "build_into:") {
            if fm.build_into.is_empty() {
                fm.build_into = val.to_string();
            }
        }
    }

    fm
}

/// Extract the value from a `**Key:** value` line.
/// Strips trailing inline comments (` # ...`).
fn extract_bold_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    // Match `**Key:** value` pattern.
    let prefix = format!("**{key}**");
    let line = line.strip_prefix(&prefix)?.trim_start_matches(':').trim();
    // Strip trailing inline comment.
    let value = line.find(" # ").map_or(line, |pos| &line[..pos]);
    Some(value.trim())
}

/// Convert `visions/atlas.md` → `atlas` (strip dir prefix and .md suffix).
fn strip_vision_path(raw: &str) -> String {
    let base = raw.trim_start_matches("visions/");
    base.strip_suffix(".md").unwrap_or(base).to_string()
}

// ── Build manifest parser ───────────────────────────────────────────────────

/// Minimal shape of a build-manifest PRD entry.
#[derive(Debug, Deserialize)]
pub struct BuildManifestEntry {
    /// PRD file path from the manifest.
    pub path: Option<String>,
    /// Local output repo path if shipped.
    pub output_repo_path: Option<String>,
    /// GitHub repo URL if shipped.
    pub output_repo_url: Option<String>,
    /// Build status string (e.g. `"shipped"`, `"in_progress"`).
    pub status: Option<String>,
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct BuildManifest {
    prds: Vec<BuildManifestEntry>,
}

/// Parse the build manifest JSON.
///
/// Returns a map from PRD basename (e.g. "PRD-atlas-core.md") to entry.
/// Missing or malformed entries are silently dropped (tolerant).
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed as JSON.
pub fn parse_build_manifest(path: &Path) -> Result<HashMap<String, BuildManifestEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading build manifest: {}", path.display()))?;
    let manifest: BuildManifest = serde_json::from_str(&content)
        .with_context(|| format!("parsing build manifest: {}", path.display()))?;

    let mut map = HashMap::new();
    for entry in manifest.prds {
        if let Some(ref p) = entry.path {
            let basename = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p.as_str())
                .to_string();
            map.insert(basename, entry);
        }
    }
    Ok(map)
}

// ── Dream manifest parser ────────────────────────────────────────────────────

/// Minimal shape of a dream-manifest vision entry.
#[derive(Debug, Deserialize)]
pub struct DreamVisionEntry {
    /// Path to the vision file on disk.
    pub path: Option<String>,
    /// Vision status string (e.g. "active", "complete").
    pub status: Option<String>,
    /// PRD slugs drafted by this vision.
    pub prds_drafted: Option<Vec<String>>,
    /// One-line seed description.
    pub seed: Option<String>,
}

/// Raw dream manifest: visions is a heterogeneous map — most values are objects
/// (`DreamVisionEntry`) but some keys (e.g. `_no_fleet_passes`) hold arrays.
/// We parse as `serde_json::Value` first, then filter + coerce.
#[derive(Debug, Deserialize)]
struct DreamManifestRaw {
    visions: HashMap<String, serde_json::Value>,
}

/// Parse the dream manifest JSON.
///
/// Excludes the `_no_fleet_passes` sentinel key and any value that is not a
/// JSON object (the sentinel is an array and would panic a typed deserialize).
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed as JSON.
pub fn parse_dream_manifest(path: &Path) -> Result<HashMap<String, DreamVisionEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading dream manifest: {}", path.display()))?;
    let raw: DreamManifestRaw = serde_json::from_str(&content)
        .with_context(|| format!("parsing dream manifest: {}", path.display()))?;

    let mut map = HashMap::new();
    for (k, v) in raw.visions {
        // Skip the sentinel and any non-object entries.
        if k == "_no_fleet_passes" || !v.is_object() {
            continue;
        }
        // Tolerantly deserialize: skip entries that don't match the shape.
        if let Ok(entry) = serde_json::from_value::<DreamVisionEntry>(v) {
            map.insert(k, entry);
        }
    }
    Ok(map)
}

// ── REPOS.md parser ──────────────────────────────────────────────────────────

/// Parse REPOS.md into a list of `RepoNode`.
///
/// Looks for markdown list items containing a hyperlink: `- [name](url) — desc`
/// or `- **[name](url)** — desc`. Tolerates arbitrary formatting around entries.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn parse_repos_md(path: &Path) -> Result<Vec<RepoNode>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading REPOS.md: {}", path.display()))?;

    let mut repos = Vec::new();
    for line in content.lines() {
        if let Some(repo) = parse_repos_md_line(line) {
            repos.push(repo);
        }
    }
    Ok(repos)
}

/// Try to parse a single REPOS.md line into a `RepoNode`.
///
/// Handles two formats:
/// - List items: `- [name](url) — description`
/// - Table rows:  `| [name](url) | binary | description |`
///   (skips header/divider rows that have no `](` link pattern)
fn parse_repos_md_line(line: &str) -> Option<RepoNode> {
    let trimmed = line.trim();

    // Must be a list item OR a table row (starts with `|`).
    let is_list = trimmed.starts_with("- ") || trimmed.starts_with("* ");
    let is_table = trimmed.starts_with('|');
    if !is_list && !is_table {
        return None;
    }

    // Find `[name](url)` pattern anywhere on the line.
    let link_start = trimmed.find('[')? ;
    let link_end = trimmed[link_start..].find(']')? + link_start;
    // Must be followed by `(` for a real link (not just `[text]` without URL).
    let url_start = trimmed[link_end..].find('(')? + link_end;
    let url_end = trimmed[url_start..].find(')')? + url_start;

    // Validate URL looks like a github link (skip divider rows like `|---|---|`).
    let url = &trimmed[url_start + 1..url_end];
    if url.is_empty() || url.starts_with('-') {
        return None;
    }

    let name = trimmed[link_start + 1..link_end].to_string();
    let url = url.to_string();

    // Extract description:
    // - List: rest of line after `)`.
    // - Table: last non-empty `|`-delimited cell after splitting.
    let description = if is_table {
        // Split by `|`, collect non-empty trimmed cells, take the last one.
        let cells: Vec<&str> = trimmed
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        // The last cell is the description; first is `[name](url)`, second is binary.
        cells.last().copied().unwrap_or("").to_string()
    } else {
        let rest = trimmed[url_end + 1..].trim();
        rest.trim_start_matches(" —")
            .trim_start_matches('—')
            .trim_start_matches(" -")
            .trim_start_matches('-')
            .trim()
            .to_string()
    };

    Some(RepoNode {
        name,
        url,
        description,
    })
}

// ── PRD directory scanner ────────────────────────────────────────────────────

/// Scan the autobuilder directory for PRD files and parse each into a `PrdNode`.
///
/// Joins with the build-manifest map to derive status and repo info.
/// Never panics on malformed PRD content.
///
/// # Errors
///
/// Returns an error if the autobuilder directory cannot be read.
pub fn scan_prd_nodes<S: std::hash::BuildHasher>(
    autobuilder_dir: &Path,
    build_map: &HashMap<String, BuildManifestEntry, S>,
) -> Result<Vec<PrdNode>> {
    let mut nodes = Vec::new();

    let entries = std::fs::read_dir(autobuilder_dir)
        .with_context(|| format!("reading autobuilder dir: {}", autobuilder_dir.display()))?;

    for entry in entries {
        let entry = entry.context("reading directory entry")?;
        let fname = entry.file_name();
        let filename = fname.to_string_lossy().to_string();

        if !filename.starts_with("PRD-")
            || !std::path::Path::new(&filename)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        // Skip archived PRDs if they somehow appear.
        if entry
            .path()
            .parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "PRDs-archive")
        {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue; // skip unreadable files
        };

        let fm = parse_prd_frontmatter(&content);

        // Derive status from build manifest.
        let (status, repo_url, repo_path) = derive_prd_status(&filename, build_map);

        nodes.push(PrdNode {
            filename,
            title: fm.title,
            vision: fm.vision,
            build_target: fm.build_target,
            build_into: fm.build_into,
            status,
            repo_url,
            repo_path,
        });
    }

    nodes.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(nodes)
}

/// Derive PRD status from the build-manifest map.
fn derive_prd_status<S: std::hash::BuildHasher>(
    filename: &str,
    build_map: &HashMap<String, BuildManifestEntry, S>,
) -> (PrdStatus, String, String) {
    let Some(entry) = build_map.get(filename) else {
        return (PrdStatus::Drafted, String::new(), String::new());
    };

    let repo_path = entry.output_repo_path.clone().unwrap_or_default();
    let repo_url = entry.output_repo_url.clone().unwrap_or_default();

    // shipped: output_repo_path is non-empty AND the path exists on disk.
    if !repo_path.is_empty() && Path::new(&repo_path).exists() {
        return (PrdStatus::Shipped, repo_url, repo_path);
    }

    // Also treat manifest status="shipped" as shipped (for archived PRDs
    // where the path may have moved).
    if entry.status.as_deref() == Some("shipped") {
        return (PrdStatus::Shipped, repo_url, repo_path);
    }

    (PrdStatus::InFlight, repo_url, repo_path)
}

/// Convert dream manifest entries to `VisionNode`s.
#[must_use]
pub fn dream_entries_to_vision_nodes<S: std::hash::BuildHasher>(
    entries: HashMap<String, DreamVisionEntry, S>,
    autobuilder_dir: &Path,
) -> Vec<VisionNode> {
    let mut nodes: Vec<VisionNode> = entries
        .into_iter()
        .map(|(slug, entry)| {
            // Resolve path: try the entry's path field, else build it from slug.
            let path = entry.path.unwrap_or_else(|| {
                autobuilder_dir
                    .join("visions")
                    .join(format!("{slug}.md"))
                    .to_string_lossy()
                    .into_owned()
            });
            VisionNode {
                slug,
                path,
                status: entry.status.unwrap_or_default(),
                prds_drafted: entry.prds_drafted.unwrap_or_default(),
                seed: entry.seed.unwrap_or_default(),
            }
        })
        .collect();
    nodes.sort_by(|a, b| a.slug.cmp(&b.slug));
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prd_frontmatter_full() {
        let content = r#"# PRD: atlas-core — the corpus, as a graph

**Author:** /dream
**Status:** Draft v0.1
**Vision:** visions/atlas.md
**build_target:** rust-cli
**build_into:** /home/jsy/wintermute/atlas
"#;
        let fm = parse_prd_frontmatter(content);
        assert_eq!(fm.title, "atlas-core — the corpus, as a graph");
        assert_eq!(fm.vision, "atlas");
        assert_eq!(fm.build_target, "rust-cli");
        assert_eq!(fm.build_into, "/home/jsy/wintermute/atlas");
    }

    #[test]
    fn test_parse_prd_frontmatter_missing_fields() {
        let content = "# PRD: minimal\n\nSome body text.\n";
        let fm = parse_prd_frontmatter(content);
        assert_eq!(fm.title, "minimal");
        assert!(fm.vision.is_empty());
        assert!(fm.build_target.is_empty());
    }

    #[test]
    fn test_parse_prd_frontmatter_skips_code_blocks() {
        let content = r#"# PRD: test

```
**Vision:** visions/fake.md
**build_target:** should-be-ignored
```

**Vision:** visions/real.md
**build_target:** rust-cli
"#;
        let fm = parse_prd_frontmatter(content);
        assert_eq!(fm.vision, "real");
        assert_eq!(fm.build_target, "rust-cli");
    }

    #[test]
    fn test_strip_vision_path() {
        assert_eq!(strip_vision_path("visions/atlas.md"), "atlas");
        assert_eq!(strip_vision_path("atlas"), "atlas");
        assert_eq!(strip_vision_path("visions/foo-bar.md"), "foo-bar");
    }

    #[test]
    fn test_parse_repos_md_line_standard() {
        let line = "- [atlas](https://github.com/j0yen/atlas) — corpus query CLI";
        let node = parse_repos_md_line(line).unwrap();
        assert_eq!(node.name, "atlas");
        assert_eq!(node.url, "https://github.com/j0yen/atlas");
        assert_eq!(node.description, "corpus query CLI");
    }

    #[test]
    fn test_parse_repos_md_line_no_description() {
        let line = "- [foo](https://github.com/j0yen/foo)";
        let node = parse_repos_md_line(line).unwrap();
        assert_eq!(node.name, "foo");
        assert!(node.description.is_empty());
    }

    #[test]
    fn test_parse_repos_md_line_header_skipped() {
        let line = "## Category";
        assert!(parse_repos_md_line(line).is_none());
    }

    #[test]
    fn test_parse_repos_md_line_table_format() {
        // The actual REPOS.md format: `| [name](url) | binary | description |`
        let line = "| [agorabus](https://github.com/j0yen/agorabus) | `agorabus` | Single-host advisory pub/sub bus |";
        let node = parse_repos_md_line(line).unwrap();
        assert_eq!(node.name, "agorabus");
        assert_eq!(node.url, "https://github.com/j0yen/agorabus");
        assert_eq!(node.description, "Single-host advisory pub/sub bus");
    }

    #[test]
    fn test_parse_repos_md_line_table_divider_skipped() {
        // Divider row: `|---|---|---|`
        let line = "|---|---|---|";
        assert!(parse_repos_md_line(line).is_none());
    }

    #[test]
    fn test_derive_prd_status_no_entry() {
        let map = HashMap::new();
        let (status, url, path) = derive_prd_status("PRD-foo.md", &map);
        assert_eq!(status, PrdStatus::Drafted);
        assert!(url.is_empty());
        assert!(path.is_empty());
    }

    #[test]
    fn test_derive_prd_status_in_flight() {
        let mut map = HashMap::new();
        map.insert(
            "PRD-foo.md".to_string(),
            BuildManifestEntry {
                path: Some("/autobuilder/PRD-foo.md".to_string()),
                output_repo_path: None,
                output_repo_url: None,
                status: Some("in_progress".to_string()),
            },
        );
        let (status, _, _) = derive_prd_status("PRD-foo.md", &map);
        assert_eq!(status, PrdStatus::InFlight);
    }
}
