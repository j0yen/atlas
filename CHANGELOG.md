# Changelog

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
