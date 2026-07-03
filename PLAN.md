# Implementation Plan

## Current Decisions

- Build a Rust replacement, not a line-by-line port.
- Use local embeddings. Start with `fastembed`; keep a backend trait for custom ONNX models via `hf-hub` and `ort`.
- Use SQLite through `rusqlite` as the primary on-disk store.
- Generalize language support with Tree-sitter query files and language metadata, plus small language adapter hooks for cases queries cannot express cleanly.
- Keep Markdown reports and ignore-file workflow for version 1.
- Keep exact all-pairs cosine for version 1, with an ANN/vector-index extension point gated by benchmark evidence.
- Structure analysis as a reusable `AnalysisContext` plus focused analyzers, starting with duplicate detection and leaving concern/project-comparison analysis as separate lanes.
- Store indexed roots as `projects` internally. The single-project `source_dir` config remains the simple path, but comparison workflows use labeled project roots in one database.

## Open Decisions

- Project/binary name.
- Repository license. Because upstream is AGPL, avoid copying implementation code unless we intentionally choose a compatible license path.
- Default embedding model. `fastembed` gives us reliable local model execution, but we still need quality benchmarks across `BGESmallENV15`, `BGEBaseENV15`, and `JinaEmbeddingsV2BaseCode`.
- First language set. The practical default is upstream parity: Rust, Python, TypeScript, JavaScript, Java, Kotlin, C#, Go.
- Runtime grammar plugins. Version 1 should bundle parser crates; WASM or `tree-sitter-loader` can come later.
- Source retention default. Supported modes should be `full`, `report`, and `minimal`; the provisional default is `report`.
- Multi-project comparison defaults: thresholds, whether name/path hints should be enabled by default, and how strict coverage classification should be.
- Cross-language comparison model quality. We should not advertise strong cross-language parity checks until benchmarks include known ports or rewrites.

## Phase 0: Repository Setup

Tasks:

- Create a Rust workspace and binary crate.
- Add baseline dependencies: CLI, config parsing, errors, SQLite, hashing, path scanning, Tree-sitter, tests.
- Add CI commands locally documented in `README.md`.
- Add license once chosen.
- Keep `_upstream/slopo` as ignored research material.

Acceptance:

- `cargo test`, `cargo fmt --check`, and `cargo clippy` run on an empty skeleton.
- `decombine --help` shows planned top-level commands.

## Phase 1: Config and CLI

Tasks:

- Implement `init`, `show-config`, `doctor`.
- Parse YAML config with defaults.
- Validate either single-project `source_dir` or multi-project `projects`, but not an ambiguous mixture.
- Validate source directories, project labels, threshold ranges, batch sizes, and model config.
- Validate source-retention mode, execution provider, quantization flag, and the relationship between candidate/similarity/rerank thresholds.
- Validate concern-analysis config if present: unique names, non-empty query text, projection threshold, and top-N limits.
- Validate comparison config if present: left/right labels exist, labels differ, thresholds are ordered, and top-k is bounded.
- Add `.env` support only if needed for private Hugging Face tokens; no API key should be required.

Acceptance:

- Golden config template test.
- Invalid config tests for missing source dir, conflicting `source_dir`/`projects`, duplicate project labels, bad thresholds, unknown language id, bad retention mode, bad comparison labels, and incompatible embedding config.

## Phase 2: SQLite Store

Tasks:

- Implement `db::open_or_create`.
- Add migrations using `PRAGMA user_version` or `rusqlite_migration`.
- Create tables for metadata, projects, files, code units, embedding models, embeddings, analysis runs, and generic analysis artifacts.
- Add primary keys, foreign keys, cascades, uniqueness constraints, and CHECK constraints from the architecture schema.
- Add immutable-setting checks for project roots, body threshold, embedding backend/model/dimensions, and normalization.
- Persist retention mode and expanded model identity fields, including backend crate version, execution provider, quantization, model/tokenizer hashes, and effective cache path when known.
- Add typed read/write APIs. Avoid spreading raw SQL across the codebase.

