# Rust Replacement Architecture

## Purpose

This project is a Rust replacement for Slopo, an embedding-based detector for non-exact code duplication. It should keep the useful product shape of the upstream tool while improving the implementation where Rust gives us better options:

- local embeddings instead of LiteLLM/API providers
- generalized language support instead of one hand-written parser module per language
- a storage layer designed for incremental indexing, local model metadata, and future vector indexing
- faster clustering and analysis algorithms for larger repositories
- reusable vector-space analysis for duplicate detection, concern discovery, and project-to-project comparison

The upstream repository is AGPL-3.0-or-later. Treat it as behavioral research only. Do not copy source code into this repository unless we deliberately accept the resulting license obligations.

## Upstream Analysis

The upstream tool has three main commands after config initialization:

- `index`: scan source files, parse code units with Tree-sitter, store units in SQLite
- `embed`: batch unembedded distinct body hashes, call LiteLLM, store float32 embedding blobs
- `analyze`: compute cosine-similar pairs, cluster them, boost remote duplicates, fold exact copies, write Markdown reports

Important implementation details:

- Configuration is YAML. Required fields are `source_dir`, `embedding_model`, `embedding_dimensions`, and an API key. Defaults cover the database file, report directory, ignore file, thresholds, batch limits, and minimum AST body node count.
- SQLite schema stores metadata, files, code units, and embeddings. Embeddings are keyed by normalized body hash, so exact copies share one vector.
- Indexing is incremental by file path and `mtime`. Changed files have their prior units deleted and reinserted. Removed files are pruned, then orphaned embeddings are removed.
- Parsing supports Python, TypeScript, JavaScript, Java, Kotlin, C#, Go, and Rust. Each language module repeats the same traversal, comment stripping, body-node counting, and hash logic, but differs in Tree-sitter node names and anonymous-function name recovery.
- Similarity is exact all-pairs cosine over normalized vectors. The upstream implementation computes the matrix in row blocks to cap intermediate memory.
- Clustering uses transitive connected components: if A is similar to B and B is similar to C, all three are one cluster.
- Reranking boosts pairs far apart in the repository: cross-directory distance can add up to 15 percent and same-file line distance can add up to 10 percent.
- Reports are Markdown: one `index.md`, one file per cluster, stable cluster hashes for `slopo.ignore.txt`, and exact-copy folding so identical snippets are displayed once with multiple locations.

Limitations to address:

- Parser logic is duplicated across languages.
- Cluster merging is a simple list-of-sets algorithm; use union-find instead.
- Local nested-unit overlap is line-based; byte ranges are more precise.
- Embedding depends on external network APIs and API keys.
- Embedding model identity is under-specified for reproducible local runs: a local tool should persist model revision, backend, tokenizer/model file identity, and dimensions.
- Exact all-pairs cosine is fine for modest projects, but needs a clear extension point for approximate nearest-neighbor indexing later.

## Recommended Shape

Use a single Rust workspace with a binary crate and internal modules:

```text
src/
  main.rs
  cli.rs
  config.rs
  db/
    mod.rs
    migrations.rs
    models.rs
  index/
    scanner.rs
    language.rs
    extractor.rs
    normalizer.rs
  embed/
    mod.rs
    fastembed.rs
    custom_onnx.rs
    model_cache.rs
  analyze/
    mod.rs
    context.rs
    vector_store.rs
    paths.rs
    artifacts.rs
    duplicate/
      mod.rs
      similarity.rs
      clustering.rs
      rerank.rs
      dedup.rs
      cross_directory.rs
      ignore.rs
    concerns/
      mod.rs
      projection.rs
      spread.rs
      report_model.rs
    compare/
      mod.rs
      matching.rs
      coverage.rs
      grouping.rs
      report_model.rs
    experimental/
      subspace.rs
      spectra.rs
      factor.rs
      spectral.rs
      alignment.rs
  report/
    markdown.rs
    filesystem.rs
assets/
  languages/
    rust.toml
    rust/units.scm
    python.toml
    python/units.scm
```

