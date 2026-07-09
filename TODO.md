# TODO

## Agent Query Interface (core shipped 2026-07-09)

Steps 1–3 of `docs/research/agent-query-interface.md` landed: analyzer
`--json`/`--limit` envelopes with stable `unit:`/`cluster:`/`match:` IDs
(`src/report/json.rs`), and `decombine query`
capabilities/inspect/units/similar/search/qbe (`src/query/`). Remaining,
deferred until real agent usage demands them:

- JSONL event streams (`--jsonl`, `--progress jsonl`) for large result sets.
- `decombine schema <name>` returning JSON Schema for each envelope.
- Exit-code policy table and `--fail-on` (findings currently never fail).
- Step 4 graph expansion (`same_file`/`similar`/`duplicate`/`compare_match`/
  `concern_hit` edges over existing analyzer data).

## OSS Sweep v2 Residuals (2026-07-08) — resolved 2026-07-08, see EXPERIMENTS.md

- ~~Rust closure naming~~ DONE: `rust` adapter (let-binding / call-argument),
  Go assignment + enclosing-function fallback, JS enclosing-function fallback.
  `<anonymous>` is now zero across gin/ripgrep/express reports, both arms.
- ~~Idiom-family downrank~~ DONE: `Cluster.name_family` (≥6 members, ≥3
  scopes, ≥75% one name) orders same-name impl families last in their section
  with an explicit marker; `#[cfg(test)]` fns now classify as test code.
- ~~Semantic FP gate~~ CLOSED as tested-negative: no embedding-geometry
  statistic (prominence, out-margin, coherence, ABTT) separates the redis
  sentinel family from true duplicate families in either model. Future paths:
  CodeRank as analysis model, or non-embedding rerank features (AST statement
  histograms, call-graph overlap).
- ~~Memory budget + redis RSS anomaly~~ DONE: chars/4 undershot real token
  counts 1.3–2.3x in padded area; packer now uses exact tokenizer counts
  (`Embedder::count_tokens`) and the default budget dropped 32M → 16M
  (Pareto-better: coderank/gin 25.8→5.4 GB and faster; redis/bge
  16.8→4.5 GB and faster). No arena pathology, no linear term needed.
  `embed` now prints token percentiles/truncation/padding-waste.
  `runs/oss-eval/timings.tsv` embed rows are stale until the next sweep.

## CodeRankEmbed Quantization — DONE 2026-07-09 (int8 rejected, fp16 shippable)

Ran the full pipeline (see EXPERIMENTS.md 2026-07-09 + `docs/research/
quantized-model-distribution.md`). Verdict: **int8 is not viable** for
CodeRankEmbed at the quality gate (static minmax cos 0.569/recall 0.493;
dynamic per-tensor 0.925/0.804; dynamic per-channel 0.039/0.044 — all fail
mean≥0.999 / recall≥0.98). **fp16 is near-lossless** (cos 0.999998, recall
0.9983) at half the size (548→275 MB), so it is the shippable compressed
variant. fp16 CPU throughput is neutral (1.01×; ORT upcasts) — the win is
download size + GPU compute.

- fp16 ONNX produced at `~/.cache/decombine/custom/coderankembed-fp16` (hashes
  recorded in the distribution doc), verified, **pending HF upload + a
  `MANAGED_MODELS` entry** — the only remaining steps to distribute it (needs an
  HF repo + credentials; reuses the existing managed-model download/verify).
- Balanced calibration/holdout were built directly from a full-retention OSS
  index because the script's stratified split greedily fills alphabetically
  (concentrates on C). Worth fixing `split_corpus` to sample proportionally.
- `scripts/coderank_onnx.py` static path OOMs at max_length 2048 / percentile;
  `minmax` at 256 works. Consider adding a `quantize-dynamic` + `to-fp16`
  subcommand so the winning recipe is codified (currently done inline).
- Not pursued (int8 dead): per-target int8 artifact dirs, quantization manifest
  polish, `embedding.quantized` semantics for custom ONNX.

## Token-Aware Embedding Batching (mostly shipped 2026-07-08)

Shipped: length-sorted packing bounded by `embedding.max_batch_token_area`
(`items × longest_tokens²`, chars/4 token estimate clamped to the model's
truncation length), replacing count-driven batching. Result: 2–4x faster
embeds, BGE RSS halved, the coderank/express OOM case completes. Remaining:

- ~~Add token-length instrumentation for embedding inputs.~~ DONE
  (2026-07-08, see EXPERIMENTS.md). `embed` prints p50/p90/p99/max, truncation
  count/rate, and padding waste over newly-embedded inputs using exact
  tokenizer counts. New `tokens` subcommand + `embed::token_report` add the
  per-language, **untruncated** distribution over all indexed units (works on
  already-embedded DBs), exposing over-cap severity that `count_tokens` had
  clamped. Measured truncation at 512: Python 1.4% → C/heavy-Rust ~13%; at
  CodeRank's 2048: 0.0–0.8%. → data supports CodeRank-as-default (below) and
  scopes long-unit chunking to the <1% tail.
- Keep embedding text whitespace-preserving by default.
  - Do not collapse raw whitespace until token stats show a win.
  - If added, make it an explicit model-identity field such as `embedding.input_text_mode: preserve|collapse_trivia`.
  - `collapse_trivia` must be tokenizer/AST-aware and must not rewrite string literal contents.
- Research long-unit handling before raising CodeRank `max_length` globally.
  - Prefer sliding windows or AST/block chunks plus vector aggregation over `max_length: 8192` for every unit.
  - Scope: only the <1% tail over 2048 needs this (see truncation audit
    2026-07-08). Not urgent once CodeRank is the default.

