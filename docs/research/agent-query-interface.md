# Agent Query Interface Research

Date: 2026-07-08 (scope decisions recorded 2026-07-09)

Status: research synthesis, reviewed and descoped. The in-scope core
(roadmap Steps 1–3) shipped 2026-07-09: analyzer `--json`/`--limit` output
(`src/report/json.rs`) and the `decombine query`
capabilities/inspect/units/similar/search/qbe family (`src/query/`).
Step 4 (graph expansion), JSONL streaming, the `schema` command, and the
exit-code/`--fail-on` policy remain unimplemented.

Scope decisions (2026-07-09, see Decisions at the bottom):

- Build the embeddings-native core only: JSON output for existing analyzers,
  then `capabilities`/`inspect`/`units`, then `similar`/`search`/`qbe`.
- Text/AST search, tree-sitter query files, and YAML query packs are cut:
  agents already have ripgrep and ast-grep; decombine's unique asset is
  embeddings and its analyzers. The sections below are kept as reference
  research, not as a plan.
- Default output is human-readable text; `--json`/`--jsonl` are opt-in flags.
- Pagination is `--limit` plus an `exhaustive` flag only. No cursor protocol.
- Explanations are inline per result behind `--why`. No persisted query runs,
  no `explain result:<id>`.
- CLI JSON is the only agent surface. MCP is explicitly out of scope.

## Question

How should decombine expose a query interface that coding agents can use to
explore codebases faster than humans?

The goal is not a prettier human report. The goal is a stable, scriptable,
machine-readable interface over decombine's indexed code units, embeddings,
duplicate clusters, concern projections, comparison matches, and future
exploration features.

Short answer: build a typed query pipeline with JSON/JSONL as the stable
contract. Do not make agents scrape Markdown. Do not start with one giant query
language. Start with a small family of explicit commands, stable selectors,
explainable result objects, and bounded output. Only build what nothing else
on the agent's box can do: everything rides on decombine's embeddings and
analyzer outputs, not on lexical or structural search.

## Method

Local code inspected:

