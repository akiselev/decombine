# TODO

## CodeRankEmbed Quantization

- Build a CodeRank calibration/holdout corpus under `runs/model-calibration/`.
  - Source embedding texts from Altium, Cadabra, and OSS eval databases.
  - Stratify by language, body length, boilerplate-heavy code, short utilities, long functions near `max_length: 2048`, and test code.
  - Keep calibration and holdout files separate.
- Extend `scripts/coderank_onnx.py` with static int8 quantization once the calibration corpus exists.
  - Use ONNX Runtime calibration APIs.
  - Emit target-specific artifact directories such as `coderankembed-int8-static-avx512_vnni`.
  - Record artifact SHA256 hashes and quantization settings in `decombine-model-manifest.json`.
- Add a higher-level verification command that can optionally compare against the Torch `nomic-ai/CodeRankEmbed` reference, not only fp32 ONNX.
  - Gate fp32 ONNX vs Torch at pooled cosine `>= 0.99999`.
  - Gate int8 vs fp32 at mean pooled cosine `>= 0.999`, minimum pooled cosine `>= 0.995`, p95 pairwise delta `<= 0.005`, max pairwise delta `<= 0.02`, and top-10 recall `>= 0.98`.
- Run and record real quantized CodeRank experiments.
  - Altium CodeRank fp32 vs int8.
  - Cadabra CodeRank fp32 vs int8.
  - Promote int8 only if throughput improves by at least `1.5x` or RSS falls substantially without quality regression.

## Token-Aware Embedding Batching

- Add token-length instrumentation for embedding inputs.
  - Measure token counts with the same tokenizer used for inference.
  - Report p50/p90/p95/p99 token lengths, truncation count/rate, max observed length, padded token positions, and padding waste ratio.
- Replace or augment `embedding.max_batch_chars` with token-position limits.
  - Keep character limits as a fallback.
  - Add `embedding.max_batch_token_positions`.
  - Use `batch_size: 16` as the conservative CodeRank CPU default before token bucketing and `32` after token bucketing is validated.
- Batch by token length to reduce FastEmbed `BatchLongest` padding waste.
  - Sort or bucket each pending page by token length before embedding.
  - Preserve deterministic ordering for reproducible progress and logs.
- Keep embedding text whitespace-preserving by default.
  - Do not collapse raw whitespace until token stats show a win.
  - If added, make it an explicit model-identity field such as `embedding.input_text_mode: preserve|collapse_trivia`.
  - `collapse_trivia` must be tokenizer/AST-aware and must not rewrite string literal contents.
- Research long-unit handling before raising CodeRank `max_length` globally.
  - Prefer sliding windows or AST/block chunks plus vector aggregation over `max_length: 8192` for every unit.

## Accelerated Execution Providers

- Decide which ONNX Runtime providers to support first.
  - Recommended order: `directml` on Windows, `cuda` on NVIDIA/Linux, `coreml` on Apple, `openvino` or `xnnpack` for CPU acceleration research.
- Add Cargo feature plumbing for provider-specific builds.
  - The repo currently depends on `fastembed` but not directly on `ort`.
  - Provider support likely needs explicit `fastembed`/`ort` feature flags and packaging docs for provider libraries.
- Replace the current CPU-only backend rejection with provider dispatch once features are available.
  - Pass `with_execution_providers(...)` for both built-in and custom FastEmbed models.
  - Fail clearly when the binary was not built with the requested provider.
  - Record the effective provider in `ModelIdentity.execution_provider`.
- Add provider smoke tests that do not require every developer to have every accelerator.
  - CPU path stays default.
  - Hardware/provider tests should be opt-in.

## Robust Comparison Calibration

- Add overlap-robust calibration knobs.
  - `comparison.calibration_anchor: top1_p95|same_name|hybrid`
  - `comparison.min_same_name_anchors`
  - `comparison.candidate_sigma_floor`
  - `comparison.match_sigma_floor`
  - `comparison.strong_min_margin_sigma`
- Upgrade background calibration.
  - Compute same-name anchors from unambiguous same-language/same-kind cross-project names.
  - In hybrid mode, use `max(top1_p95, same_name_anchor)` when enough anchors exist.
  - Clamp effective thresholds with sigma floors, e.g. `match_eff >= bg + 4*sigma`.
  - Report anchor source/count and floors in markdown reports.
- Make one-to-one strong matches margin-aware.
  - Compute top-1/top-2 margins in both directions.
  - Require mutual best, score above match threshold, and margin above `strong_min_margin_sigma * sigma`.
  - Do not apply the same margin gate to split/merge classes.
- Validate on existing real databases.
  - Cadabra CodeRank robust calibration.
  - Cadabra CodeRank robust calibration plus `max_right_candidate_fanout: 10`.
  - Cadabra CodeRank robust calibration plus `abtt_directions: 1`.
  - Altium CodeRank robust calibration to protect known rename probes.
  - Success: `face_count` vs `TopologyStore.counts` is not strong; `Point3.vector_to`, `Point2.vector_to`, and `Vec2.length` stay covered; Altium rename probes stay strong.

## Compare-Phase Performance

- Optimize `VectorStore::top_k_between`.
  - Parallelize over query rows.
  - Use bounded top-k selection instead of collecting and sorting every threshold hit.
  - Replace O(n) pending-side checks in edge construction with sets or direction tags.
- Reuse calibration scans.
  - Avoid a full top-1 cross-product pass followed by two more candidate passes when background calibration is enabled.
- Add large-corpus compare benchmarks after classifier changes settle.
  - Record exact commands/configs/results in `EXPERIMENTS.md`.
  - Only add ANN/HNSW after exact search is measured as a real bottleneck on the OSS eval ladder.