The CLI should expose these commands:

- `init`
- `show-config`
- `index`
- `embed`
- `analyze`
- `analyze duplicates`
- `analyze concerns`
- `compare`
- `run` as a convenience pipeline for `index`, `embed`, `analyze`
- `languages list`
- `models list`
- `models download`
- `doctor` for parser/model/storage diagnostics

## Configuration

Initial config fields:

```yaml
source_dir:
source_dir_exclude: []
db_file: decombine.db
report_dir: decombine-report
ignore_file: .decombineignore

# Optional multi-project form. If present, this replaces source_dir.
projects:
  - label: main
    source_dir:
    source_dir_exclude: []
  - label: rewrite
    source_dir:
    source_dir_exclude: []

languages:
  enabled: ["rust", "python", "typescript", "javascript", "java", "kotlin", "csharp", "go"]

embedding:
  backend: fastembed
  model: CodeRankEmbed # managed: downloaded + hash-verified on first use
  cache_dir:
  batch_size: 256
  max_batch_chars: 200000
  max_body_chars: 10000
  pending_page_size: 512
  normalize: true
  execution_provider: cpu
  quantized: false

index:
  retention: report # full | report | minimal

analysis:
  candidate_threshold: 0.70 # CodeRankEmbed scale; BGE used 0.88/0.92/0.94
  similarity_threshold: 0.81
  rerank_threshold: 0.85
  block_size: 1000
  body_node_count_threshold: 10
  min_semantic_body_node_count: 20
  max_edges_per_unit: 5
  max_cluster_size: 100
  concerns:
    enabled: false
    min_projection: 0.45
    top_units_per_concern: 50
    queries: []

comparison:
  left:
  right:
  candidate_threshold: 0.78
  match_threshold: 0.86
  top_k_per_unit: 5
  use_name_hints: true
  use_path_hints: true
```

For the simple one-project workflow, `source_dir` creates an implicit project labeled `main`. For comparison workflows, `projects` names multiple roots in the same database. Persist immutable indexing and embedding settings in the database metadata table. Reject incompatible changes unless the user requests a full reindex or migration.

Retention controls how much source text is stored in the local database:

- `full`: store display source and embedding text for debugging and reproducibility.
- `report`: store only the report/display source plus hashes and ranges.
- `minimal`: store hashes and byte/line ranges only; reports reread source files and may degrade if files change.

## Language Support

Use Tree-sitter, but move language-specific extraction into data files instead of Rust code.

Recommended design:

- A `LanguageSpec` TOML file maps file extensions, language id, parser source, comment node kinds, string/doc-comment rules, and extraction query path.
- A generic extractor parses source once, runs a Tree-sitter query, and emits `CodeUnit` records from captures.
- A small `LanguageAdapter` hook layer handles cases that queries cannot express cleanly: anonymous-function names, receiver names, decorator/annotation filtering, docstring rules, macros, preprocessor regions, and scope recovery.
- Each supported language contributes one `units.scm` query file rather than one Rust module.
- Adding a language should usually mean adding a parser dependency plus `assets/languages/<id>.toml` and `assets/languages/<id>/units.scm`.
- Keep language fixtures and golden tests mandatory. Language support is only accepted when names, ranges, body text, comment stripping, nested units, and body-node thresholds are validated.

Query capture convention:

```scheme
; Example shape, exact node names differ by language.
(
  (function_item
    name: (identifier) @unit.name
    body: (block) @unit.body) @unit
  (#set! unit.kind "function")
)
```

Supported captures:

- `@unit`: full syntactic unit range
- `@unit.name`: display name
- `@unit.body`: range used for complexity counting and optional body-only embedding
- `@unit.strip`: comments, docstrings, annotations, or other ranges removed before hashing/embedding
- `@unit.scope`: optional class/module/receiver scope for display
- `#set! unit.kind`: function, method, closure, lambda, constructor, macro, etc.

