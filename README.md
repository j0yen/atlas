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
atlas --version
atlas --help
```

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
