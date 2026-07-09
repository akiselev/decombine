# Query Interface

`decombine query` is the agent-facing surface over an indexed database:
stable unit selectors, metadata filtering, vector-neighbor lookup, and
semantic search, all built on the embeddings decombine already computes.
The design and scope decisions live in
`docs/research/agent-query-interface.md`; this page documents what shipped.

Every example below is real output from running decombine against its own
source tree: two indexed projects, `main` (the current `src/`) and
`baseline` (`src/` from an older commit), embedded with `BGESmallENV15`.

## Ground rules

- Default output is human-readable text. `--json` switches stdout to a
  single machine-readable envelope; progress and warnings stay on stderr,
  so `decombine ... --json | jq` always works.
- Results are bounded with `--limit`. The summary always reports the full
  `matched` count plus `exhaustive`/`has_more`, never silently truncates.
  Re-running with a bigger `--limit` is the pagination story — no cursors.
- IDs are deterministic per index generation: the same DB yields the same
  IDs, and any edit to a unit changes its ID by design. Agents holding IDs
  across a re-index re-resolve them (`query units`, or re-run the query
  that produced them).
- `--why` (on `similar`/`search`/`qbe`) attaches inline, stateless score
  and decision explanations to each result. Nothing is persisted.

## Stable selectors

Every machine surface (query commands and analyzer `--json` reports) shares
one unit object:

```json
{
  "unit_id": "unit:9c03b5a918f28c4a",
  "db_unit_id": 120,
  "project": "main",
  "path": "analyze/vector_store.rs",
  "language": "rust",
  "kind": "function",
  "name": "similar_pairs",
  "scope": "VectorStore",
  "byte_range": [2613, 3798],
  "line_range": [83, 114],
  "body_node_count": 191,
  "body_hash": "9ca3d0b2..."
}
```

`unit:<16-hex>` hashes (project, path, byte range, body hash, name, scope,
language). `db_unit_id` is a debug field, not a durable selector. Duplicate
clusters keep their existing `cluster:<hash>` IDs; comparison records get
`match:<hash>` IDs stable over (class, member paths, member bodies).

## `--where` filters

`query units`, `similar`, and `search` accept the same metadata filter:

```text
--where 'language=rust kind=function path=src/** min_nodes=8'
```

- Different keys AND together; repeating a key ORs its values
  (`project=a project=b`).
- Keys: `project`, `language`, `kind` (exact match); `name`, `scope`,
  `path` (globs); `min_nodes=N` (minimum body node count).
- `path` globs treat `/` literally: `analyze/*` is direct children,
  `analyze/**` the whole subtree.

## Commands

### `query capabilities`

Orientation first: what this config/database can answer.

```console
$ decombine query capabilities
config: decombine.yaml
db: self.db
retention: report
project baseline: .../decombine-head/src (31 files, 320 units)
project main: /home/dev/git/decombine2/src (34 files, 456 units)
language rust: 776 units
model: BGESmallENV15 (384 dims, provider cpu)
embeddings: 481 bodies (0 pending)
concerns: disabled (1 queries)
```

With `--json` it also reports `display_source_available`, the configured
analyzers, and which query commands are usable right now
(`query_commands.similar` is `false` until `decombine embed` has run).
`embeddings.pending_bodies` is the staleness signal: non-zero means indexed
code exists that the model has not embedded yet, so vector results lag
recent edits.

### `query units`

Fact-table listing with metadata filters.

```console
$ decombine query units --where 'project=main kind=function path=analyze/** min_nodes=60' --limit 5
unit:e526614fa1b2afb1 main:analyze/compare/mod.rs:128-146 function hint_bonus
unit:a355a201cef21c77 main:analyze/compare/mod.rs:590-685 function abtt_store
unit:55c1871540409ad2 main:analyze/compare/mod.rs:698-738 function same_name_anchor
unit:492271388abc6e69 main:analyze/compare/mod.rs:740-871 function calibrate
unit:62cdbbada257e265 main:analyze/compare/mod.rs:873-906 function coverage
(5 of 37 shown; raise --limit for the rest)
```

The text line format is `unit_id project:path:start-end kind name (scope)`.
Works before embedding — only the index is needed.

### `query inspect`

Resolve a selector back to metadata and (with `--source`) the code.

```console
$ decombine query inspect unit:e526614fa1b2afb1 --source
unit:e526614fa1b2afb1 main:analyze/compare/mod.rs:128-146 function hint_bonus
bytes 4224-4924, 118 body nodes, body hash 71d2fa1a...

fn hint_bonus(config: &ComparisonConfig, a: &CodeUnitRef, b: &CodeUnitRef) -> f32 {
    let mut bonus: f32 = 0.0;
    if config.use_name_hints {
        ...
```

Source comes from the stored display source when retention kept it,
otherwise it is recovered from the project source tree by byte range. An
unknown ID fails with re-resolution guidance (IDs change on re-index).

### `query search`

Semantic search from natural language. Embeds the query with the configured
model (which must match the model the DB was embedded with) and ranks every
embedded unit by cosine.

```console
$ decombine query search --text "compute cosine similarity between vectors" \
    --where 'project=main' --limit 5
0.7136 unit:ad1a5e4a96cf9275 main:analyze/similarity_index.rs:35-37 function similar_pairs (ExactFlat<'_>)
0.6937 unit:b2abb281182c130e main:analyze/drift.rs:103-103 closure percentile
0.6786 unit:5413121571185709 main:analyze/compare/mod.rs:270-273 closure as_ref().and_then(...) (CompareAnalyzer)
0.6751 unit:9c03b5a918f28c4a main:analyze/vector_store.rs:83-114 function similar_pairs (VectorStore)
0.6663 unit:094f230fe1b3489b main:index/language.rs:160-160 closure trim_matches(...)
```

