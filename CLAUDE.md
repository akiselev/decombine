# CLAUDE.md

decombine: an embedding-based detector for non-exact code duplication and
project-to-project comparison. Tree-sitter extraction → local ONNX embeddings
(fastembed) → SQLite → markdown reports. Rust replacement for the AGPL
upstream Slopo (`_upstream/`, research-only, never copy code from it).

Deeper docs: `architecture.md` (design), `PLAN.md` (phases), `EXPERIMENTS.md`
(every experiment, append-only log), `TODO.md` (next work),
`docs/benchmarks.md` (calibration baselines), `docs/query-interface.md`
(`query` command family + analyzer `--json` reference, with examples).

## Build, test, verify

- Prefix cargo commands with `env RUSTC_WRAPPER=` — sccache is configured but
  broken in this environment.
- Definition of done for any change: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` (all fast, <2 min).
- Golden snapshots (extraction + report fixtures) regenerate with
  `UPDATE_GOLDEN=1 cargo test`. Regenerate and eyeball the diff rather than
  hand-editing fixtures.
- Record every comparison/model/method experiment in `EXPERIMENTS.md` before
  ending the session: command/config, observed result, follow-up decision.

## Machine notes (dev box: WSL2, 16 cores, 32 GB)

- `/usr/bin/time` and `bc` do not exist here. Use
  `runs/oss-eval/run_phase.py <log> <cmd...>` to capture wall time + child
  peak RSS (prints `wall_s<TAB>max_rss_kb<TAB>rc`).
- Embedding saturates all cores: serialize embed runs, parallelize index runs
  (they are parser-bound with independent DBs). `runs/oss-eval/sweep.sh` is
  the reference benchmark driver.
- Long sweeps: run small corpora first so config mistakes fail cheaply.

## Model and embedding facts (measured, see EXPERIMENTS.md)

- Default model `BGESmallENV15` (fastembed catalog models truncate at 512
  tokens). Quality leader is `CodeRankEmbed` via `embedding.custom` ONNX at
  `~/.cache/decombine/custom/coderankembed` (fp32 community export verified
  vs Torch at cos 1.000000; its int8 sibling was broken — always verify
  exports against reference embeddings before use).
- Embed peak memory scales with `batch_items × longest_item_tokens²`.
  `embedding.max_batch_token_area` (default 16M) budgets this using the
  model tokenizer's exact counts (`Embedder::count_tokens` — the old chars/4
  estimate undershot by 1.3–2.3x in area). Batches are packed length-sorted;
  on CPU, smaller area budgets are faster as well as smaller (16M beat 32M
  on both time and RSS), so don't raise the budget chasing throughput.
- Raw cosine thresholds do not port across models. Port by background-relative
  position: BGE duplicate defaults `0.88/0.92/0.94` ≈ CodeRank
  `0.70/0.81/0.85` (measured backgrounds 0.698 vs 0.261 on the altium pair).
- Standing testbeds: `runs/altium-rebuilds/` (small, built-in rename ground
  truth), `runs/oss-eval/` (5-project OSS ladder: flask/express/gin/ripgrep/
  redis, corpora as shallow clones under `runs/oss-eval/corpora/`).

## Code map and gotchas

- `src/index/`: scanner → tree-sitter extraction. Languages are declarative:
  `assets/languages/<id>.toml` + `<id>/units.scm`; irregular cases (anonymous
  naming, receiver scopes, docstrings) live in adapters in
  `src/index/language.rs`.
- `src/embed/`: `Embedder` trait + fastembed backend + token-area batch
  packer (`pack_batches` in `embed/mod.rs`).
- `src/analyze/`: `duplicate/` (candidate pairs → rerank → distinct-body-
  bounded union-find → `ClusterKind` sectioning), `compare/`, `concerns/`.
- `src/report/markdown.rs`: all report rendering; duplicate index is
  sectioned Product / Mixed / Test-and-docs (mixed = the impl↔test
  false-positive shape).
- Incremental indexing skips unchanged files (mtime/size, then content hash —
  `touch` does NOT help), so extraction changes (naming, queries, adapters) do
  NOT refresh existing units. Cheap refresh without re-embedding:
  `sqlite3 db "DELETE FROM files; DELETE FROM code_units;"` then re-index —
  embeddings are keyed by (model, body-hash) and survive.
- A DB is bound to one model identity (`embedding.*` immutability checks);
  use one DB per model arm.
- Cluster hashes are stable over (path, body-hash) pairs and feed the ignore
  workflow; renumbering cluster files is fine, changing hash inputs is not.
