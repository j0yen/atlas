# Changelog

## v0.4.0 — 2026-06-02

Adds `atlas graph` — render the vision→PRD→repo model as Graphviz DOT,
Mermaid, or a terminal tree.

**New:** `atlas graph [--format dot|mermaid|tree] [--vision <slug>] [--shipped-only]`

- `--format dot` — Graphviz `digraph atlas {}`: vision nodes (ellipse, blue),
  PRD nodes (box, colored by status: grey=drafted, amber=in-flight,
  green=shipped), repo nodes (cylinder, red); vision→PRD edges dotted blue,
  PRD→PRD dependency edges solid (frontmatter) or dashed (gossip),
  shipped-PRD→repo edges gray.
- `--format mermaid` — `graph TD` block: same topology; gossip edges `-.->`,
  frontmatter `-->`.  Renders inline in GitHub Markdown.
- `--format tree` — terminal tree: vision as root, PRDs indented with status
  glyph (○ drafted / ◑ in-flight / ● shipped); dependency arrows inline.
  Default format; no Graphviz install needed.
- `--vision <slug>` scopes output to one vision's subgraph.
- `--shipped-only` prunes to shipped PRDs and their repos only.
- Output is deterministic: nodes sorted by (vision_slug, prd_filename),
  edges by (from, to) — byte-identical on re-run with unchanged corpus.

No new dependencies; pure string templating.  All edge ACs implemented
(atlas-edges API was already present from v0.2.0).  37 new unit tests
covering all three renderers; all 49 tests green; clippy clean.

## v0.3.0 — 2026-05-30

Adds `atlas doctor`: a read-only lint that surfaces five corpus divergence
classes invisible to manual inspection — PRDs with no vision, visions with
no PRDs, repos with no originating PRD, shipped PRDs whose repo path is
gone on disk, and active visions whose every drafted PRD is shipped
(fulfilled-but-unmarked). Exit code reflects severity: 0=clean, 1=info,
2=warn. All five classes have positive and negative fixture tests (19 doctor
unit tests; all 50 tests green). Read-only invariant verified: no writes to
any PRD, manifest, gossip, or REPOS.md.

## v0.2.0 — 2026-05-30

Adds typed dependency edges between PRDs, closing the atlas-edges PRD.

**New:** `atlas deps <prd> [--format text|json]` — shows what a PRD depends on
and what depends on it, with per-edge `kind` (frontmatter or gossip) and
`source` provenance.

**New:** `atlas blocked [--format text|json]` — lists every PRD that has at
least one dependency whose status is not `shipped`; the "what can't build yet,
and why" view.

**New:** edge model (serde-serializable `Edge` / `EdgeKind`) with two parsers:
- Frontmatter parser: reads `**Depends on:**` lines from each PRD.  Authoritative.
- Gossip parser: reads `Order:` blocks in `notes/gossip.md` for `→` / `->` /
  `depends on` arrows.  Best-effort; duplicate of a frontmatter edge is silently
  dropped.

Unresolvable endpoints are counted in `unresolved` (no panics, no phantom nodes).
README now documents the frontmatter-over-gossip precedence rule and the full
edge model.

## v0.1.0 — 2026-05-29

Initial release — atlas-core.  Node model (vision, prd, repo), `atlas nodes`,
`atlas show <vision>`, `--format json`, env-overridable source paths.