Extraction algorithm:

1. Resolve language by extension and optional first-line/content checks.
2. Parse source with the configured Tree-sitter grammar.
3. Run the language's `units.scm` query.
4. For each match containing `@unit`, collect related captures from that match.
5. Apply the language adapter, if present, to refine names, scopes, strip ranges, and language-specific metadata.
6. Build display metadata from name, kind, language, byte range, line range, and optional scope.
7. Strip all `@unit.strip` ranges and adapter-provided ranges from the embedding text.
8. Count named AST nodes under `@unit.body` when present, otherwise under `@unit`.
9. Hash normalized embedding text and source text separately.
10. Drop units below the complexity threshold or above the body character limit.

Parser loading options:

- Version 1 should compile a curated set of Tree-sitter parser crates into the binary for predictable installation.
- A later plugin mode can load grammar WASM files using Tree-sitter's `wasm` feature or use `tree-sitter-loader` for runtime grammars. This is useful for long-tail languages but adds packaging, cache, and trust concerns.
- `tree-sitter-tags` can be evaluated as a bootstrap source for definition queries, but it does not by itself solve our body extraction and duplicate-detection needs.

This makes language expansion mostly declarative, but not free. Tree-sitter node names still vary, anonymous/lambda naming is language-specific, and some languages need doc-comment, macro, decorator, or preprocessor handling. Keep adapters small and test-driven; if an adapter starts doing traversal that belongs in a query, push that logic back into `units.scm`.

## Embeddings

Use local embeddings by default.

Recommended path:

- Start with `fastembed` for the default backend. It provides local ONNX inference, text embedding APIs, model download/cache behavior, tokenization, and ONNX Runtime integration in one crate.
- Benchmark the first default against at least `BGESmallENV15`, `BGEBaseENV15`, and `JinaEmbeddingsV2BaseCode`; fast first-run behavior and code-duplication quality should both be measured.
- Add a `custom_onnx` backend behind a trait only after `fastembed` cannot represent a model we need. This is milestone 2, not part of the critical path.
- Use `hf-hub` when we need explicit Hugging Face repository download/cache control.
- Use `ort` directly only for custom ONNX models where `fastembed` cannot represent the tokenizer, pooling, output selection, or execution provider we need.

Trait boundary:

```rust
trait Embedder {
    fn model_id(&self) -> ModelIdentity;
    fn dimensions(&self) -> usize;
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}
```

Persist `ModelIdentity` with at least:

- backend name and version
- embedding backend crate version
- ONNX Runtime or `ort` version when applicable
- model name or Hugging Face repo id
- revision or resolved commit when available
- tokenizer file hash
- ONNX/model file hash
- output dimensions
- pooling/normalization mode
- execution provider, such as CPU, CUDA, CoreML, DirectML, or OpenVINO
- quantization mode
- effective cache path and whether `embedding.cache_dir`, `FASTEMBED_CACHE_DIR`, or the OS cache default selected it

Default model choice is still an open product decision. General text embedding models are easy to run locally; code-focused models may produce better duplicate-detection quality. Because `fastembed` already supports some code-oriented models, the first milestone should stay on `fastembed` and benchmark before adding custom ONNX plumbing.

## Storage Options

The core workload is relational:

- projects have labels and source roots
- files have project-relative paths, mtimes, source hashes, and languages
- code units belong to files
- embeddings belong to normalized body hashes and model identities
- analysis needs joins from embeddings to units and files
- report generation needs stable ordering and metadata

Recommendation: use SQLite via `rusqlite` with bundled SQLite.

Reasons:

