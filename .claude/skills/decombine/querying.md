# Query interface: explore an indexed codebase

`decombine query` is built for agents: semantic search, vector neighbors,
and metadata lookup over the indexed units, with stable IDs and bounded
JSON output. Full reference with examples: `docs/query-interface.md` in
the decombine repo.

Ground rules:

- Default output is human-readable text; add `--json` for one envelope on
  stdout (progress stays on stderr, so piping to `jq` is safe).
- `--limit N` bounds results; the summary always reports the true
  `matched` count plus `exhaustive`/`has_more`. Re-run with a bigger limit
  instead of paginating.
- `unit:<16-hex>` IDs are deterministic per index generation and change
  when the code changes. After a re-index, re-resolve IDs (re-run the
  query); never cache them across edits.

## Orient first

```sh
decombine query capabilities --json
```

Reports projects, languages, unit counts, model identity, embedding count
and `pending_bodies` (non-zero = vector results are stale for recent
edits), and which query commands are currently usable. Run this before
vector queries; `similar`/`search` need `decombine embed` to have run.

## Commands

```sh
# List units by metadata (works pre-embedding)
decombine query units --where 'language=rust kind=function path=src/** min_nodes=20' --limit 20

# Resolve an ID to metadata + source text
decombine query inspect unit:<id> --source

# Natural-language semantic search (embeds your query with the configured model)
decombine query search --text "retry with exponential backoff" --limit 10

# Nearest neighbors of a known unit (qbe = same engine, --neighbors alias)
decombine query similar --unit unit:<id> --limit 10
decombine query qbe --unit unit:<id> --neighbors 10
```

Text output format: `unit_id project:path:startline-endline kind name (scope)`,
prefixed with the cosine score for search/similar.

## `--where` filter (units, similar, search)

```text
--where 'language=rust kind=function path=src/** min_nodes=8'
```

- Different keys AND; repeating a key ORs its values (`project=a project=b`).
- `project`, `language`, `kind`: exact. `name`, `scope`, `path`: globs.
  `min_nodes=N`: minimum body node count (filters trivial units).
- Path globs treat `/` literally: `src/*` = direct children, `src/**` = subtree.

## Flags that matter

- `--why` (similar/search/qbe): inline per-result explanation — score
  components, applied filters, evidence. Stateless, computed at query time.
- `--threshold` (similar/qbe): raw-cosine floor. Cosine scales are
  model-specific; prefer ranking (`--limit`) over absolute cutoffs unless
  you know the model's scale.

## Recipes

Find a concept, then expand from the best hit:

```sh
decombine query search --text "parse configuration file" --limit 10 --json \
  | jq -r '.items[0].unit_id' \
  | xargs -I{} decombine query similar --unit {} --limit 10
```

Find all twins of a function you're about to change:

```sh
decombine query qbe --unit unit:<id> --neighbors 20 --why --json
```

Interpretation: on most code-embedding models, neighbors ≳0.9 raw cosine
are near-duplicates worth reading before you edit; mid-range scores are
"same topic", not copies. Scores near 1.0 with a different path = an exact
copy elsewhere.