## Make CodeRank the Default Model (2026-07-08) — mostly DONE 2026-07-08

Motivation: the token audit (EXPERIMENTS.md 2026-07-08) shows BGE's 512 cap
silently truncates 1.4% (Python) to ~13% (C, heavy Rust) of units, worst on
large real codebases; CodeRank's 2048 cap drops that to 0.0–0.8%. Switching the
default closes a real quality hole.

- ~~Distribution story~~ DONE: "managed model" — `config::MANAGED_MODELS`
  pins the HF files by SHA256; the fastembed backend downloads (streamed,
  hash-gated) + verifies into `<cache>/custom/<id>` on first use and loads via
  the custom ONNX path. `ureq` optional dep, fastembed-gated.
- ~~Port duplicate threshold defaults~~ DONE: `0.88/0.92/0.94` →
  `0.70/0.81/0.85`; tests pinned to the old values to decouple from the flip.
- ~~Update init template + docs~~ DONE: CONFIG_TEMPLATE, architecture.md,
  benchmarks.md; BGE documented as the lightweight/no-download option.
- Remaining:
  - Smoke-test the real network download on a clean machine (this env had the
    files cached, so only the verify-and-skip path was exercised).
  - Port comparison raw-threshold defaults (left at BGE scale; opt-in + has
    background calibration — belongs with Robust Comparison Calibration below).
  - Refresh calibration baselines / `docs/benchmarks.md` tables under the
    CodeRank default when the next sweep runs.
  - Consider pinning `revision` to a commit SHA (currently `main`; integrity is
    still gated by per-file SHA256).

## Accelerated Execution Providers — Stage 0 + drift gate DONE

Stage 0 already shipped (see EXPERIMENTS.md "Execution-Provider Backend
Plumbing"): cargo lanes `accel`/`cuda`/`directml`/`coreml`/`openvino`/
`load-dynamic`, `ort` shared with fastembed, `resolve_providers` /
`build_accelerator` dispatch passing `with_execution_providers(...)` for catalog
AND custom models, effective provider recorded in
`ModelIdentity.execution_provider`, `provider_mode: require|auto` fallback, and
`doctor [--provider X]` reporting + smoke test.

- ~~Drift gate~~ DONE 2026-07-09: `decombine drift --baseline A.db --candidate
  B.db` compares embeddings across two DBs (cosine distribution, component
  delta, top-k neighbour recall) and fails non-zero on drift. Validated on CPU
  (self-vs-self PASS, cross-model FAIL). This is the CI guardrail for
  accelerator builds.
- Remaining (hardware/CI-gated — this dev box is CPU-only):
  - Actually build + smoke-test one accelerator lane (DirectML on Windows is the
    recommended first artifact) and run `drift` against the CPU DB to confirm
    same-model cross-provider cosine ≈ 1.0 / recall ≈ 1.0.
  - CI matrix + per-provider release packaging (docs/research/
    release-architecture.md): provider libraries, artifact lanes, the drift gate
    wired as a release check.
  - Opt-in hardware tests (CPU path stays the default, no accelerator required
    for `cargo test`).

## Robust Comparison Calibration — DONE 2026-07-08 (see EXPERIMENTS.md)

- ~~Overlap-robust calibration knobs~~ DONE: `calibration_anchor`
  (top1_p95|same_name|hybrid, default hybrid), `min_same_name_anchors` (3),
  `candidate_sigma_floor` (2.0), `match_sigma_floor` (4.0),
  `strong_min_margin_sigma` (0.0, opt-in).
- ~~Upgrade background calibration~~ DONE: same-name/same-kind/same-language
  unambiguous anchor (median cosine), hybrid = max(top1_p95, same_name),
  `bg + kσ` floors, anchor source/count + floors reported.
- ~~Margin-aware strong matches~~ DONE: mutual-best + raw ≥ match + top1−top2
  margin ≥ `strong_min_margin_sigma·σ` both directions; split/merge exempt.
- ~~Validate on real databases~~ DONE: cadabra + altium CodeRank. All success
  criteria met — `face_count`↔`TopologyStore.counts` demoted strong→possible
  by the bg+4σ match floor; `vector_to`/`length` stay covered; altium renames
  stay strong. candidate_sigma_floor tuned 3→2 (recall guard; strong/split/
  merge are match-gated and invariant to it).
- Remaining (optional):
  - Ablations not yet run: robust + `max_right_candidate_fanout: 10`, robust +
    `abtt_directions: 1` (both compose with the new knobs; measure if needed).
  - Tune `strong_min_margin_sigma` if a corpus shows semantic-magnet strong
    FPs the floors miss (currently off; the floors already fixed the known FP).

## Compare-Phase Performance — DONE 2026-07-08 (see EXPERIMENTS.md)

- ~~Optimize `VectorStore::top_k_between`~~ DONE: parallel over `from` (rayon);
  bounded top-k selection (no collect-and-sort-every-hit); edge construction
  uses the known per-pass direction instead of O(n) `pending_left.contains`.
- ~~Reuse calibration scans~~ DONE: background calibration reuses the unpruned
  left→right scan for the top-1 anchor (full cross-product scans 3 → 2).
- ~~Large-corpus compare benchmark~~ DONE: cadabra CodeRank (4.6k/side) release
  0.9 s (was 8.6 s single-thread; debug 25.7 s, was >10 min). Behavior byte-
  identical to pre-#5. Exact search is NOT a bottleneck → ANN/HNSW stays
  deferred.
- Remaining (optional): `top_k_between` is still O(n²) in dot products; only
  revisit (ANN/HNSW, or blocked SIMD kernels) if a corpus >>10k units/side
  makes exact search a measured bottleneck.