Raw cosine scores do not port across models (see CLAUDE.md); treat them as
a ranking, not an absolute quality bar.

### `query similar` / `query qbe`

Vector neighbors of a known unit. `qbe` (query by example) is the same
engine under the workflow name agents know; its `--neighbors` is an alias
for `--limit`.

```console
$ decombine query similar --unit unit:9c03b5a918f28c4a --limit 5
query: unit:9c03b5a918f28c4a main:analyze/vector_store.rs:83-114 function similar_pairs (VectorStore)
1.0000 unit:e4307c832e7b1509 baseline:analyze/vector_store.rs:83-114 function similar_pairs (VectorStore)
0.9307 unit:f59ccb635d023aab baseline:analyze/similarity_index.rs:35-37 function similar_pairs (ExactFlat<'_>)
0.9307 unit:ad1a5e4a96cf9275 main:analyze/similarity_index.rs:35-37 function similar_pairs (ExactFlat<'_>)
0.8856 unit:fcac85558be6ee75 baseline:analyze/vector_store.rs:89-105 closure par_iter().flat_map_iter(...) (VectorStore)
0.8856 unit:a76b183c7abeea85 main:analyze/vector_store.rs:89-105 closure par_iter().flat_map_iter(...) (VectorStore)
(top 5 of 775 candidates; raise --limit for more)
```

(The 1.0000 hit is the same function in the `baseline` arm — unchanged
across the two commits.) `--threshold` adds a raw-cosine floor; `--where`
narrows the candidate set. With `--why --json` each item carries its
evidence inline:

```json
{
  "unit_id": "unit:e4307c832e7b1509",
  "name": "similar_pairs",
  "project": "baseline",
  "score": 1.0,
  "why": {
    "scores": {"cosine": 1.0},
    "evidence": [{"source": "vector", "unit": "unit:9c03b5a918f28c4a"}],
    "decision": {"reason": "vector_rank", "filters": [], "threshold": null, "suppressed": false}
  }
}
```

## Analyzer JSON (`--json` on analyze/compare)

`decombine analyze duplicates|concerns --json` and
`decombine compare --json` print one JSON envelope on stdout instead of
writing the markdown report directory (markdown remains the default human
output; nothing scrapes it). `--limit` bounds the item list.

```console
$ decombine compare --json --limit 3 | jq .summary
{
  "matched": 448,
  "returned": 3,
  "exhaustive": false,
  "has_more": true,
  "exact_copy": 282,
  "strong_match": 28,
  "possible_match": 3,
  "split": 5,
  "merge": 1,
  "missing_in_right": 0,
  "new_in_right": 129
}
```

That run compares `baseline` against `main`: 282 functions unchanged, 28
strong matches (edited but recognizably the same), 129 new units in `main`
(the work landed between the two commits), nothing lost. Each match record
carries `match_id`, `class`, `score`, `hint_bonus`, and full unit objects
for both sides; the envelope also includes `calibration`,
`suppressed_right_candidates`, and coverage tables.

Note the ordering: match records sort by class (exact copies first,
`missing_in_right`/`new_in_right` last), so a `--limit` truncates the tail
classes first. When you plan to filter by class, take the full list — the
default (no `--limit`) returns everything.

```console
$ decombine analyze duplicates --json --limit 2 | jq .summary
{
  "matched": 147,
  "returned": 2,
  "exhaustive": false,
  "has_more": true,
  "candidate_pairs": 54815,
  "ignored_clusters": 0
}
```

Duplicate items carry `cluster_id`, section `kind`
(`product`/`mixed`/`test_or_docs`), scores, `name_family`, member unit
objects, surviving pairs, and exact-copy groups; the envelope adds
`cross_directory` candidates and `ignored` cluster IDs. (With both arms of
the same codebase indexed, most "duplicates" here are the cross-arm copies
plus the repo's real repeated `sort_by` idioms.)

Concern reports (`analyze concerns --json`) list one item per configured
concern query with spread metrics and scored units.

## Schema versions

| Output | `schema_version` |
| --- | --- |
| `query units/inspect/similar/search/qbe --json` | `decombine.query.v1` |
| `query capabilities --json` | `decombine.capabilities.v1` |
| `analyze duplicates --json` | `decombine.duplicates.v1` |
| `analyze concerns --json` | `decombine.concerns.v1` |
| `compare --json` | `decombine.compare.v1` |

Every envelope also carries `decombine_version`, the model identity, the
query/analysis arguments, and the bounding summary.

## Recipes

Find a concept, then expand from the best hit:

```sh
decombine query search --text "retry backoff timeout" --where 'kind=function' --limit 10 --json \
  | jq -r '.items[0].unit_id' \
  | xargs -I{} decombine query similar --unit {} --limit 10
```

Audit a rewrite (two labeled projects, `comparison.left`/`right` in
config). No `--limit` here: matches sort by class and class filters need
the tail of the list:

```sh
decombine compare --json > compare.json
jq -r '.items[] | select(.class == "missing_in_right") | .left[0].path' compare.json
decombine query inspect unit:<left-id> --source
```

Check freshness before trusting vector results:

```sh
decombine query capabilities --json | jq '.embeddings.pending_bodies'
```