- The data model is naturally relational and benefits from joins, foreign keys, uniqueness constraints, transactions, and migrations.
- SQLite is a single local file and is familiar to users of CLI tools.
- `rusqlite` is synchronous and ergonomic, which matches a CLI workload without requiring an async runtime for storage.
- The bundled feature avoids depending on the system SQLite version.
- We can add vector-search extensions or sidecar indexes later without changing the primary store.

Recommended schema additions beyond upstream, with constraints made explicit:

```sql
metadata(
  id INTEGER PRIMARY KEY CHECK (id = 1),
  schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL
)
settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)
projects(
  id INTEGER PRIMARY KEY,
  label TEXT NOT NULL UNIQUE,
  source_dir TEXT NOT NULL,
  role TEXT,
  created_at TEXT NOT NULL
)
files(
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  relative_path TEXT NOT NULL,
  language_id TEXT NOT NULL,
  mtime_ns INTEGER NOT NULL,
  size INTEGER NOT NULL,
  source_hash TEXT NOT NULL,
  UNIQUE (project_id, relative_path)
)
code_units(
  id INTEGER PRIMARY KEY,
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  language_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  scope TEXT,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  body_node_count INTEGER NOT NULL,
  source_hash TEXT NOT NULL,
  normalized_body_hash TEXT NOT NULL,
  display_source TEXT,
  embedding_text TEXT,
  CHECK (start_byte < end_byte),
  CHECK (start_line <= end_line)
)
embedding_models(
  id INTEGER PRIMARY KEY,
  backend TEXT NOT NULL,
  backend_version TEXT NOT NULL,
  model TEXT NOT NULL,
  revision TEXT,
  dimensions INTEGER NOT NULL,
  tokenizer_hash TEXT,
  model_hash TEXT,
  normalize INTEGER NOT NULL,
  execution_provider TEXT NOT NULL,
  quantization TEXT,
  cache_path TEXT,
  UNIQUE (
    backend, backend_version, model, revision, dimensions,
    tokenizer_hash, model_hash, normalize, execution_provider, quantization
  )
)
embeddings(
  model_id INTEGER NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
  normalized_body_hash TEXT NOT NULL,
  vector_blob BLOB NOT NULL,
  norm REAL NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (model_id, normalized_body_hash)
)
analysis_runs(
  id INTEGER PRIMARY KEY,
  analysis_kind TEXT NOT NULL,
  model_id INTEGER NOT NULL REFERENCES embedding_models(id),
  project_scope_json TEXT NOT NULL,
  config_json TEXT NOT NULL,
  created_at TEXT NOT NULL
)
analysis_artifacts(
  id INTEGER PRIMARY KEY,
  run_id INTEGER NOT NULL REFERENCES analysis_runs(id) ON DELETE CASCADE,
  artifact_kind TEXT NOT NULL,
  method TEXT NOT NULL,
  params_json TEXT NOT NULL,
  metrics_json TEXT NOT NULL,
  blob BLOB,
  created_at TEXT NOT NULL
)
```

Indexes:

- `projects(label)`
- `files(project_id, relative_path)`
- `files(source_hash)`
- `code_units(file_id)`
- `code_units(normalized_body_hash)`
- `code_units(file_id, start_byte, end_byte)`
- `embeddings(model_id, normalized_body_hash)` is covered by the primary key

Storage alternatives:

| Option | Fit | Notes |
| --- | --- | --- |
| SQLite + `rusqlite` | Best default | Strong relational fit, simple binary distribution with bundled SQLite, good migrations, easy BLOB storage. |
| SQLite + `sqlx` | Possible | Compile-time query checking is attractive, but async and migration workflow are heavier for a local CLI. Current `sqlx` has a high Rust version requirement. |
| `redb` | Good KV store, not default | Pure Rust, ACID, MVCC, crash-safe. Useful if we design around key-value tables, but we would rebuild relational joins and constraints ourselves. |
| `fjall` | Good write-heavy KV/LSM store, not default | Pure Rust LSM with keyspaces, compression, and range/prefix scans. Better for large log-structured KV workloads than this relational metadata workload. |
| RocksDB | Powerful, not default | Mature LSM and column families, but C++ dependency and more operational weight than needed. |
| `heed`/LMDB | Possible | Fast embedded B-tree KV with typed wrapper, but still requires manual indexing and map-size planning. |
| `sled` | Avoid for default | Rust-native, but project status and alpha/current version make it a weaker conservative choice. |
| SQLite `vec1` | Future vector extension | Official SQLite ANN extension with cosine/L2 support. Watch maturity, distribution, and extension-loading story before adopting. |
| `sqlite-vec` / `sqlite-vector-rs` | Future vector extension | Promising for local vector search inside SQLite, but pre-v1 or young enough to treat as optional. |
| `hnsw_rs` sidecar | Future ANN index | Useful if exact all-pairs cosine becomes too slow. Rebuild from SQLite embeddings initially. |
| Tantivy | Not core storage | Excellent full-text search crate. Consider only if we add lexical search or source browsing. |
| Qdrant Edge | Future evaluation | In-process vector search is a good conceptual fit, but it is beta and should be evaluated for license, distribution, and API stability. |

## Analysis Architecture

Keep indexing and embedding independent from the question being asked. The analysis layer should load a reusable `AnalysisContext`, then pass it to focused analyzers.

Core shared types:

```rust
struct AnalysisContext {
    model_id: ModelId,
    projects: Vec<ProjectRef>,
    units: Vec<CodeUnitRef>,
    vectors: VectorStore,
}

trait Analyzer {
    type Config;
    type Output;

    fn run(&self, ctx: &AnalysisContext, config: &Self::Config) -> Result<Self::Output>;
}
```

`VectorStore` owns normalized vectors and shared operations:

- block-wise dot products
- top-k nearest-neighbor search
- sparse candidate edge generation
- vector lookup by code unit id or normalized body hash
- streaming covariance accumulation for experimental subspace analysis, using `C = X^T X` rather than materializing an `n x n` Gram matrix
- optional future dispatch to exact flat search or ANN indexes

`paths.rs` owns reusable structural metrics:

- same-file byte overlap checks
- same-file line distance
- cross-directory path distance
- module/path entropy
- project-relative path normalization

This structure keeps the math reusable without making duplicate detection depend on every future analysis experiment.

## Duplicate Analysis Pipeline

Version 1 should keep exact cosine search because it is deterministic and easier to validate:

1. Load an `AnalysisContext` for one project.
2. Compute block-wise cosine similarities with `ndarray` or a small custom dot-product kernel.
3. Emit candidate pairs at or above `candidate_threshold`.
4. Exclude pairs whose source byte ranges overlap in the same file.
5. Rerank candidate pairs using path distance or same-file distance.
6. Drop reranked pairs below `rerank_threshold` and raw pairs below `similarity_threshold` unless the distance boost makes them actionable.
7. Use union-find to build connected components from the surviving pairs.
8. Fold exact duplicates by normalized body hash.
9. Derive cross-directory duplication candidates from surviving clusters.
10. Apply ignored cluster hashes.
11. Write Markdown reports.

Cross-directory duplication is the first concern-like signal that should ship. It does not require PCA or other latent-basis assumptions. A candidate should pass all of these gates:

- cluster evidence: the unit belongs to a real duplicate/similar-code cluster rather than only being weakly near many unrelated units
- geographic dispersion: semantic neighbors or cluster members span distant paths or multiple top-level modules
- kind/scope gating: generic free functions in `utils`, `common`, or similar shared scopes are downranked, while similar methods on different receiver/class scopes are emphasized

The report should name this section "Cross-directory duplication" rather than "cross-cutting concerns." The latter is a hypothesis for human review; the former is directly supported by the evidence.