Acceptance:

- Migration tests from empty DB.
- Insert/update/delete project tests.
- Insert/update/delete file tests.
- File path uniqueness is scoped by project label/root, not global path text.
- Embedding deduplication by `(model_id, normalized_body_hash)`.
- Constraint tests for duplicate file paths, invalid ranges, and embedding model uniqueness.
- Orphan cleanup tests.

## Phase 3: Generic Tree-Sitter Extraction

Tasks:

- Define `LanguageSpec`.
- Define a minimal `LanguageAdapter` trait for name/scope refinement, extra strip ranges, decorators/annotations, docstrings, macros, and preprocessor handling.
- Implement language registry loading from bundled assets.
- Implement generic extractor over `tree_sitter::Query`.
- Implement captures: `@unit`, `@unit.name`, `@unit.body`, `@unit.strip`, `@unit.scope`, and `unit.kind`.
- Apply adapter hooks after query matching and before normalization.
- Normalize body text for hashing.
- Count named body nodes.
- Add exact byte and line ranges.

Initial language work:

- Build Rust and Python specs first to validate the abstraction.
- Add TypeScript/JavaScript next because anonymous function naming stresses the query/adaptor boundary.
- Add Java/Kotlin/C#/Go after extractor behavior is stable.

Acceptance:

- Golden fixtures per language for functions, methods, closures/lambdas, nested units, comments/docstrings, and small-body filtering.
- Adding a new language usually requires no Rust source changes beyond registering a parser crate or feature; adapter changes are allowed only when fixtures prove query-only extraction is insufficient.

## Phase 4: Incremental Indexing

Tasks:

- Implement scanner with gitignore-style excludes.
- Resolve enabled languages by extension and optional first-line/content checks.
- Track `mtime_ns`, file size, and source hash.
- Reparse files when timestamp/size/hash changes.
- Delete removed file records and prune orphaned embeddings.
- Support indexing one implicit `main` project or multiple labeled projects.
- Report indexed, skipped, removed, and failed files by project label.

Acceptance:

- Re-index unchanged tree skips files.
- Modified file replaces units.
- Deleted file removes units and orphan embeddings.
- Two projects can contain the same relative path without colliding.
- Exclude patterns work.

## Phase 5: Local Embeddings

Tasks:

- Implement `Embedder` trait.
- Implement `fastembed` backend.
- Persist model identity and dimensions.
- Persist backend crate version, `ort`/ONNX Runtime version when applicable, execution provider, quantization mode, model/tokenizer hashes, and effective cache path.
- Add model cache config and `models download`.
- Batch by item count and total character count.
- Store normalized `f32` vectors as little-endian blobs.
- Benchmark `BGESmallENV15`, `BGEBaseENV15`, and `JinaEmbeddingsV2BaseCode` before locking a default.
- Add custom ONNX spike behind a feature using `hf-hub`, `tokenizers`, and `ort` only after fastembed-supported models fail the benchmark target.

Acceptance:

- `embed` downloads/loads a local model without API credentials.
- Distinct identical bodies embed once.
- Resume behavior skips already embedded body hashes.
- Model mismatch produces a clear error.
- A small fixture repo can index and embed offline after first model download.
- Default-model benchmark notes document quality, runtime, model size, and offline behavior.

## Phase 6: Duplicate Analysis

Tasks:

- Implement `AnalysisContext` and `VectorStore` for loading selected projects, code units, metadata, and normalized vectors.
- Implement shared path metrics for byte overlap, same-file distance, cross-directory distance, and project-relative paths.
- Implement exact flat cosine search in blocks.
- Emit pairs from a lower `candidate_threshold` before reranking.
- Add optional Rayon parallelism after deterministic tests exist.
- Exclude overlapping same-file byte ranges.
- Rerank candidates before final pair filtering so distance boost can rescue far-apart near-misses.
- Build clusters with union-find.
- Rerank by cross-directory path hops and same-file distance.
- Filter by candidate, raw similarity, and rerank thresholds according to the architecture pipeline.
- Fold exact duplicates.
- Derive cross-directory duplication candidates from clusters whose members span distant paths or multiple top-level modules.
- Compute per-function or per-cluster dispersion from nearest-neighbor path distances; do not assume pair rerank scores are already a per-function metric.
- Downrank generic shared-scope helpers such as `utils`/`common`, and emphasize repeated method logic across different receiver/class scopes.
- Apply ignore-file hashes.

