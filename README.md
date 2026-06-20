# atlas

The wintermute PRD corpus as a queryable graph of typed nodes — visions, PRDs, and shipped repos.

The corpus already describes one connected structure: a vision owns PRDs, and PRDs ship to repos. But that structure lives only in the reader's head — spread across PRD frontmatter, two skill manifests, and `REPOS.md`, with nothing that lets you ask it a question. atlas makes it an object you can query. It parses every PRD's frontmatter, both manifests, and `REPOS.md` into one in-memory graph of typed nodes, then exposes it: `atlas nodes` lists them, `atlas show <vision>` walks one vision's PRDs, and every command takes `--format json`.

The graph is rebuilt fresh on each invocation; there is no persistent store, so the answer always reflects the corpus as it is right now.

**Read-only invariant:** atlas never writes to the autobuilder corpus or any manifest. It reports drift; it never repairs it.

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
atlas doctor [--format text|json] [--class <name>]
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

## Corpus health: `atlas doctor`

`atlas doctor` is a **read-only** lint over the full corpus. It reports five
classes of structural divergence that are invisible to manual inspection across
a corpus of 100+ PRDs, then exits with a code that reflects the highest severity
found. It **never writes to any PRD, manifest, gossip file, or REPOS.md** — it
reports drift; it never repairs it.

### Usage

```bash
atlas doctor                              # report all classes, text output
atlas doctor --format json                # machine-readable JSON array
atlas doctor --class shipped_repo_gone    # restrict to one class
```

### Divergence classes

| class                | severity | fires when                                                                           |
|----------------------|----------|--------------------------------------------------------------------------------------|
| `prd_no_vision`      | warn     | PRD's `Vision:` field is empty or names a vision doc that does not exist             |
| `vision_no_prd`      | info     | vision has an empty `prds_drafted` list and no PRD's `Vision:` field points at it   |
| `repo_no_prd`        | warn     | REPOS.md entry has no originating PRD (no PRD title, slug, `build_into`, or `output_repo_path` maps to it) |
| `shipped_repo_gone`  | warn     | build manifest records a non-empty `output_repo_path` for a PRD but that path does not exist on disk |
| `fulfilled_unmarked` | info     | vision is marked `active` in the dream manifest but every PRD in its `prds_drafted` list derives status `shipped` |

### Exit-code contract

| exit | meaning                                                                     |
|------|-----------------------------------------------------------------------------|
| `0`  | no findings — corpus is clean                                               |
| `1`  | only info-level findings (`vision_no_prd`, `fulfilled_unmarked`)            |
| `2`  | at least one warn-level finding (`prd_no_vision`, `repo_no_prd`, `shipped_repo_gone`) |

This contract is stable, so a caller (e.g. `self-review`) can gate on the exit
code without parsing output.

### JSON output

`--format json` emits a JSON array; each element has four fields:

```json
[
  {
    "class":  "shipped_repo_gone",
    "node":   "PRD-some-tool.md",
    "detail": "output_repo_path '/home/jsy/wintermute/some-tool' does not exist on disk",
    "source": "PRD-some-tool.md"
  }
]
```

### Advisory + read-only

`atlas doctor` is **advisory**. It surfaces divergence for a human or skill to
act on; it never modifies the corpus. Running it over the live corpus leaves
every file's mtime unchanged.

## Performance

There is no persistent store. Every command parses the PRDs, visions, manifests,
and `REPOS.md` from scratch and builds the graph in memory, so the result is
always current and a stale index is never a failure mode. The corpus is a few
hundred small text files, and a cold run finishes well inside an interactive
budget.

## Install

```bash
cargo install --path .
# or, from the wintermute repo layout:
install -Dm755 target/release/atlas ~/.local/bin/atlas
```

## License

MIT — Joe Yen, 2026