Keep `candidate_threshold` lower than `similarity_threshold` so distance-aware reranking can surface far-apart code that would otherwise be discarded before rerank. Benchmarks should tune the gap; the default should prefer manageable candidate counts over maximum recall until quality data exists.

Future scaling path:

- Add a `SimilarityIndex` trait with `ExactFlat` and `AnnHnsw` implementations.
- Store ANN index metadata separately from source metadata.
- Rebuild ANN indexes from SQLite embeddings until incremental ANN deletion/update behavior is proven reliable.
- Do not enable ANN by default until benchmark data identifies a concrete trigger point, such as indexed unit count, exact-search wall time, or peak memory.

## Concern Analysis Pipeline

Concern analysis asks which semantic directions are present in a project and whether they are structurally scattered. It should be a post-MVP analyzer built on the same indexed units and embeddings. The duplicate analyzer's cross-directory duplication section is the baseline; query projection is the first opt-in concern analyzer.

The first concern analyzer should use projection lenses:

1. Load an `AnalysisContext` for one project.
2. Embed configured concern query strings with the same model identity.
3. Normalize query vectors.
4. Score every code unit with `dot(concern, unit)`.
5. Keep top units per concern above `min_projection`.
6. Compute structural spread using directory entropy and path distance.
7. Write concern reports that present candidates, evidence, and examples.

The report should call these "candidate concerns"; embeddings provide evidence, not proof. Projection should be evaluated against the simpler cross-directory duplication baseline before it becomes a default report section.

Future concern experiments:

- learn repo-specific semantic bases with uncentered PCA/SVD over `C = X^T X`
- use NMF on non-negative identifier/token matrices for more interpretable additive concern parts
- use graph Laplacian eigenvectors to split large semantic components or treat concern scores as signals on the repository graph

These belong under `analyze/experimental/` until benchmark data shows clear product value.

Experimental subspace rules:

- Do not materialize the full `G = X X^T` Gram matrix for large repositories; stream code-unit embeddings and accumulate the small `d x d` covariance-like matrix `C = X^T X`.
- Keep uncentered PCA as the default because centering normalized embeddings changes cosine geometry. If centering is added, it must be an explicit experimental flag.
- Treat the first few uncentered axes as likely corpus mean, boilerplate, or embedding anisotropy directions. Drop a configurable top-m common directions before using residual axes for concept experiments.
- In multi-language projects, compute subspaces per language, per subsystem, or after language effects are controlled. Otherwise the strongest axes may separate languages rather than concerns.
- Do not expose unlabeled "axis N" findings as product output. Axes need labels and benchmark evidence before becoming user-facing.

## Project Comparison Pipeline

Project comparison compares two indexed projects in the same database. The target use cases are:

- a v2 rewrite compared with a v1 codebase
- two independent implementations of the same system
- cross-language ports, such as Python versus Rust or JavaScript versus Go
- checking whether a migration preserved semantic coverage

The comparison should be asymmetric by default: `left` is the reference project and `right` is the candidate implementation. Reports should answer:

- which left-side functions appear covered by right-side code?
- which left-side behavior has no strong match?
- which right-side units look new or extra?
- where did one old unit split into several new units?
- where did several old units merge into one new unit?
- which matches are semantically strong but structurally surprising?
- which areas deserve human review before declaring parity?

Version 1 comparison algorithm:

1. Load an `AnalysisContext` containing exactly two projects.
2. Partition units into left and right sets.
3. Generate cross-project candidate edges only; do not compare units within the same project.
4. Score edges by embedding cosine.
5. Optionally add small, bounded hints for name similarity and path similarity. These hints should never rescue a semantically weak edge by themselves.
6. Keep top-k right candidates for each left unit and top-k left candidates for each right unit.
7. Mark mutual nearest neighbors as strong one-to-one matches when they exceed `match_threshold`.
8. Build bipartite connected components from remaining candidate edges to detect one-to-many, many-to-one, and many-to-many rewrite relationships.
9. Classify unmatched left units as possible missing coverage and unmatched right units as possible new behavior.
10. Aggregate coverage by directory/module and language.
11. Write a comparison report with match evidence, unmatched lists, split/merge groups, and summary metrics.