Acceptance:

- `AnalysisContext` tests cover single-project loading, selected-project filtering, sparse unit IDs, and missing embeddings.
- Cosine tests cover normalization, sparse unit IDs, thresholds, sorting, and no self-pairs.
- Rerank tests cover a pair below raw similarity threshold but above final threshold after distance boost.
- Clustering tests cover transitive chains, disconnected components, stable ordering, and reranked ordering.
- Overlap tests use byte ranges, not just lines.
- Cross-directory duplication tests cover multi-module clusters, local-only clusters, generic helper downranking, and repeated method logic across different scopes.
- Reports are deterministic.

## Phase 6b: Concern Signals and Projection Analysis

Tasks:

- Treat Phase 6 cross-directory duplication as the baseline concern-like signal.
- Add `analyze concerns` as a separate analyzer built on `AnalysisContext` for explicit query projection.
- Embed named concern queries with the same selected model identity used for code units.
- Compute projection scores with normalized dot products.
- Keep top units per concern above `min_projection`.
- Compute structural spread using module/path entropy and path distance.
- Produce a typed concern report model with candidate evidence, representative units, and spread metrics.
- Compare projection findings against the cross-directory duplication baseline before enabling them by default.
- Keep SVD, NMF, graph Laplacian, and basis-learning experiments out of the default analyzer.

Acceptance:

- Query embeddings are associated with the selected model identity.
- Projection scoring tests use deterministic fixture vectors.
- Concern names are stable and sorted deterministically.
- Spread metrics cover same-file, same-directory, and cross-directory examples.
- Fixtures include generic high-dispersion helpers that should not become high-confidence concerns without additional evidence.
- Reports label results as candidate concerns, not confirmed facts.

## Phase 6c: Project Comparison Analysis

Tasks:

- Add `compare` as a separate analyzer requiring exactly two project labels.
- Load a two-project `AnalysisContext` and partition units into left/reference and right/candidate sets.
- Generate only cross-project candidate edges.
- Score candidates by embedding cosine, with optional bounded name/path hints.
- Keep top-k candidates in both directions.
- Classify exact copies, strong matches, possible matches, splits, merges, missing-in-right, and new-in-right.
- Aggregate semantic coverage by directory/module and language.
- Produce a typed comparison report model with match evidence and unmatched units.

Acceptance:

- Comparison rejects missing labels, identical labels, or one-project contexts.
- Cross-project search never emits same-project pairs.
- Exact body-hash matches are classified separately from semantic matches.
- Mutual-nearest fixtures produce stable one-to-one matches.
- One-to-many and many-to-one fixtures classify as split and merge.
- Unmatched left units are reported as possible missing coverage; unmatched right units are reported as possible new behavior.
- Name/path hints cannot raise a semantically weak edge above the strong-match threshold by themselves.
- Cross-language fixture tests document current model quality before the feature is advertised as reliable for ports.

## Phase 7: Reporting

Tasks:

- Generate `index.md` and `cluster-N.md`.
- Include a cross-directory duplication section in the duplicate-analysis report when candidates exist.
- Generate `concerns/index.md` and per-concern files when concern analysis is enabled.
- Generate `compare/index.md` and comparison detail files when project comparison is run.
- Include model identity, thresholds, retention mode, run timestamp, and source root.
- Include project labels and source roots where reports involve multiple projects.
- Keep stable cluster hashes based on relative path and body hash.
- Write exact duplicates once with all locations.
- Clean stale prior report files safely.

Acceptance:

