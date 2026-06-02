# atlas

Queryable node graph of the wintermute PRD corpus: visions, PRDs, and shipped repos.

107 PRDs, 24 vision docs, two manifests, and a 117-line REPOS.md describe a single
connected structure — vision owns PRDs, PRDs ship to repos — but the structure lives
only in the reader's head. atlas-core is the substrate that makes it a queryable
object: parse every PRD's frontmatter, both skill manifests, and REPOS.md into one
in-memory graph of typed nodes (vision, prd, repo), and expose it via `atlas nodes`
and `atlas show <vision>`, every command offering `--format json`.

**Read-only invariant:** atlas never writes to the autobuilder corpus or any manifest.

## Node model

| kind     | id       | fields                                                                        |
|----------|----------|-------------------------------------------------------------------------------|
| `vision` | slug     | `path`, `status`, `prds_drafted[]`, `seed`                                   |
| `prd`    | filename | `title`, `vision`, `build_target`, `build_into`, `status`, `repo_url`, `repo_path` |
| `repo`   | name     | `url`, `description`                                                          |

**PRD status** is derived at load time:
- `drafted` — no manifest entry or no last_action
- `in_flight` — manifest entry exists but not yet shipped
- `shipped` — `output_repo_path` in the manifest is non-empty and exists on disk

## Source resolution

| path              | env override          | default                                          |
|-------------------|-----------------------|--------------------------------------------------|
| PRDs + visions    | `ATLAS_AUTOBUILDER`   | `~/wintermute/autobuilder`                       |
| build manifest    | `ATLAS_BUILD_MANIFEST`| `~/.claude/skills/build/state/manifest.json`     |
| dream manifest    | `ATLAS_DREAM_MANIFEST`| `~/.claude/skills/dream/state/manifest.json`     |
| repos index       | `ATLAS_REPOS`         | `~/wintermute/REPOS.md`                          |

All four paths are env-overridable, and the test suite drives parsers off fixture
directories — never touching live `~/.claude` or `~/wintermute` paths.

## Edge model

Dependency edges connect PRDs that declare `**Depends on:**` frontmatter or appear
in `Order:` blocks in `notes/gossip.md`.

| field    | notes                                                                  |
|----------|------------------------------------------------------------------------|
| `from`   | dependent PRD filename                                                 |
| `to`     | prerequisite PRD filename                                              |
| `kind`   | `frontmatter` (authoritative) or `gossip` (best-effort)               |
| `source` | `file:line` provenance of the edge                                     |

**Precedence rule:** when both sources assert the same `(from, to)` pair, the
frontmatter edge wins and the gossip edge is silently dropped.  A gossip-only
edge is kept and tagged `kind: gossip` so callers can see it is the softer
signal.  Endpoints that cannot be resolved to a known PRD are dropped and
counted in the `unresolved` field (never panics).

## Commands

```
atlas nodes [--kind vision|prd|repo] [--format text|json]
atlas show <vision-slug> [--format text|json]
atlas deps <prd> [--format text|json]
atlas blocked [--format text|json]
atlas graph [--format dot|mermaid|tree] [--vision <slug>] [--shipped-only]
atlas --version
atlas --help
```

## Graph rendering

`atlas graph` draws the vision→PRD→repo web as a Graphviz DOT graph, a Mermaid
diagram, or a terminal tree.  Output is deterministic: re-running over an
unchanged corpus produces byte-identical text, so committed `.dot`/`.mmd` files
diff cleanly.

### Mermaid — inline in markdown

```bash
atlas graph --format mermaid --vision atlas
```

```
graph TD
  %% Vision nodes
  vision_atlas(("atlas"))
  %% PRD nodes
  prd_PRD_atlas_edges_md(["atlas-edges"])
  prd_PRD_atlas_orphans_md(["atlas-orphans"])
  prd_PRD_atlas_render_md(["atlas-render"])
  %% Repo nodes
  repo_https___github_com_j0yen_atlas[("j0yen/atlas")]
  %% Vision to PRD edges
  vision_atlas --> prd_PRD_atlas_edges_md
  vision_atlas --> prd_PRD_atlas_orphans_md
  vision_atlas --> prd_PRD_atlas_render_md
  %% PRD dependency edges
  prd_PRD_atlas_orphans_md --> prd_PRD_atlas_edges_md
  prd_PRD_atlas_render_md --> prd_PRD_atlas_edges_md
  %% Shipped PRD to Repo edges
  prd_PRD_atlas_edges_md --> repo_https___github_com_j0yen_atlas
  prd_PRD_atlas_orphans_md --> repo_https___github_com_j0yen_atlas
```

Node shapes encode kind: `(( ))` = vision, `[ ]` = shipped PRD, `([ ])` = in-flight
PRD, repository nodes use `[( )]`.  Paste this block into any GitHub README or
Mermaid Live Editor and it renders immediately.

### DOT — pipe into Graphviz

```bash
atlas graph --format dot --vision atlas | dot -Tsvg -o atlas-vision.svg
```

atlas emits valid DOT text; rendering to SVG/PNG is handled by the `dot`
binary from [Graphviz](https://graphviz.org/).  The generated digraph uses:

- **ellipse / blue fill** for vision nodes
- **box / green fill** for shipped PRDs, yellow for in-flight, grey for drafted
- **cylinder / red fill** for repo nodes
- **solid edges** for `frontmatter` dependencies, **dashed** for `gossip`

To view the whole corpus (all visions, all PRDs):

```bash
atlas graph --format dot | dot -Tsvg -o wintermute.svg
xdg-open wintermute.svg
```

### Tree — no Graphviz required

```bash
atlas graph --format tree --vision atlas
```

```
vision: atlas  [active]
  ├── ● atlas-edges  → https://github.com/j0yen/atlas
  ├── ● atlas-orphans  ← needs: atlas-edges  → https://github.com/j0yen/atlas
  └── ● atlas-render  ← needs: atlas-edges
```

Status glyphs: `●` shipped, `○` in-flight, `·` drafted.  Dependency arrows
(`← needs: …`) show PRD prerequisites inline.  This is the default format
when `--format` is omitted.

## Performance

Cold run of `atlas nodes` over the full live corpus (107 PRDs, 24 visions,
REPOS.md, two manifests) completes in **~23 ms** — well under the 200 ms budget.
No persistent store; sources are parsed fresh on each invocation.

## Install

```bash
cargo install --path .
# or, from the wintermute repo layout:
install -Dm755 target/release/atlas ~/.local/bin/atlas
```

## License

MIT — Joe Yen, 2026