- `src/cli.rs`
- `src/main.rs`
- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/duplicate/mod.rs`
- `src/analyze/concerns/mod.rs`
- `src/analyze/compare/mod.rs`
- `src/db/migrations.rs`
- `src/report/markdown.rs`
- `RESEARCH.md`
- existing `docs/research/*`

Parallel research lanes:

- local CLI/analyzer inventory;
- agent-oriented query UX patterns;
- code intelligence systems and query languages;
- machine-readable CLI output design.

Primary references:

- ripgrep JSON/event output and search defaults:
  <https://manpages.debian.org/testing/ripgrep/rg.1.en.html>
- jq manual:
  <https://jqlang.org/manual/>
- SQLite CLI output modes:
  <https://sqlite.org/climode.html>
- Sourcegraph code search queries:
  <https://sourcegraph.com/docs/code-search/queries>
- Sourcegraph streaming search API:
  <https://sourcegraph.com/docs/api/graphql/search>
- Sourcegraph structural search caveats:
  <https://sourcegraph.com/docs/code-search/types/structural>
- CodeQL language and query outputs:
  <https://codeql.github.com/docs/ql-language-reference/about-the-ql-language/>
- CodeQL data-flow queries:
  <https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/>
- CodeQL SARIF output:
  <https://docs.github.com/en/code-security/reference/code-scanning/codeql/codeql-cli/sarif-output>
- Semgrep CLI and rule syntax:
  <https://docs.semgrep.dev/cli-reference>
  <https://docs.semgrep.dev/writing-rules/rule-syntax>
- Tree-sitter query syntax:
  <https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html>
  <https://tree-sitter.github.io/tree-sitter/using-parsers/queries/3-predicates-and-directives.html>
- ast-grep CLI/rule/JSON docs:
  <https://ast-grep.github.io/reference/cli.html>
  <https://ast-grep.github.io/guide/rule-config.html>
  <https://ast-grep.github.io/guide/tools/json.html>
- LSP 3.17 specification:
  <https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/>
- SCIP:
  <https://scip-code.org/>
- LSIF:
  <https://microsoft.github.io/language-server-protocol/specifications/lsif/0.4.0/specification/>
- OpenGrok REST:
  <https://github.com/oracle/opengrok/wiki/Web-services-%28RESTful-API%29>
- Hound:
  <https://github.com/hound-search/hound>
- Qdrant points/filtering:
  <https://qdrant.tech/documentation/manage-data/points/>
  <https://qdrant.tech/documentation/search/filtering/>
- LanceDB hybrid search:
  <https://docs.lancedb.com/search/hybrid-search>
- Chroma query/get:
  <https://docs.trychroma.com/docs/querying-collections/query-and-get>
- Google AIP-158 pagination:
  <https://google.aip.dev/158>

## Current Local Surface

`decombine` currently exposes:

```text
decombine init
decombine show-config
decombine index [--project]
decombine embed
decombine tokens
decombine analyze [duplicates|concerns]
decombine compare [--left --right]
decombine run
decombine languages list
decombine models list
decombine models download
decombine doctor [--provider]
decombine drift [--baseline --candidate ...]
```

Implemented analysis algorithms:

- duplicate clusters:
  - exact dense cosine over normalized vectors;
  - overlap suppression;
  - body-size gates;
  - distance reranking;
  - per-unit edge cap;
  - bounded union-find;
  - exact-copy folding;
  - same-name family downranking;
  - ignore-file suppression;
  - cross-directory candidate scoring.
- concern projections:
  - embed configured natural-language concern query;
  - normalize;
  - dot-score every code unit;
  - report top units and spread metrics.
- comparison:
  - exact copies;
  - cross-project top-k candidate edges;
  - name/path hint ranking;
  - high-fanout right-target suppression;
  - split/merge/strong/possible/missing/new classification;
  - optional background calibration;
  - optional ABTT.

Queryable data already exists:

```text
projects
files
code_units
embedding_models
embeddings
analysis_runs
analysis_artifacts
```

`AnalysisContext` exposes:

- selected projects;
- code unit refs;
- project label;
- path;
- language;
- kind;
- name;
- scope;
- byte and line ranges;
- body node count;
- body hash;
- optional display source;
- model identity;
- normalized vectors.

Main gaps (in scope):

- no `query`, `search`, `inspect`, `qbe`, or JSON-output command;
- reports are Markdown-only and brittle for agents to parse;
- no stable unit selector syntax;
- no score-explanation schema;
- no result limiting with an explicit exhaustiveness signal;
- no `capabilities` command to tell agents what is indexed.

Non-gaps (deliberately not built): lexical/BM25 search, cursor pagination,
reusable query/session state. Agents bring ripgrep for lexical search;
re-running a local query with a bigger `--limit` costs milliseconds.

## Design Principles

## 1. Do Not Build One Magic Query Language First

The best shape is a typed query family. In scope:

```text
units        metadata/fact-table filtering
inspect      resolve a selector, return unit metadata and source
similar      vector neighbors of an indexed unit
search       semantic search from a natural-language query
qbe          query by example
report       structured duplicate/concern/compare results (via --json)
capabilities what this DB can answer
```

Cut (agents already have better tools for these; revisit only on demonstrated
need):

```text
text      literal/regex/token search        -> ripgrep
ast       structural search                  -> ast-grep / tree-sitter CLI
packs     YAML multi-signal query packs      -> untested ranking, no consumer
graph     edge expansion                     -> cheap later add-on over
                                                existing cluster/compare data
```

Each command should have a clear contract.

## 2. Machine Output Is the Product for Agents

Human reports can stay Markdown. Agents need:

- JSON for bounded final results;
- JSONL/NDJSON for streams and large result sets;
- stable schema versions;
- stable IDs;
- deterministic ordering;
- byte ranges;
- result explanations;
- result bounding (`--limit` + exhaustiveness reporting);
- skipped-file and non-exhaustive-result metadata.

Never require agents to parse Markdown tables.

## 3. Output and Progress Must Not Mix

In machine modes:

```text
stdout = data only
stderr = progress, warnings, diagnostics
```

This matches the existing decombine habit of sending compare progress to
stderr. Keep that boundary and make it explicit.

Default output is human-readable text (which agents can read too). Machine
output is opt-in via flags, not an `--output` enum:

```text
--json     bounded JSON envelope on stdout
--jsonl    JSONL event stream on stdout (large/streaming result sets)
--progress text|jsonl|none
```

JSONL progress should go to stderr.

## 4. Stable Selectors Beat Line Numbers

Line numbers alone are not stable. What we can honestly promise: IDs that are
deterministic across re-runs of the same index generation. IDs include byte
ranges and the body hash, so any edit to a unit changes its ID — that is by
design; "survives edits" would require a fuzzy re-anchoring subsystem we are
not building. Agents re-resolve after re-indexing:

```json
{
  "unit_id": "unit:sha256:...",
  "project": "main",
  "path": "src/foo.rs",
  "language": "rust",
  "kind": "function",
  "name": "parse_record",
  "scope": "Parser",
  "byte_range": [1204, 1899],
  "line_range": [41, 67],
  "body_hash": "sha256:...",
  "source_hash": "sha256:...",
  "content_fingerprint": "sha256:..."
}
```

Selector layers:

- repo/worktree identity;
- optional git revision or dirty snapshot hash;
- normalized project/path;
- byte range;
- line range for humans;
- language and node kind;
- symbol name when available;
- body/source hash;
- parent/scope chain;
- AST hash later.

Deferred: a `query resolve <selector>` command with `exact/moved/ambiguous/
missing/stale_index` states is a fuzzy re-anchoring subsystem hiding in one
command. `query inspect` returning `found`/`missing` against the current index
is enough; agents that hold IDs across edits re-index and re-query.

## 5. Every Result Needs a Why

For an agent, a raw score is not enough. With `--why`, each result should
carry inline:

- score components;
- filters applied;
- thresholds;
- suppression/downrank reasons;
- whether the result set is exhaustive;
- whether the index is stale.

Explanations are stateless and inline. There is no `explain result:<id>`
command: it would require persisting query runs (statefulness, GC concerns)
for something `--why` provides at query time. `query inspect unit:<id>` covers
after-the-fact digging because units, unlike transient results, live in the DB.

## Proposed Command Surface

## Phase 0: Structured Existing Reports

Before a broad query system, add machine output for existing analyzers:

```sh
decombine compare --json --limit 100
decombine compare --jsonl --progress jsonl
decombine analyze duplicates --json
decombine analyze concerns --json
decombine schema compare --schema-version 1
```

This creates the stable ID/schema/page/explain discipline that later query
commands can reuse.

Highest-value first implementation:

```sh
decombine compare --json --limit 100
```

(No `--fields` projection: every agent has jq; column selection is not worth
the surface area.)

Reasons:

- comparison output is already structured internally;
- agents need it for rewrite/reimplementation reviews;
- scores, hints, calibration, ABTT, and fanout suppression are important to
  expose;
- it exercises stable IDs and result bounding.

## Phase 1: Read-Only Query Commands

```sh
decombine query capabilities --json
decombine query units --where 'language=rust kind=function path=src/**' --json
decombine query inspect unit:<id> --json
decombine query similar --unit unit:<id> --limit 20 --json
decombine query search --text "retry backoff timeout" --limit 50 --json
decombine query qbe --unit unit:<id> --neighbors 50 --why --json
```

`capabilities` should report:

- config path;
- DB path;
- indexed projects;
- languages indexed;
- model identity;
- embedding availability;
- retention mode;
- display-source availability;
- exact vector search availability;
- analyzers available;
- structural query support by language;
- symbol/call graph availability when added;
- index staleness/dirty worktree status.

## Phase 2: Structural and Fact Queries (CUT except `query facts`-style filters)

Cut 2026-07-09: structural search duplicates ast-grep/tree-sitter CLI, which
agents already have. The metadata-filter half survives as `query units
--where`. Kept below as reference only.

```sh
decombine query ast \
  --lang rust \
  --pattern 'if let Err($E) = $X { $$$BODY }' \
  --jsonl

decombine query facts \
  --from units \
  --where 'language="rust" and body_node_count > 20' \
  --select id,path,name,body_node_count \
  --json
```

Tree-sitter query mode:

```sh
decombine query tree-sitter \
  --lang rust \
  --query-file queries/retry.scm \
  --jsonl
```

AST result evidence should include captures:

```json
{
  "source": "ast",
  "pattern": "if let Err($E) = $X { $$$BODY }",
  "captures": {
    "E": {"text": "err", "byte_range": [1250, 1253]},
    "X": {"text": "client.send()", "byte_range": [1232, 1245]}
  }
}
```

## Phase 3: Query Packs (CUT)

Cut 2026-07-09: multi-signal hybrid ranking rests on invented weights (the
`0.55/0.20/0.15/0.10` combine below was never measured — any such weighting
is an EXPERIMENTS.md experiment, not a spec), and no consumer exists. Kept as
reference only.

YAML query packs let agents run repeatable multi-signal queries:

```yaml
version: 1
id: retry-error-handling
description: Find retry/error-handling candidates for agent review.

from: units

where:
  language: rust
  path:
    include: ["src/**"]
    exclude: ["tests/**"]
  kind: ["function", "method"]
  min_body_node_count: 8

retrieve:
  lexical:
    any: ["retry", "timeout", "backoff", "transient"]
  vector:
    query: "retry transient error timeout exponential backoff"
    top_k: 100
    min_score: 0.72
  ast:
    any:
      - lang: rust
        pattern: 'Err($E)'
      - lang: rust
        tree_sitter: |
          (call_expression
            function: (identifier) @call.name
            (#match? @call.name "retry|sleep|timeout"))

rank:
  combine:
    vector: 0.55
    lexical: 0.20
    ast: 0.15
    path_distance: 0.10

return:
  snippets: true
  byte_ranges: true
  score_breakdown: true
  evidence: true
```

Command:

```sh
decombine query run retry-error-handling.yaml --format jsonl --limit 50
```

## Phase 4: Graph Expansion (DEFERRED)

Deferred 2026-07-09: the cheap edges (`same_file`, `similar`, `duplicate`,
`compare_match`, `concern_hit`) all derive from data decombine already has,
so this can land later without new infrastructure. Symbol-graph edges
(`calls`, `references`, ...) are further out still.

Agents need context expansion:

```sh
decombine query graph \
  --from unit:<id> \
  --edges contains,calls,refs,similar,cluster \
  --depth 2 \
  --json
```

Initial edge types:

```text
contains       file/module/scope contains unit
same_file      units in same file
similar        vector neighbor
duplicate      same duplicate cluster
compare_match  comparison relation
concern_hit    concern projection relation
```

Later edge types:

```text
defines
references
calls
called_by
imports
implements
overrides
```

Do not block the query interface on precise symbol graphs. Add graph edges
incrementally.

## Output Formats

## JSON Envelope

For bounded results:

```json
{
  "schema_version": "decombine.query.v1",
  "decombine_version": "x.y.z",
  "kind": "query_result",
  "run": {
    "run_id": "run:sha256:...",
    "analysis_run_id": 42,
    "config_hash": "sha256:...",
    "db_path": "decombine.db",
    "rerun_command": "decombine query ..."
  },
  "model": {
    "backend": "fastembed",
    "model": "BGESmallENV15",
    "dimensions": 384,
    "execution_provider": "cpu"
  },
  "query": {
    "mode": "similar",
    "args": {}
  },
  "summary": {
    "matched": 100,
    "returned": 50,
    "exhaustive": false,
    "timeout": false,
    "skipped": 3
  },
  "items": [],
  "page": {
    "limit": 50,
    "has_more": true,
    "sort": ["-score", "unit_id"]
  }
}
```

## JSONL Events

For streams:

```json
{"schema_version":"decombine.query_event.v1","event":"begin","run_id":"run:..."}
{"schema_version":"decombine.query_event.v1","event":"match","item":{...}}
{"schema_version":"decombine.query_event.v1","event":"progress","searched":1000}
{"schema_version":"decombine.query_event.v1","event":"skipped","path":"vendor/x.rs","reason":"ignored"}
{"schema_version":"decombine.query_event.v1","event":"summary","matched":100,"returned":50}
```

In machine modes:

```text
stdout: match/summary data
stderr: progress/warnings/errors unless --progress jsonl is requested
```

If `--progress jsonl` is used, progress events still go to stderr.

## Stable IDs

Do not expose `usize` indexes as durable IDs.

Suggested IDs:

```text
unit:<hash>
match:<hash>
cluster:<hash>
query-run:<hash>
artifact:<id>
```

Unit ID ingredients:

```text
project label
relative path
start byte
end byte
normalized body hash
name
scope
language
```

For exact-copy groups, the same body hash may appear at several locations; the
unit ID must include location or DB `code_units.id` plus a content fingerprint.

Expose DB row IDs as debug fields, not durable selectors:

```json
{
  "unit_id": "unit:sha256:...",
  "db_unit_id": 1234
}
```

## Limits (no cursors)

Decided 2026-07-09: no cursor protocol. Opaque cursors with query-hash and
index-generation invalidation are hosted-API design (AIP-158); this is a local
CLI over local SQLite where re-running with a bigger `--limit` costs
milliseconds and is deterministic. Every command that can return many results
supports:

```text
--limit N
--sort KEY
--all
--timeout 10s
```

Always report:

```json
{
  "exhaustive": false,
  "limit_hit": true,
  "timeout": false,
  "has_more": true
}
```

An agent that hits `has_more: true` re-runs with a larger `--limit` or `--all`.

## Explainability

Every result should have compact inline evidence when `--why` is passed
(stateless — computed at query time, never persisted):

```json
{
  "scores": {
    "final": 0.842,
    "cosine": 0.842,
    "background": 0.698
  },
  "evidence": [
    {"source": "vector", "query": "retry transient error timeout exponential backoff"}
  ],
  "decision": {
    "reason": "vector_rank",
    "filters": ["language=rust", "path=src/**"],
    "suppressed": false
  }
}
```

Then allow deeper inspection of things that live in the DB (units, not
transient results):

```sh
decombine query explain unit:<id> --neighbors vector,cluster,compare --json
```

Skipped files are reported inline in the summary/JSONL events of the run that
skipped them, not via a separate command over a persisted run.

## Schema Command

Agents should be able to discover the contract:

```sh
decombine schema list --json
decombine schema query-result --schema-version 1
decombine schema compare --schema-version 1
```

Return JSON Schema 2020-12 or a documented stable equivalent.

## Exit Codes

Separate command success from findings:

```text
0 success, even if findings exist
1 runtime/internal failure
2 CLI usage or config validation error
3 input/index/model state error
4 partial output produced
```

Policy failures should be explicit:

```sh
decombine analyze duplicates --fail-on findings
decombine compare --fail-on missing-in-right
```

Without `--fail-on`, findings should not make the command fail.

## Agent Workflow Examples

## Find a Concept Quickly

```sh
decombine query search \
  --text "retry backoff timeout transient error" \
  --where 'language=rust path=src/** kind=function' \
  --limit 20 \
  --json
```

Agent reads top results, then expands one:

```sh
decombine query graph --from unit:<id> --edges similar,cluster,same_file --depth 1 --json
```

## Audit a Rewrite

```sh
decombine compare --left old --right new --json --limit 100 --why
decombine query inspect unit:<left-id> --source --json
decombine query inspect unit:<right-id> --source --json
```

## Find Twins of a Known Unit (QBE)

```sh
decombine query qbe --unit unit:<id> --neighbors 50 --why --json
decombine query similar --unit unit:<candidate-id> --limit 10 --json
```

(Structural-pattern and query-pack examples removed with the Phase 2/3 cut;
agents use ast-grep/ripgrep directly for those.)

## Fact Model

Before building a large DSL, expose fact tables:

```text
units
files
projects
models
vectors
similar_edges
duplicate_clusters
concern_hits
comparison_matches
captures
symbols
references
calls
artifacts
```

Initial implementation can expose only facts already available:

```text
units
files
projects
models
vectors
duplicate_clusters from latest run
comparison_matches from latest run
concern_hits from latest run
```

Use `analysis_artifacts` for structured result persistence before adding new
schema tables.

## Integration With Existing Reports

Do not replace Markdown reports. Add serializers beside them:

```text
report::markdown
report::json
report::jsonl
```

For each analyzer output:

```text
DuplicateReport -> Markdown + JSON
ConcernReport   -> Markdown + JSON
ComparisonReport -> Markdown + JSON
```

The JSON serializers should be tested with golden fixtures just like Markdown.

## Query Interface Roadmap

### Step 1: JSON for Existing Analyzers

Implement:

```text
--json (default output stays human-readable text)
--limit
stable unit IDs
stable match/cluster IDs
schema_version
```

Start with `compare`, then `duplicates`, then `concerns`.

### Step 2: Inspect and Capabilities

Implement:

```text
decombine query capabilities --json
decombine query inspect unit:<id> --json
decombine query units --where ... --json
```

This gives agents reliable orientation and source lookup.

### Step 3: Semantic Query and QBE

Implement:

```text
decombine query search --text ...
decombine query similar --unit ...
decombine query qbe --unit ...
```

Reuse `ConcernAnalyzer` query embedding and `VectorStore` top-k search.

### Step 4 (deferred): Graph Expansion

Expose `same_file`, `similar`, `duplicate`, `compare_match`, `concern_hit`
edges over data decombine already computes. Only after Steps 1–3 have real
agent usage.

### Cut: Query Packs, Structural Search, Symbol Graph

Cut 2026-07-09 (see Decisions). Revisit only on demonstrated need from agent
usage of Steps 1–3.

## Guardrails

- Default read-only.
- No rewrites in the query command family.
- Respect ignores and configured source roots.
- Cap result count, snippet length, context lines, file size, and runtime.
- Report skipped files and parse failures.
- Include index staleness.
- Include dirty-worktree/revision metadata when available.
- Do not hide non-exhaustive results.
- Do not silently fall back from semantic search to lexical search.
- Keep color and human formatting out of machine modes.
- Support `--query-file` and stdin to avoid shell-quoting pain.

## Decisions

Original synthesis (2026-07-08), revised after review (2026-07-09):

1. Build an agent-oriented interface as a typed query family, not a single DSL.
2. Add JSON for existing analyzers before building new query algorithms.
3. Make JSON/JSONL schema versions, stable IDs, `--limit`/`exhaustive`
   bounding, and explainability non-negotiable.
4. Keep human-readable text as the default output for every command (agents
   read it fine); `--json`/`--jsonl` are opt-in flags, not an output enum.
   Markdown reports stay for humans, but agents never scrape them.
5. Expose `query capabilities` and `query inspect` early; agents need
   orientation more than clever ranking.
6. Scope is the embeddings-native core only: JSON for analyzers →
   capabilities/inspect/units → similar/search/qbe. Text/AST search,
   tree-sitter query files, and YAML query packs are cut — agents already
   have ripgrep/ast-grep, and the proposed hybrid ranking weights were never
   measured. Any future hybrid retrieval is an EXPERIMENTS.md experiment
   first.
7. No cursor pagination: `--limit` plus honest `exhaustive`/`has_more`
   reporting; re-running locally is cheap.
8. Explanations are inline via `--why`, stateless. No persisted query runs,
   no `explain result:<id>`; `explain unit:<id>` is fine because units live
   in the DB.
9. Unit IDs are deterministic per index generation, not edit-surviving; no
   fuzzy `resolve` subsystem.
10. MCP is explicitly out of scope. The CLI is the agent surface; agents use
    it directly from their shell.
11. Graph expansion over existing cluster/compare/concern data is deferred,
    not cut; symbol/reference/call graph facts come later still, if ever.

## Related Files

- `RESEARCH.md`
- `EXPERIMENTS.md`
- `src/cli.rs`
- `src/main.rs`
- `src/analyze/context.rs`
- `src/analyze/vector_store.rs`
- `src/analyze/duplicate/mod.rs`
- `src/analyze/concerns/mod.rs`
- `src/analyze/compare/mod.rs`
- `src/db/migrations.rs`
- `src/report/markdown.rs`