- Markdown golden tests.
- Ignore-file test: a reviewed cluster disappears and reappears when body or path changes.
- Report includes language-tagged code fences.
- Cross-directory duplication report tests cover cluster span, dispersion, scope evidence, and local-only suppression.
- Concern report tests cover projection score ordering, structural spread, and candidate wording.
- Comparison report tests cover exact, strong, possible, split, merge, missing, and new classes.
- Report tests cover `full`, `report`, and `minimal` retention behavior.

## Phase 8: Benchmarks and Calibration

Tasks:

- Build benchmark fixture repos: small, medium, synthetic large.
- Compare `fastembed` model candidates on known duplicate/non-duplicate clusters.
- Add known cross-directory duplication fixtures with intentionally scattered and intentionally local duplicates.
- Add known concern-analysis fixtures with intentionally scattered concerns, local concerns, and generic helper false positives.
- Add project-comparison fixtures: small same-language rewrite, cross-language port, split function, merged functions, missing function, and new behavior.
- Measure index time, embed time, DB size, analysis time, peak memory.
- Tune defaults for thresholds and body-node count.
- Tune cross-directory duplication scoring and helper/scope downranking.
- Tune concern projection and structural-spread defaults.
- Tune project-comparison thresholds and decide how much name/path hints should affect scoring.
- Identify exact-search trigger points for future ANN work by indexed unit count, wall time, and peak memory.

Acceptance:

- Baseline benchmark document.
- Chosen default model has documented tradeoffs.
- Exact search limits are known, with a trigger point for ANN work.
- Cross-directory duplication precision/recall notes are documented against fixtures.
- Concern-analysis precision/recall notes are documented against fixtures.
- Project-comparison quality notes separate same-language rewrite quality from cross-language port quality.
- ANN remains disabled by default until exact-search limits are exceeded in measured workloads.

## Phase 9: Optional Vector Index

Tasks:

- Add `SimilarityIndex` trait.
- Prototype SQLite `vec1`, `sqlite-vec`, or `sqlite-vector-rs` behind a feature.
- Prototype `hnsw_rs` sidecar index rebuilt from SQLite embeddings.
- Evaluate Qdrant Edge separately for license, binary distribution, API stability, and performance.
- Prototype streaming subspace extraction behind an experimental command or benchmark harness: accumulate `C = X^T X` in batches and never materialize `G = X X^T`.
- Prototype spectra diagnostics such as bulk effective rank after dropping top common/anisotropy directions.
- Keep uncentered PCA as the default for embedding subspaces; centering normalized vectors must be explicit and benchmarked.
- Evaluate per-language and per-subsystem subspaces before interpreting axes as possible concerns in multi-language projects.
- Prototype NMF only on non-negative feature matrices such as identifier/token TF-IDF.
- Prototype graph Laplacian spectral clustering for splitting large duplicate/concern components only with sparse k-nearest-neighbor graphs, Nyström landmarks, or another bounded-memory method.
- Prototype model-to-model alignment diagnostics with Orthogonal Procrustes when anchor units exist, and evaluate CCA canonical correlations or principal angles for basis-invariant embedding-space comparison.

Acceptance:

- ANN implementation reproduces exact-search clusters within an agreed recall tolerance on benchmark fixtures.
- Exact search remains the deterministic default until ANN behavior is trusted.
- Feature selection is based on current maturity, distribution cost, update/delete behavior, and measured recall.
- Subspace experiments document anisotropy/common-direction handling, language-axis behavior, and whether axes can be labeled meaningfully.
- Model-comparison experiments do not use determinant as a headline quality metric.
- Experimental linear-algebra features stay out of the default report until they beat simpler projection/comparison baselines.

## Phase 10: Packaging

Tasks:

- Add release profiles.
- Document model cache behavior and offline use.
- Test Linux/macOS/Windows binary builds.
- Decide whether ONNX Runtime binaries are downloaded by build, runtime, or bundled feature.

Acceptance:

- User can install a release binary, run `init`, run `models download`, then run full analysis on a repo.
- Clear errors for missing CPU features, missing model files, private HF repos, and unsupported languages.