Recommended match classes:

| Class | Meaning |
| --- | --- |
| `exact_copy` | Same normalized body hash appears in both projects. |
| `strong_match` | Mutual or near-mutual semantic match above threshold. |
| `possible_match` | Candidate edge above lower threshold but not strong enough for coverage. |
| `split` | One left unit maps to multiple right units. |
| `merge` | Multiple left units map to one right unit. |
| `missing_in_right` | Left unit has no adequate right-side candidate. |
| `new_in_right` | Right unit has no adequate left-side candidate. |

Cross-language comparison depends heavily on the embedding model. The benchmark suite must include cross-language fixtures before claiming this works well. Reports should expose model identity and avoid implying semantic equivalence from path/name similarity alone.

Comparison output should be useful to a human or AI reviewer, not a binary pass/fail gate. The tool can say "this v1 function's closest v2 candidates are these three units with these scores"; it should not claim behavioral equivalence without tests or formal evidence.

Future comparison experiments:

- use an assignment algorithm for stricter one-to-one coverage when desired
- compare concern projection profiles between projects
- learn alignment transforms between embedding models with Orthogonal Procrustes when the same anchor units exist under multiple model versions
- use CCA canonical correlations or principal angles when comparing two embedding spaces; these are more coordinate-invariant than a determinant of a fitted transform
- inspect singular values, rank, condition number, and residual error for model-to-model transforms; determinant alone is ill-posed or too coarse for cross-model comparison
- compare two snapshots of the same project with cluster membership diffs and comparison classes before reaching for linear-algebra diagnostics
- compare project-level semantic bases to identify dimensions that disappeared, appeared, or changed emphasis

## Reporting

Keep Markdown reports for version 1:

- `index.md` summary table
- `cluster-N.md` detail files
- `concerns/index.md` and one file per concern when concern analysis is enabled
- `compare/index.md` and detail files for project comparisons
- stable cluster hashes for ignore workflow
- exact-copy folding
- language-tagged fenced code blocks

Improvements:

- Include model identity and thresholds in `index.md`.
- Include cluster reasons: top pair score, path-distance boost, exact-copy count.
- Include cross-directory duplication reasons: cluster span, representative bridge unit, path dispersion, and scope/receiver evidence.
- Include concern reasons: top projection score, structural spread, and representative units.
- Include comparison reasons: top match score, match class, name/path hints, split/merge classification, and unmatched coverage.
- Include copyable ignore commands or hash-only block.
- Include the configured retention mode so users know whether report source came from the database or from rereading files.
- Make output deterministic for tests.

Agent-facing machine output (shipped 2026-07-09, see
`docs/research/agent-query-interface.md`): every analyzer gains `--json`
(bounded envelope on stdout, progress on stderr) with `--limit` plus honest
`exhaustive`/`has_more` reporting, serialized in `src/report/json.rs` beside
the markdown renderer. `src/query/` adds the `decombine query` family —
`capabilities`, `inspect`, `units` (with `--where` metadata filters),
`similar`, `search`, and `qbe` — over stable per-index-generation
`unit:<hash>` selectors shared by all machine surfaces.

## Dependencies To Start

Likely initial crates:

- `clap` for CLI
- `serde`, `serde_yaml`, `toml`
- `anyhow`, `thiserror`
- `rusqlite` with `bundled`
- `rusqlite_migration` or a small `PRAGMA user_version` migration module
- `ignore` or `globset` for repo scanning and gitignore-style excludes
- `tree-sitter` plus curated `tree-sitter-*` language crates
- `fastembed`
- `hf-hub` for explicit model downloads when needed
- `ort` behind a feature for custom ONNX backend
- `ndarray` and optionally `rayon`
- a pure-Rust small dense eigensolver such as `nalgebra` or `faer` only when experimental subspace analysis graduates into implementation
- `sha2`
- `camino` for UTF-8 paths, if path handling becomes noisy

