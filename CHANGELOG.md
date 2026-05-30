# Changelog

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