## Research Notes

- `fastembed` docs state it provides local ONNX inference, synchronous embedding APIs, and model download/cache behavior.
- `hf-hub` is the Rust Hugging Face Hub client and supports file download plus cache-related environment variables.
- `ort` is the Rust ONNX Runtime wrapper.
- Tree-sitter Rust bindings support parser crates and optional WASM grammar loading.
- Tree-sitter query files are a standard way to match syntax tree patterns with captures and predicates.
- `rusqlite` is the ergonomic Rust wrapper for SQLite; `sqlx` can statically link bundled SQLite too.
- `redb` and `fjall` are strong Rust-native embedded KV stores but do not match the relational core as well as SQLite.
- `sqlite-vec` is promising but pre-v1.
- SQLite `vec1` provides ANN vector search with cosine/L2 support and should be evaluated as it matures.
- Latent Semantic Analysis and Latent Semantic Indexing are established SVD-based ways to derive latent semantic spaces from source-code artifacts and vocabulary.
- For embedding matrices with many code units and modest dimensions, the practical PCA route is the small `C = X^T X` matrix, not the full `G = X X^T` Gram matrix.
- PCA over embeddings should account for anisotropy, boilerplate/common directions, and language-separation axes before interpreting residual directions as possible concerns.
- Spectral clustering uses eigenvectors of matrices derived from pairwise data; this is a plausible future alternative to threshold-only connected components.
- Dense graph Laplacians have the same scaling problem as dense Gram matrices; spectral experiments should use sparse k-nearest-neighbor graphs, Nyström landmarks, or another bounded-memory method.
- Non-negative matrix factorization is useful as an interpretability candidate because additive parts are easier to label than PCA/SVD components with cancellations.
- Orthogonal Procrustes alignment is the right starting point for comparing embedding spaces when the same anchor units exist under two model versions.
- CCA canonical correlations and principal angles are better diagnostics than determinant for comparing different embedding spaces.

## Sources

- Upstream Slopo repository: https://github.com/rafal-qa/slopo
- `fastembed`: https://docs.rs/fastembed
- `hf-hub`: https://github.com/huggingface/hf-hub
- `ort`: https://docs.rs/ort
- Tree-sitter Rust bindings: https://docs.rs/tree-sitter
- Tree-sitter query syntax: https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html
- Tree-sitter query files and language config: https://tree-sitter.github.io/tree-sitter/3-syntax-highlighting.html
- `rusqlite`: https://docs.rs/rusqlite
- `sqlx` SQLite static linking: https://docs.rs/sqlx/latest/sqlx/sqlite/index.html
- `redb`: https://docs.rs/redb
- `fjall`: https://docs.rs/fjall
- `sqlite-vec`: https://github.com/asg017/sqlite-vec
- SQLite `vec1`: https://sqlite.org/vec1
- Qdrant Edge: https://qdrant.tech/documentation/edge/
- Tantivy: https://github.com/quickwit-oss/tantivy
- `hnsw_rs`: https://github.com/jean-pierreBoth/hnswlib-rs
- Source-code LSA: https://www.cs.kent.edu/~jmaletic/papers/ICTAI00.pdf
- Semantic clustering with LSI: https://research.cs.queensu.ca/home/ahmed/home/teaching/CISC880/F11/papers/SemanticClustering_IST2007.pdf
- Spectral clustering: https://papers.nips.cc/paper/2092-on-spectral-clustering-analysis-and-an-algorithm
- Non-negative matrix factorization: https://www.cs.columbia.edu/~blei/fogm/2020F/readings/LeeSeung1999.pdf
- Embedding-space Procrustes alignment: https://arxiv.org/html/2510.13406v1
