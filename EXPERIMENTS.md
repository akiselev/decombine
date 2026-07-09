# Experiments

## 2026-07-03 Cadabra vs Cadabra2 Baseline

- Command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/decombine.yaml`
- Config: Rust only, `BGESmallENV15`, comparison candidate threshold `0.78`, match threshold `0.86`, top-k `5`, name/path hints enabled.
- Scope: `cadabra` reference (`4145` units, `537` files) vs `cadabra2` candidate (`757` units, `55` files).
- Result: `4238` match records: `3` exact copies, `55` strong matches, `2718` possible matches, `57` splits, `39` merges, `1128` possible missing coverage, `238` possible new behavior.
- Observations: exact copies and semantic renames are real signals; split/merge buckets reveal useful architectural consolidation; possible matches and some strong matches overmatch generic small functions such as simple counters, constructors, arithmetic wrappers, and assertions.
- Decision: before changing model choice, improve evaluation UX by reporting comparison-specific thresholds, adding progress output, and putting source snippets/examples into comparison detail pages.

## 2026-07-03 Comparison Report UX Pass

- Change: comparison reports now record comparison-specific thresholds (`candidate_threshold`, `match_threshold`, `top_k_per_unit`) and hint settings instead of duplicate-analysis thresholds.
- Change: `compare` emits phase progress for exact-copy classification, forward/reverse candidate search, edge ranking, split/merge classification, strong matching, and leftovers.
- Change: detail pages lead with up to five source-backed examples before the full record list.
- Validation: `cargo fmt --check` and `env RUSTC_WRAPPER= cargo test` passed.
- Cadabra refresh: reran `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/decombine.yaml`; result stayed at `4238` match records, confirming report-only/UX changes did not alter classification.
- Observation: snippets make true positives easy to confirm (`Point3.vector_to` migrated from `Vec3` to `Vector3`) and expose false positives quickly (`face_count` matched to `TopologyStore.counts` because both are count-like aggregations).
- Decision: next method experiment should add explicit generic/small-unit noise controls and more granular candidate-search progress/timing before spending time on alternate embedding models.

## 2026-07-03 Code Embedding Model Research

- Task: find Hugging Face code embedding models that are realistically compatible with the current Rust runner (`fastembed` 5.17.2, CPU ONNX `TextEmbedding` path).
- Current runner constraint: `src/embed/fastembed_backend.rs` only maps `BGESmallENV15`, `BGEBaseENV15`, and `JinaEmbeddingsV2BaseCode`; `src/config.rs` only validates those three model names. Adding another normal `EmbeddingModel` variant is a small config/backend mapping change. Adding Qwen3 is a larger backend change because fastembed exposes it through `Qwen3TextEmbedding` behind the `qwen3` Candle feature, not through the existing ONNX `TextEmbedding` path.
- Directly compatible code-specific candidate: `JinaEmbeddingsV2BaseCode` (`jinaai/jina-embeddings-v2-base-code`). It is already supported by our runner, has 768 dimensions, Apache-2.0 license, ONNX availability, 161M parameters, 8192-token context, and model-card coverage for English plus 30 programming languages. Source: https://huggingface.co/jinaai/jina-embeddings-v2-base-code
- Stronger but not drop-in candidate: `Qwen/Qwen3-Embedding-0.6B`. It is a newer Apache-2.0 embedding model with published claims of strong multilingual and code-retrieval performance, and fastembed 5.17.2 has a `Qwen3TextEmbedding::from_hf` path. Cost: enable fastembed `qwen3`, add Candle dependencies/runner wrapper, handle safetensors HF loading, choose CPU dtype/device/max length, and record a different backend/runtime identity. Sources: https://huggingface.co/Qwen/Qwen3-Embedding-0.6B and https://github.com/Anush008/fastembed-rs
- Useful non-code baselines already available in fastembed `EmbeddingModel`: `GTEBaseENV15`/`GTELargeENV15`, `SnowflakeArcticEmbedM`/`L`, `BGEM3`, `MxbaiEmbedLargeV1`, `ModernBertEmbedLarge`, and `EmbeddingGemma300M`. These are not code-specialized, but some are stronger retrieval baselines than BGE v1.5 on general MTEB retrieval. Sources: https://github.com/Anush008/fastembed-rs, https://huggingface.co/Alibaba-NLP/gte-base-en-v1.5, https://huggingface.co/Snowflake/snowflake-arctic-embed-m, https://huggingface.co/BAAI/bge-m3
- Caveat: public text-retrieval benchmarks do not directly measure decombine's migration/clone-detection task. The practical quality metric should be cadabra/cadabra2 inspection: exact/strong precision, false-positive rate among generic units, missing/new list usefulness, and runtime/memory.
- Decision: next model experiment should first run cadabra/cadabra2 with `JinaEmbeddingsV2BaseCode` because it is already wired in and code-specific. After that, add one cheap general-retrieval baseline (`SnowflakeArcticEmbedM` or `GTEBaseENV15`) as a small fastembed mapping change. Defer Qwen3 until Jina/general baselines prove model quality, not method noise, is the bottleneck.

## 2026-07-03 Cadabra Compare Method Sweep

- Hints-off command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/no-hints.yaml`
- Hints-off config: reused the baseline `BGESmallENV15` database, comparison candidate threshold `0.78`, match threshold `0.86`, top-k `5`, name/path hints disabled.
- Hints-off result: `4236` match records: `3` exact copies, `57` strong matches, `2712` possible matches, `55` splits, `41` merges, `1128` possible missing coverage, `240` possible new behavior.
- Hints-off observation: disabling hints barely changed counts versus baseline (`4238` records), so the dominant noise source is not path/name hint rescue; it is generic code-unit similarity and candidate ranking.
- Strict-match command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/strict-match.yaml`
- Strict-match config: reused the baseline `BGESmallENV15` database, comparison candidate threshold `0.78`, match threshold raised from `0.86` to `0.90`, top-k `5`, name/path hints enabled.
- Strict-match result: `4421` match records: `3` exact copies, `20` strong matches, `2951` possible matches, `18` splits, `9` merges, `1128` possible missing coverage, `292` possible new behavior.
- Strict-match observation: raising only the match threshold reduces confident classifications but mostly pushes plausible work into the already-large possible bucket; it improves caution, not usability.
- Decision: add generic/small-unit noise controls and better candidate-search instrumentation; do not rely on hints or a stricter threshold alone to fix precision.

## 2026-07-03 Jina Model Startup and Partial Embed

- Config: `runs/cadabra-jina-compare/decombine.yaml`, Rust only, `JinaEmbeddingsV2BaseCode`, CPU, batch size `128`, pending page size `256`, max batch chars `120000`.
- Index command: `env RUSTC_WRAPPER= cargo run -- index --config runs/cadabra-jina-compare/decombine.yaml`
- Index result: `cadabra` indexed `537` files and `4145` units; `cadabra2` indexed `55` files and `757` units.
- Initial embed command: `env RUSTC_WRAPPER= cargo run -- embed --config runs/cadabra-jina-compare/decombine.yaml`
- Initial embed observation: before the progress fix, model download/load was silent for several minutes; after initialization, embedding progressed to `384/4625` bodies before the run was interrupted.
- UX change: added visible model initialization progress output for `embed` and `models download`, because FastEmbed's internal `show_download_progress` did not appear in captured CLI output.
- Verification command: `env RUSTC_WRAPPER= cargo run -- models download --config runs/cadabra-jina-compare/decombine.yaml`
- Verification result: output now includes visible model progress lines such as `model: [=>========] loading/downloading embedding model JinaEmbeddingsV2BaseCode (2s)`, then reports the model ready with `768` dimensions.
- Resumed embed result: after the partial run, resume reported `4241` pending bodies and embedded `256` more bodies in two batches before being stopped.
- Observation: on CPU, `JinaEmbeddingsV2BaseCode` is much slower than the BGE small baseline for this corpus; the full cadabra/cadabra2 run would likely take too long for quick iteration.
- Decision: use Jina first on a smaller evaluation subset or after adding better runtime controls/provider support. For immediate functionality improvement, prioritize generic/small-unit noise controls and candidate-search timing.

## 2026-07-03 Cadabra Compare Noise Controls

- Change: added optional comparison controls `min_body_node_count` and `max_right_candidate_fanout`. Both default to `0` (disabled). Exact copies are still classified before semantic matching; these controls affect semantic candidate eligibility/ranking. Comparison reports now list suppressed right-side candidate targets with their fanout when fanout suppression is active.
- Validation: `cargo fmt --check`, `env RUSTC_WRAPPER= cargo test --test analysis comparison`, and `env RUSTC_WRAPPER= cargo test --test report_golden` passed.
- Node-count experiment command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/min-nodes-30.yaml`
- Node-count result (`min_body_node_count: 30`): `4467` records: `1` exact copy, `36` strong matches, `1915` possible matches, `32` splits, `25` merges, `2042` possible missing coverage, `416` possible new behavior.
- Node-count experiment command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/min-nodes-40.yaml`
- Node-count result (`min_body_node_count: 40`): `4527` records: `0` exact copies, `38` strong matches, `1711` possible matches, `27` splits, `20` merges, `2270` possible missing coverage, `461` possible new behavior.
- Node-count observation: this removed the known false strong match `BoundedSsiFaceImprintPublicationBatchReport.face_count` ↔ `TopologyStore.counts`, but it also hid useful small geometry matches such as `Point3.vector_to` and `Point2.vector_to` by pushing the right-side tiny functions into `new_in_right`. It is useful as a blunt review knob, not as a good default.
- Right-fanout experiment command: `env RUSTC_WRAPPER= cargo run -- compare --config runs/cadabra-compare/right-fanout-10.yaml`
- Right-fanout result (`max_right_candidate_fanout: 10`): `4443` records: `3` exact copies, `45` strong matches, `1118` possible matches, `50` splits, `15` merges, `2893` possible missing coverage, `319` possible new behavior.
- Right-fanout observation: this is the better control. It removed the `face_count` ↔ `TopologyStore.counts` false strong match and left `TopologyStore.counts` as possible new behavior, while preserving true small geometry matches including `Point3.vector_to`, `Point2.vector_to`, and `Vec2.length` split examples. The suppression table made the main semantic magnets explicit, including `tests.curved_surface_eval_exposes_derivatives_and_topology_metadata` (`81` fanout), `intersection_recorder_builds_report_from_branch_witness` (`72`), `intersect_plane_cylinder_rects_returns_topology_ready_full_circle` (`66`), `TopologyStore.new` (`50`), and `TopologyStore.face` (`43`).
- Remaining issue: the fanout run spent long stretches inside forward/reverse candidate search with only phase-level progress, so comparison needs chunk-level candidate-search timing/progress next.
- Decision: keep both controls because they expose different review tradeoffs, but prefer `max_right_candidate_fanout` for cadabra/cadabra2. Next method work should make high-fanout suppression visible in the report and add candidate-search progress/timing.

## 2026-07-03 Altium Rebuild Pair Corpus

- New corpus: `altium-cli-rebuild` (16 files, 119 units, ~2.9k lines) vs `altium-cli-rebuild-codex` (12 files, 256 units, ~3.8k lines) — two independent rebuilds of the same Altium CLI, both Rust. Configs in `runs/altium-rebuilds/`. Small enough for fast full-sweep iteration including CPU-expensive models.
- Baseline command: `runs/altium-rebuilds`: `decombine index/embed/compare --config decombine.yaml` (`BGESmallENV15`, candidate `0.78`, match `0.86`, top-k `5`, hints on).
- Baseline result: `281` records: `19` exact copies, `8` strong matches, `54` possible matches, `14` splits, `9` merges, `5` possible missing coverage, `172` possible new behavior.
- Quality: strong matches are excellent on this corpus — same-named cross-rebuild functions (`SchLib.component_names` at `0.9997`, `read_blocks` at `0.9911`) plus true semantic renames (`parse_params` ↔ `parse_entries` at `0.9306`, `tests.preserves_duplicate_keys` ↔ `duplicate_keys_are_preserved_after_edit` at `0.9165`). The 19 exact copies are shared/derived code between the rebuilds.
- Fanout check: `right-fanout-10.yaml` (`max_right_candidate_fanout: 10`) produced identical counts to baseline — no right-side unit exceeds fanout 10 here, so the cadabra semantic-magnet problem does not appear at this corpus size.
- Other candidate pairs surveyed in `~/git` for future runs: `old-altium-cli` (114 files, 36k lines) vs `altium-cli` (139 files, 109k lines) as an evolution pair, `solverang` vs `solverang-cad`, `decombine` vs `decombine2`. All Rust-dominant.
- Decision: use the rebuild pair as the standing model/method testbed; the two known semantic renames plus the same-named function set act as a small built-in ground truth.

## 2026-07-03 Five-Model Sweep on Altium Rebuild Pair

- Change: extended the runner with four fastembed baseline models — `AllMiniLML6V2` (384d), `GTEBaseENV15` (768d), `SnowflakeArcticEmbedM` (768d), `NomicEmbedTextV15` (768d, 8192 ctx) — as `SUPPORTED_MODELS` entries in `src/config.rs` plus mappings in `src/embed/fastembed_backend.rs` (quantized variants included). `cargo fmt --check` and `env RUSTC_WRAPPER= cargo test` (99 tests) pass.
- Sweep: identical corpus/thresholds (`0.78`/`0.86`/top-k `5`, hints on), one db+report per model in `runs/altium-rebuilds/` (`minilm|gte|arctic|nomic|jina.yaml`).
- Counts (exact/strong/possible/split/merge/missing/new): BGE-small `19/8/54/14/9/5/172`; MiniLM `19/16/21/3/3/52/203`; GTE-base `19/10/34/11/4/30/190`; Arctic-M `19/11/42/15/5/19/177`; Nomic-v1.5 `19/13/33/8/5/33/193`; Jina-code `19/18/14/4/3/58/203`.
- Rename probes (`parse_params`↔`parse_entries`, `preserves_duplicate_keys`↔`duplicate_keys_are_preserved_after_edit`): strong in BGE-small (both) and Arctic-M (both); Nomic gets the second only (first at `0.8540` possible); Jina scores them `0.8286`/`0.8291` (possible), GTE `0.8572`/split, MiniLM `0.8456`/`0.8300` (possible). Every model still ranks the correct partner top-1 — the misses are purely absolute-threshold artifacts.
- Key insight: cosine score distributions differ enough per model that fixed `0.78`/`0.86` thresholds are not portable. Jina-code finds the most same-name strong matches (18) yet reports the most false "missing coverage" (58, e.g. real counterparts scored just under 0.78). MiniLM has the same pathology (52 missing, e.g. `derive_storages`, `assert_streams_match` dropped below candidate threshold).
- Runtime: all five models embedded the 326-body corpus in seconds on CPU, including Jina-code — the cadabra slowness was corpus size, not per-body cost alone; small-corpus sweeps are the right iteration loop for model work.
- Decision: before crowning any model, add per-model threshold calibration (relative/margin-based classification, e.g. score percentile within the run or top1–top2 margin, instead of absolute cosine cutoffs). With fixed thresholds, `SnowflakeArcticEmbedM` is currently the best drop-in alternative (recovers both renames, lowest false-missing among the new models); `JinaEmbeddingsV2BaseCode` needs recalibrated thresholds (~0.80 match / ~0.72 candidate by eyeball) to be judged fairly.

## 2026-07-03 Code Embedding Model Research, Round 2 (user-defined ONNX path)

- Finding: fastembed 5.17.2 supports arbitrary models via `TextEmbedding::try_new_from_user_defined(UserDefinedEmbeddingModel, InitOptionsUserDefined)` — needs ONNX bytes, tokenizer files, pooling, quantization mode, optional output key. This removes the "must be in the `EmbeddingModel` enum" constraint; any HF repo with `onnx/model.onnx` + `tokenizer.json` is loadable with a modest backend extension (manual file download/cache instead of fastembed's built-in fetcher).
- Best new candidate: `nomic-ai/CodeRankEmbed` — 137M params, MIT, 8192-token context, code-specialized bi-encoder fine-tuned from `Snowflake/snowflake-arctic-embed-m-long` on the CoRNStack dataset (77.9 MRR CodeSearchNet, 60.1 NDCG@10 CoIR). No official ONNX, but community exports exist with the exact file set the user-defined path needs (e.g. `Zenabius/CodeRankEmbed-onnx`: `onnx/model.onnx`, `onnx/model_quantized.onnx`, `tokenizer.json`, mean pooling). Caveats: community exports are low-download/unverified — prefer exporting our own ONNX from the MIT source weights via optimum; queries need the prefix `Represent this query for searching relevant code` (document-side embeddings, which is what decombine compares, need no prefix).
- Checked and rejected for now: `jinaai/jina-code-embeddings-0.5b/1.5b` (newer, strong, but CC-BY-NC license and no official ONNX); `Qodo/Qodo-Embed-1-1.5B` (custom license, no official ONNX, 1.5B heavy for CPU); `nomic-ai/nomic-embed-code` (7B, Apache-2.0, far too heavy for CPU ONNX).
- Note: `Snowflake/snowflake-arctic-embed-m-long` itself ships full official ONNX and is already in fastembed's enum (`SnowflakeArcticEmbedMLong`) — adding it is a two-line mapping change and gives the same architecture/context length as CodeRankEmbed without the code fine-tune, making it the perfect ablation partner (base vs code-tuned).
- Decision: next model work order: (1) per-model threshold calibration in the comparator, (2) add `SnowflakeArcticEmbedMLong` mapping, (3) build the user-defined-ONNX backend path and evaluate CodeRankEmbed (own optimum export) against Arctic-M-Long on the altium rebuild pair, then cadabra.

## 2026-07-03 Jina Threshold Recalibration Check

- Command: `runs/altium-rebuilds`: `decombine compare --config jina-calib.yaml` (`JinaEmbeddingsV2BaseCode`, candidate lowered `0.78`→`0.72`, match lowered `0.86`→`0.80`, same db as the jina sweep run).
- Result: `308` records: `19` exact copies, `18` strong matches, `13` possible matches, `8` splits, `4` merges, `52` possible missing coverage, `194` possible new behavior.
- Probe outcome: both semantic renames became strong matches (`parse_params` ↔ `parse_entries` at `0.8286`, `preserves_duplicate_keys` ↔ `duplicate_keys_are_preserved_after_edit` at `0.8291`), confirming the misses in the fixed-threshold sweep were calibration artifacts, not ranking failures.
- Residual-missing check: remaining `missing_in_right` entries at score `0.0000` are largely genuine decomposition differences, e.g. `derive_storages` has no codex counterpart because the codex rebuild handles storage paths inline via `parent_storage_path`.
- Decision: confirms per-model threshold calibration is the highest-value next comparator feature; hand-tuned yaml thresholds work but should become automatic (percentile- or margin-based classification recorded alongside absolute scores).

## 2026-07-03 Background Threshold Calibration Feature

- Change: added `comparison.calibration: none|background` and `comparison.calibration_sample_pairs` (default `4096`). In `background` mode the comparator samples deterministic random cross-project pairs for a background cosine mean, anchors the top of the scale at the 95th percentile of per-left-unit top-1 scores, and reinterprets `candidate_threshold`/`match_threshold` as normalized positions between those anchors (`effective = bg + t * (anchor - bg)`). A `MIN_CALIBRATION_RANGE` guard (`0.05`) falls back to raw thresholds when the corpus has no signal range. Calibration stats (mean, std, anchor, effective thresholds, applied flag) are recorded in `ComparisonReport` and printed in the report header. Unit tests cover the rescaling win and the narrow-range fallback; `fmt`/`clippy`/`cargo test` (101 tests) clean.
- Also added `SnowflakeArcticEmbedMLong` (768d, 8192 ctx) as a two-line fastembed mapping.
- Measured background means per model on the altium pair: BGE-small `0.698`, Arctic-M-Long `0.699`, Arctic-M `0.615`, Nomic `0.589`, GTE-base `0.569`, MiniLM `0.366`, Jina-code `0.265`, CodeRankEmbed `0.261` — a 0.44-wide spread that no fixed raw threshold can straddle.
- Position tuning: interpreting the old raw values (`0.78`/`0.86`) as positions is far too strict (BGE effective match `0.956`). Converting BGE's proven raw thresholds into normalized space gives positions ≈ `0.27`/`0.54`; sweeping positions `0.30`/`0.55` (calib2) and `0.30`/`0.65` (calib3) across all models found `0.30`/`0.65` best.
- Calib3 result (exact/strong/possible/split/merge/missing/new): BGE `19/13/57/9/7/6/181`, MiniLM `19/11/50/11/6/7/182`, GTE `19/10/52/14/4/8/179`, Arctic-M `19/11/57/15/5/4/173`, Nomic `19/13/51/9/5/13/186`, Jina-code `19/18/59/10/4/3/179`. Sanity check: BGE's effective thresholds under calib3 come out `0.788`/`0.863` — nearly identical to the hand-tuned raw baseline, and its class counts match the baseline within ±1.
- Probe outcome: with one portable config, both semantic renames are recovered as strong matches in 5 of 6 models (GTE keeps `preserves_duplicate_keys` in a split that still contains the true partner). False "missing coverage" for Jina/MiniLM collapsed from `58`/`52` to `3`/`7`.
- Decision: `calibration: background` with positions `0.30`/`0.65` is the new recommended comparison setup for cross-model work; raw thresholds remain the default for backward compatibility.

## 2026-07-03 CodeRankEmbed via Custom ONNX vs Arctic-M-Long Ablation

- Change: added `embedding.custom` config block (`dir`, `onnx_file`, `dimensions`, `pooling` mean|cls, `max_length`) loading arbitrary local ONNX exports through fastembed's `try_new_from_user_defined`. The model identity records the ONNX path, pooling, and max length as the revision; `quantized: true` is rejected for custom models. Validation and tests clean.
- Model: `nomic-ai/CodeRankEmbed` (137M, MIT, code-tuned from `Snowflake/snowflake-arctic-embed-m-long` on CoRNStack), community ONNX export `Zenabius/CodeRankEmbed-onnx` (fp32, 548MB) cached at `~/.cache/decombine/custom/coderankembed`, pooling mean, max_length `2048`, batch 32. Embedding 326 bodies took ~6 min CPU (vs seconds for enum models) — fine for the small testbed.
- Configs: `runs/altium-rebuilds/coderank.yaml` and `arcticlong.yaml`, both with calibration positions `0.30`/`0.65`.
- CodeRankEmbed result: `294` records: `19` exact, `16` strong, `56` possible, `9` splits, `5` merges, `7` missing, `182` new. Calibration: background mean `0.261`, effective candidate `0.480` / match `0.737`.
- Arctic-M-Long (same architecture, no code fine-tune) result: `290` records: `19` exact, `15` strong, `60` possible, `14` splits, `2` merges, `5` missing, `175` new. Background mean `0.699`.
- Ablation verdict: the code fine-tune wins on the rename probes. CodeRankEmbed gets all three cross-name renames strong — `parse_params`↔`parse_entries` (`0.839`), `preserves_duplicate_keys`↔`duplicate_keys_are_preserved_after_edit` (`0.791`), and uniquely `read_footprint_data`↔`decode_pcb_record` (a real rename no other model classified strong). Arctic-M-Long gets only `parse_params` strong; the other two stay possible.
- Caveat: community export is unverified; before regular use, export our own ONNX from the MIT weights with optimum and diff a few embeddings against the community file.
- Decision: CodeRankEmbed is the current quality leader on the testbed. Next: reproduce on cadabra/cadabra2 (runtime permitting), and consider an int8/quantized export to cut the 6-min CPU cost.

## 2026-07-03 Linear-Algebra Experiments over Model Embeddings (docs/ideas)

- Tool: `scripts/embedding_experiments.py <run-dir>` — runs the testable ideas from `docs/ideas/linear-algebra-research-synthesis.md` over all model dbs of the altium testbed (326 aligned bodies × 8 models, 20 ground-truth pairs = 17 unique same-name pairs + 3 renames).
- E1 spectrum gauge: effective rank of `C = XᵀX/n` cleanly explains the calibration findings. BGE-small uses effectively `6.4` of 384 dimensions with `70%` of energy in the top principal direction (`|mean| = 0.836`); Arctic-M-Long is similar (`6.1`). The code-tuned models are far more isotropic: Jina-code eff-rank `30.8`, CodeRankEmbed `40.2`, with background mean ≈ top-1 energy ≈ `0.27`. Anisotropy (mean-vector norm) predicts the measured background mean almost exactly — the "background similarity" the calibrator samples *is* the top common direction.
- E2 all-but-the-top (ABTT): removing the corpus mean plus top `m` principal directions and renormalizing standardizes every model to background `≈ 0.00 ± 0.13` and *improves* ground-truth separation everywhere: at `m=3`, gt z-score rises from `3.7–5.2` to `5.7–7.7` per model, top-1 retrieval hits `1.00` for Jina-code (was `0.95`) and `0.95` for GTE/MiniLM, and top-1 margins roughly triple (e.g. BGE `0.069 → 0.314`). ABTT is a principled, model-agnostic alternative that both ports thresholds *and* sharpens separation — stronger than background calibration, which only fixes portability. Cost is trivial: one `d×d` eigendecomposition per run.
- E3 cross-model agreement: Spearman correlation of pairwise-similarity structure ranges `0.56–0.88` — models genuinely disagree on mid-scale structure (max: Arctic-M vs Arctic-M-Long `0.878`, same family; min: Arctic-M vs Jina-code `0.562`). CCA between top-16 PCA subspaces shows top canonical correlations `0.96–0.99` decaying — dominant axes are shared, differentiation lives in the tail. Note: full-rank CCA is degenerate at `n=326 < d` (all correlations 1.0); the PCA-restricted version is the meaningful one.
- E4 rank-fusion ensemble: mean of per-row rank percentiles across model pairs. Top-1 rate saturates at `1.00` on this corpus for many singles, so no ensemble headroom is measurable here; needs a harder corpus (cadabra) or margin-based metrics.
- Decision: promote ABTT (`m=2..3`, per-run, cross-project corpus) to a comparator feature candidate — likely as `comparison.calibration: abtt` or an embedding post-processing flag — and re-test whether background calibration is still needed on top. The spectrum gauge (eff-rank + top-1 energy) is worth printing in `models`/report headers as a model-diagnostic. Defer ensembles and spectral clustering until a labeled, harder benchmark exists.

## 2026-07-03 ABTT Preprocessing in the Comparator

- Change: added `comparison.abtt_directions` (default `0`). When set, the comparator subtracts the corpus mean, projects out the top-m principal directions of the centered vectors (deterministic power iteration with deflation — no dense `d×d` matrix, no new dependencies), renormalizes, and runs calibration plus candidate search on the transformed vectors. Report headers record the setting. Unit test constructs a shared-boilerplate axis plus a high-variance nuisance axis where raw cosine confidently pairs the wrong units (a false merge at `0.99` while true pairs sit at `0.65`) and shows ABTT recovers the true pairs and classifies the counterpart-less unit as missing. `fmt`/`clippy`/tests clean (102 tests; golden refreshed for the new header field).
- Sweep A (`abtt_directions: 2`, old raw thresholds `0.78`/`0.86`, no calibration): fails as predicted — post-ABTT scores live on a lower scale, so both rename probes fall below the candidate cutoff in every model. ABTT does not make *old* absolute thresholds portable; it moves everyone to a new common scale.
- Sweep B (`abtt_directions: 2` + calibration positions `0.30`/`0.65`): same-name strong-match yield rises sharply (Arctic-M `19` strong vs `11` without ABTT; MiniLM `16` vs `11`), but `parse_params`↔`parse_entries` drops from strong to possible in 4 of 6 models — the rename pairs' post-ABTT scores (`0.54`–`0.68`) sit below calibrated match thresholds that the near-duplicate top-1 anchor pushes up.
- Sweep C (`abtt_directions: 1` + calibration): gentler and better — both renames strong in BGE/MiniLM/Arctic/Jina, missing coverage `3`–`15`, MiniLM notably improved over plain calibration (16 strong, both renames strong). GTE remains the straggler (renames possible).
- Honest verdict: with the current absolute-threshold classifier, ABTT+calibration is roughly equal to plain background calibration on the rename probes while boosting same-name yield; the rank/margin improvements the offline analysis showed (E2: margins ~3×) are invisible to a classifier that never looks at margins. ABTT stays opt-in (default off).
- Decision: the classifier, not the preprocessing, is now the bottleneck — add margin-aware strong-match classification (e.g. require top1−top2 margin instead of/alongside an absolute cutoff) and re-test ABTT; that is where its measured margin gains should convert into classifications.

## 2026-07-03 CodeRankEmbed ONNX Export Verification

- Goal: replace the unverified community ONNX (`Zenabius/CodeRankEmbed-onnx`) with our own optimum export from the MIT source weights.
- Own-export attempt: `optimum-cli export onnx` (optimum 2.2.0 + `optimum-onnx` + `einops`) fails inside nomic's custom `modeling_hf_nomic_bert.py` (`state_dict_from_pretrained` looks for `pytorch_model.bin`; the repo ships safetensors only). Plain `AutoModel.from_pretrained(..., trust_remote_code=True, safe_serialization=True)` loads fine (136.7M params). Manual `torch.onnx.export` produces a graph that matches torch at the traced length but hard-bakes the rotary-embedding cache (`seqlen > self._seq_len_cached` traced as constant) and fails at any other sequence length. A correct export needs the rotary rewrite the community exporters did; deferred.
- Community fp32 verification (the actual trust requirement): ONNX output vs torch reference at seq 25/355/1024 — max elementwise diff `9e-6`–`2e-4`, mean-pooled cosine `1.000000` at every length. The community fp32 export is faithful; trust concern resolved.
- Community int8 verification: **broken** — pooled cosine vs reference `0.31` (seq 25), `0.09` (355), `0.02` (1024). The quantized file is unusable garbage; deleted from the cache. Any future quantization must be done and verified ourselves.
- Decision: keep using the verified community fp32 file; add "verify custom ONNX against reference embeddings" as standard practice before adopting any export (the int8 file would have silently destroyed comparison quality).

## 2026-07-03 CodeRankEmbed on Cadabra/Cadabra2

- Command: `runs/cadabra-coderank`: full index/embed/compare, `CodeRankEmbed` (verified community fp32 ONNX), max_length `2048`, batch `32`, calibration background positions `0.30`/`0.65`.
- Runtime: `4629` bodies ≈ 75 min CPU embedding overall (resumed once; ~90–100 bodies/min while running clean).
- Result: `3667` records: `3` exact copies, `33` strong matches, `3002` possible matches, `114` splits, `96` merges, `313` possible missing coverage, `106` possible new behavior. Calibration: background mean `0.237 ± 0.107`, top-1 anchor `0.720`, effective candidate `0.382` / match `0.551`.
- Probe outcomes: `Point3.vector_to` covered (merge at `0.956`, correctly paired with its cadabra2 counterpart), `Vec2.length` covered (split at `0.903`), but `Point2.vector_to` only possible (`0.408`) and — importantly — the known false positive `face_count` ↔ counts-aggregation is back as a strong match at `0.628`.
- Key finding: the top-1-p95 calibration anchor assumes the anchor population is true matches. On the high-overlap altium testbed that holds (anchor `0.99`); on a divergent rewrite like cadabra/cadabra2 the anchor collapses to `0.720` and the effective match bar (`0.551`) becomes permissive enough to readmit known false positives. Missing coverage dropping from `1128` (BGE raw) to `313` is therefore partly genuine (CodeRankEmbed + candidate floor `0.382` finds more real candidates) and partly threshold looseness.
- Decision: calibration needs an overlap-robust anchor — candidates: anchor on same-name cross-project pairs (cheap, plentiful, high-precision), clamp effective thresholds to a floor relative to background (e.g. `bg + 4σ`), or make the match criterion margin-aware. Combine with the ABTT conclusion: the classifier upgrade (margin/anchor robustness) is the single highest-value next change; model quality (CodeRankEmbed) is no longer the limiting factor on this corpus.

## 2026-07-07 OSS Single-Project Eval + Benchmark: Candidate Selection

- Goal: run `decombine` single-project duplicate analysis (`index` + `embed` + `analyze`) against prominent open source projects to measure (a) usefulness of the duplicate clusters, (b) false-positive rate via manual inspection, and (c) end-to-end performance, so results double as an optimization benchmark for the implementation and algorithms.
- Selection criteria: prominent projects, one per supported language where possible, forming a size ladder from ~14k to ~135k analyzable code lines so per-phase timings show scaling behavior. Explicitly excluded very large repos (Chromium/Linux-class) that would take days to index/embed on CPU.
- Candidates (shallow clones under `runs/oss-eval/corpora/`, sizes via `tokei`):
  - `flask` (Python): 83 files, ~14.0k code lines — smallest rung, very prominent, mature/refactored codebase so duplicate hits are likely low-signal ground truth for false positives.
  - `express` (JavaScript): 141 files, ~15.9k code lines — includes a large test suite, which should stress test-boilerplate near-duplicate detection.
  - `gin` (Go): 99 files, ~17.9k code lines — Go web framework; Go's error-handling boilerplate is a known generic-similarity trap, good false-positive probe.
  - `ripgrep` (Rust): 100 files, ~37.9k code lines — middle rung, same language as our existing corpora for cross-corpus comparability.
  - `redis` (C, scoped to `src/` only): 133 C files + 83 headers, ~135.8k code lines — largest rung; deliberately excludes `deps/` and TCL tests. C has known intentional duplication patterns (per-type command implementations, e.g. `t_string.c`/`t_list.c`/`t_zset.c` families) that make useful-vs-noise judgments concrete.
- Benchmark protocol per project: release binary, `BGESmallENV15`, CPU, default duplicate-analysis thresholds; record per-phase wall time (`index`, `embed`, `analyze`), units/files/distinct-bodies counts, embed throughput (bodies/s), peak RSS via `/usr/bin/time -v`, and SQLite DB size. Quality protocol: report cluster counts, then manually classify the top ~20 clusters per project as true duplicate / intentional-idiom / false positive.
- Throughput expectation from the 2026-07-03 baseline (~4.5 bodies/s/thread, 16 threads): redis (largest) should embed in well under an hour; the full five-project sweep is a single-session job.
- Configs to be created per project under `runs/oss-eval/<project>/decombine.yaml`.

## 2026-07-07 OSS Eval Sweep Design: Two Model Arms

- Design change from the candidate-selection entry: the sweep runs two arms over all five projects — `BGESmallENV15` (shipped default; headline false-positive rate and performance baseline) and `CodeRankEmbed` (current quality leader from the 2026-07-03 altium ablation, via the cached community fp32 ONNX export, batch 32, max_length 2048).
- Threshold porting: duplicate analysis uses raw cosine thresholds with no calibration, and the defaults (`0.88`/`0.92`/`0.94`) are implicitly tuned to BGE's similarity scale. For the coderank arm they are ported by matching background-relative position using the altium-measured backgrounds (BGE `0.698`, CodeRankEmbed `0.261`): positions `0.60`/`0.74`/`0.80` map to `0.70`/`0.81`/`0.85`. This is a provisional prior — per-corpus threshold sensitivity is itself a finding to record.
- Execution plan for unattended throughput: `runs/oss-eval/sweep.sh` indexes all 10 project/arm databases in parallel (indexing is parser-bound and DBs are independent), then runs embed+analyze strictly serially because ONNX embedding saturates all CPU cores — BGE arm first (fast, results land early), each arm ordered small→large (flask, express, gin, ripgrep, redis).
- Instrumentation: per-phase wall time and peak RSS (`/usr/bin/time -v`) appended to `runs/oss-eval/timings.tsv`; per-phase logs in `runs/oss-eval/logs/`; DB sizes reported at the end; console log at `runs/oss-eval/sweep.log`.
- Smoke check: flask indexed `83` files / `1035` units under the bge config before launch.

## 2026-07-07 OSS Sweep: Benchmark Results (BGE arm complete, CodeRank arm 4/5)

- Harness: `runs/oss-eval/sweep.sh` + `run_phase.py` (wall time + child peak RSS via `getrusage`; `/usr/bin/time` and `bc` do not exist on this machine — first launch failed on that and was fixed). Full timings in `runs/oss-eval/timings.tsv`, logs in `runs/oss-eval/logs/`.
- Index phase (all 10 project/arm DBs in parallel): 12–48 s each, ~43–49 MB RSS, zero failures. Unit counts: flask `1035`, gin `1544`, ripgrep `1742`, express `2679`, redis `4235`.
- BGE embed (batch 256): flask `169 s`, gin `247 s`, ripgrep `285 s`, express `443 s`, redis `677 s` — throughput is flat at `5.5–6.2` bodies/s across corpus sizes, i.e. embedding scales linearly. Peak RSS however was `15.5–16.8 GB` per run, wildly above the 0.6–1.0 GB in the synthetic baseline (`docs/benchmarks.md`) — real-repo long bodies at batch 256 blow up the ONNX memory arena. **Embed peak memory, not speed, is the first optimization target: batch sizing should be token/char-aware, not count-based.**
- CodeRank embed (batch 32, fp32 community ONNX): flask `497 s` (2.0 bodies/s), gin `1155 s` (1.3), ripgrep `1469 s` (1.2), redis `7413 s` (0.57) — per-body throughput degrades with average body length, and RSS ran `21.8–31.3 GB`. **express was OOM-killed (exit -9, 31.3 GB, batch 2 of 32)** — long JS test bodies at max_length 2048; retried with `batch_size 8` + `max_batch_chars 40000` (config comment records this). Same conclusion as BGE but worse: char/token-aware batch packing is required for the coderank path to be usable on real repos.
- Analyze phase is negligible everywhere: `0.4–5.4 s`, ≤62 MB RSS, even for redis (4226 bodies). DB sizes 3–24 MB.
- Cluster counts (candidate pairs / clusters): flask bge `1320/99` vs coderank `1301/86`; gin bge `4024/134` vs coderank `4270/140`; ripgrep bge `5924/188` vs coderank `7587/196`; redis bge `3985/422` vs coderank `4499/358`; express bge `36114/254` (coderank pending retry). The ported coderank thresholds (`0.70/0.81/0.85`) land in the same regime as BGE defaults — the background-relative-position porting rule looks sound at first order.
- First qualitative signal before the full classification pass: express/bge top clusters are dominated by 100-member `<anonymous>` JS callback clusters at raw `1.0000` — exact-copy test boilerplate. Real but low-value; `<anonymous>` naming makes reports unreadable for JS. Gin top clusters similar (anonymous handlers). Flask top clusters look genuinely useful (`add_url_rule` triplication across App/Blueprint/BlueprintSetupState is a real, known duplication).
- Next: manual top-20 cluster classification per report (usefulness / false-positive protocol), plus express coderank retry result.

## 2026-07-07 OSS Sweep: Top-Cluster Quality Classification (9/10 reports)

- Protocol: inspected the top-20 cluster list per report, opened cluster detail pages for every semantic (raw < 1.0) cluster, and verified ambiguous pairs against source. Exact-copy clusters (raw 1.0000) are true duplicates by construction. express/coderank pending (embed retry in progress).
- Pair-level false positives in top-20 are rare in both arms:
  - flask/bge: 1/20 — `Flask.test_cli_runner` (impl) ↔ `test_cli_runner_class` (its test) at `0.9462`. flask/coderank: 0/20.
  - redis/bge: 1/20 — cluster 16 pairs `sentinelFailoverSelectSlave` ↔ `sentinelAbortFailover` at `0.9399`: shared failover state-machine field assignments, but the functions do different things and cannot be merged. redis/coderank: 0/20.
  - express/bge, gin both arms, ripgrep both arms: 0/20 pair-level FPs in the top lists.
  - The one recurring FP *pattern* is implementation ↔ its own test (shared identifiers dominate the embedding): flask top-20 case above, plus express's only three `lib/` product-code matches (ranks 165+, e.g. `lib/request.js` protocol getter ↔ `test/req.protocol.js` at `0.9107`).
- The real usefulness problems are not pair precision:
  1. **Ranking drowns product code in test/docs boilerplate.** flask, express, gin, ripgrep top-20 are dominated by exact-copy doc examples, test fixtures, and mocha/table-test callbacks. express/bge has zero product-code findings in its top 160 clusters. The genuinely valuable flask finding (`Flask`/`Blueprint` `send_static_file`/`open_resource`/`get_send_file_max_age`, and the `add_url_rule` triple) ranks 4–72 depending on arm.
  2. **Transitive cluster chaining over-merges.** gin/bge cluster 5 has 100 members joining unrelated `TestContext*` families; each adjacent pair is true copy-paste but the cluster as a unit is meaningless. coderank's equivalent cluster has 19 members — less chaining at its threshold position.
  3. **`<anonymous>` naming makes JS/Go reports unreadable.** express/gin top clusters read as `<anonymous> × 100`; the enclosing `describe`/`it` string or parent function would fix this.
  4. **Intentional-idiom families inflate counts.** ripgrep's 66-member `Flag::update` cluster (one 4-line trait impl per CLI flag) and the `Matcher` delegation wrappers are correctly matched but have zero refactor value.
- Model comparison: **CodeRankEmbed's semantic clusters are visibly more coherent on C.** redis/coderank top-20 is essentially all real, well-scoped duplication (`genericZrangeby*` family, `zslIsInRange`/`zzlIsInRange` cross-encoding, `publishCommand`/`spublishCommand`, per-platform `aeApiFree`) and it groups `askingCommand`/`readonlyCommand`/`readwriteCommand` into one clean 3-cluster where BGE built a 6-member mixed one. On flask it surfaced the app/blueprint duplication family in the top-20 where BGE did not. The ported thresholds (`0.70/0.81/0.85`) produced comparable cluster volumes to BGE defaults, validating the background-relative porting rule.
- Decisions (ranked): (1) report ranking should down-weight test/docs paths and exact-copy fixture clusters, or split product vs test sections — this is the single biggest usefulness win; (2) add an impl↔test pair suppressor (path heuristic: `tests?/`, `*_test.*`, `test_*`) — kills the only recurring FP pattern; (3) cap or community-detect within clusters to stop transitive chaining; (4) name anonymous units by enclosing call/string context; (5) CodeRankEmbed quality justifies making it usable: quantized export + token-aware batch packing to fix speed and the OOM.

## 2026-07-08 OSS Sweep: express/coderank Retry and Final Tally

- Retry result: `batch_size 8` + `max_batch_chars 40000` embedded the remaining `2414` bodies in `1571.5 s` (`1.54` bodies/s) at `8.4 GB` peak RSS — ~4x below the `31.3 GB` that OOM-killed batch 32. Analyze `3.1 s`; `37368` candidate pairs, `254` clusters. Confirms char-aware batch limiting is the memory fix; a token/char-aware packer should become the default embed behavior rather than a per-config rescue.
- express/coderank top-20: all exact-copy anonymous mocha callbacks, true duplicates, 0 pair-level FPs — same as express/bge. Notable difference: coderank produces **zero** `lib/` product-code clusters, i.e. it does not make the three impl↔its-test false matches BGE made (`req.protocol`, `res.jsonp`, `res.sendStatus`). The code-tuned model separates implementation from test probes; BGE matches them on shared identifiers.
- Final sweep tally (10/10 reports classified): pair-level FP rate in top-20 lists — BGE 2 FPs across 100 inspected clusters (flask impl↔test, redis sentinel state-machine), CodeRankEmbed 0 across 100. Both arms' rankings are dominated by test/docs boilerplate on test-heavy repos; ranking and cluster-chaining fixes (see 2026-07-07 decisions) remain the highest-value work, with CodeRankEmbed usability (quantization + batch packing) the best model-side investment.

## 2026-07-08 Method Changes from OSS Sweep Findings (pre-rerun)

Four changes implemented off the 2026-07-07 sweep decisions, all validated by `cargo fmt`/`clippy -D warnings`/full test suite (golden snapshots regenerated):

- **Token-area batch packer** (`embedding.max_batch_token_area`, default `32_000_000`): embed batches are now packed so `items × longest_item_tokens²` stays under budget — the term ONNX attention memory actually scales with (observed ~0.25 GB per million units on both arms of the baseline sweep). Tokens are estimated as chars/4 clamped to the model's truncation length (512 for fastembed catalog models, `custom.max_length` for custom ONNX). Pages are sorted longest-first so long bodies batch together (small count) and short bodies pack densely. The packed batch is passed to fastembed as a single batch so the budget is authoritative. Predicted effect: BGE batches cap at ~122 long-body items (~8 GB vs 16 GB), coderank-2048 at ~8 (vs the 31 GB OOM at 32).
- **Report sectioning by cluster kind**: units are classified test/docs vs product (`analyze::paths::is_test_or_docs_path`: test/docs/example/bench directory components, `test_*`/`*_test.*`/`*.test.*`/`*.spec.*` file patterns, plus Rust inline `tests` module scopes). Clusters are `Product` / `Mixed` / `TestOrDocs` and the index now leads with product clusters; `Mixed` gets an explicit "implementation matched to its own test is a common false positive" warning header. Ordering within sections is unchanged (boosted score).
- **Semantic chaining bound** (`analysis.max_semantic_cluster_size`, default `16`): union-find merges are now rejected when they would push a cluster past 16 *distinct normalized bodies* — exact-copy growth stays unbounded (many copies of one body are one finding; unit-count cap `max_cluster_size: 100` still bounds page size). Edges process best-first, so strong cores form before weak bridges are dropped. This targets gin's 100-member multi-family `TestContext*` cluster while leaving express's exact-copy callback walls intact.
- **Anonymous unit naming from enclosing calls**: JS/TS functions passed as call arguments are named `callee("first string arg")` — mocha's `it("sets ETag", fn)` becomes `it("sets ETag")`, `app.get("/", fn)` becomes `app.get("/")`; Go `func` literals get the same treatment (`t.Run("case")`). Assignment-context naming is tried first, as before.
- Rerun: baseline artifacts archived to `runs/oss-eval/baseline-2026-07-07/`; all 10 DBs deleted (unit names changed; incremental indexing would not refresh them); express/coderank restored to `batch_size 32` — the config that OOM'd — so the packer is tested on the worst case. Sweep v2 launched with the same protocol.

## 2026-07-08 OSS Sweep v2: Before/After Results

Reran the full 10-run sweep on fresh DBs with the four method changes. Zero failures. Baseline artifacts in `runs/oss-eval/baseline-2026-07-07/`, new timings in `runs/oss-eval/timings.tsv`.

- **The OOM case is fixed**: coderank/express at `batch_size 32` — killed at 31.3 GB in the baseline — now completes in `597.7 s` at `17.9 GB` peak with no per-config rescue.
- **The packer made embedding much faster, not just smaller.** Length-sorted token-area packing eliminates padding waste (uniformly-long batches, densely-packed short batches). BGE embeds: flask `169→72 s`, express `443→168 s`, gin `247→113 s`, ripgrep `285→162 s`, redis `677→474 s` (~2x overall). CodeRank embeds: flask `497→185 s`, gin `1155→387 s`, ripgrep `1469→554 s`, redis `7413→1741 s` (4.3x). CodeRank/redis went from ~2 h to 29 min, making the code model practical on real corpora.
- **Memory**: BGE peak RSS `15.5–16.8 GB → 7.5–7.7 GB` (matches the 32M-area budget prediction) except redis/bge, which stayed at `16.8 GB` — long-line C bodies do not explain it under the clamp, so something besides padded attention (tokenizer or ort arena behavior on this corpus) drives redis's peak; follow-up. CodeRank peaks `8.8–25.8 GB` (was `21.8–31.3` + one OOM): bounded but above the ~8 GB target because non-attention activations scale linearly beyond the quadratic term the budget models; a lower default or a linear term would tighten this.
- **Sectioning works as designed.** Flask/bge product section now leads with exactly the baseline's real findings: `add_url_rule` triple at #1 and the `Flask`/`Blueprint` `send_static_file`/`open_resource`/`get_send_file_max_age` family at #4–6. Gin's product section surfaces the `Bind`/`Render`/`StaticFile` families that were buried under test noise. Express correctly shows **zero** product clusters, and its only product-code matches — the three impl↔test false positives (`req.protocol`, `res.jsonp`, `res.sendStatus`) — sit at the top of the Mixed section under the FP warning. Flask's `test_cli_runner` FP is likewise quarantined in Mixed. Section counts (product/mixed/testdocs): flask bge `25/11/67`, express bge `0/3/310`, gin bge `16/4/128`, ripgrep bge `131/16/64`, redis bge `430/0/0` (src/-scoped, no test dirs). CodeRank again produces zero mixed clusters on flask/express — the code model does not cross the impl↔test boundary.
- **Chaining bound works.** Gin's 100-member multi-family `TestContext*` cluster is gone; max gin cluster is now 19 units (16 distinct bodies + exact copies). Redis's 38-member `hsetnx…` blob is a coherent 15-member hash-command family; the 35-member clusterManager blob is 16. Express's exact-copy callback walls survive intact (exact growth exempt), now readably named.
- **Naming works**: express clusters read `it("should serve static files")`, `app.post("/")`, `before(...)`; gin closures similarly. `<anonymous>` is nearly gone from the JS/Go reports (one residual gin product cluster of Rust-style unnamed closures at #12).
- Residuals for next iteration: (1) redis/bge RSS anomaly; (2) coderank gin peak `25.8 GB` — consider a linear activation term or lower default budget; (3) semantic-similarity FPs between two product units (redis `sentinelResetMaster` family) are untouched by sectioning, as expected; (4) ripgrep's product section is inflated by intentional-idiom families (`Flag::update`) — the cross-directory/generic-helper downrank exists but does not gate the main table.

## 2026-07-08 Code-Embedding Exploration Research

- Task: research future code-exploration methods now that the core duplicate detector is useful, especially whether a user-provided set of functions can define a semantic axis such as "decoder" and rank code units by projection.
- Method: inspected local embedding/analysis architecture (`AnalysisContext`, `VectorStore`, duplicate/concern/compare analyzers, ABTT/calibration, `scripts/embedding_experiments.py`) and ran parallel online research lanes for concept axes, modern code embeddings/retrieval, and structure-aware exploration.
- Observed result: example-derived semantic axes and hybrid retrieval are promising, but they need benchmarked precision/stability gates before becoming product features.
- Decision: moved detailed notes to `docs/research/code-embedding-exploration.md` and kept the roadmap link to `docs/ideas/code-embedding-exploration.md`.
- Follow-up: expanded `docs/research/code-embedding-exploration.md` into the long-form research artifact covering local architecture fit, source links, candidate algorithms, validation shields, query-by-example, hybrid retrieval, relevance feedback, topic maps, and experiment gates.

## 2026-07-08 Name/Code Embedding Consistency Research

- Task: research whether decombine can embed function/method names separately from whole code blocks to detect misleading names, name/function divergence, and naming inconsistencies across a codebase.
- Method: inspected the current extraction/schema/analysis path (`extractor.rs`, `CodeUnitRef`, `code_units.name`, `scope`, `kind`, `display_source`, `embedding_text`, body-hash embeddings) and researched method-name prediction and inconsistent-method-name detection work including code2vec, code2seq, Allamanis et al. method/class naming, MNire, NameChecker, CodeT5, GraphCodeBERT, and recent empirical reassessments of inconsistent-name detection.
- Observed result: the current full-unit embedding likely already includes the declared function name, so raw body/name comparison would be contaminated. The useful research path is multi-channel: name-only embeddings, body-with-declared-name-masked embeddings, signature/interface embeddings, lexical subtoken features, and body-neighbor name distributions.
- Observed result: the most promising v1 analyses are repo-local and explainable: duplicate-cluster naming entropy, body-similar/name-different candidates, same-name/body-different ambiguity clusters, and neighbor-grounded rename suggestions. Direct name/body dot product can be tested, but only with language/kind/name-length/body-size calibration and injected-mismatch benchmarks.
- Decision: documented the detailed research in `docs/research/name-code-embedding-consistency.md` and added it to `RESEARCH.md`. Recommended next experiment is a read-only prototype over existing DBs: compute name subtokens and name-only embeddings, score duplicate-cluster naming entropy plus body-neighbor name divergence, then validate on injected name swaps before reporting real functions as "naming consistency candidates".

## 2026-07-08 Agent Query Interface Research

- Task: research how to add a query interface to the decombine CLI so coding agents can explore indexed codebases themselves across lexical, structural, semantic, duplicate, concern, comparison, and future graph/name-analysis data.
- Method: inspected the current CLI/analyzer/report surface (`cli.rs`, `main.rs`, `AnalysisContext`, `VectorStore`, duplicate/concern/compare analyzers, SQLite schema, Markdown reports) and ran parallel online research on agent-oriented CLI design, code intelligence query systems, and machine-readable CLI output patterns from ripgrep, jq, SQLite, Sourcegraph, CodeQL, Semgrep, Tree-sitter, ast-grep, LSP/SCIP/LSIF, OpenGrok/Hound, Qdrant, LanceDB, and Chroma.
- Observed result: the right interface is a typed query family, not one giant DSL: `query capabilities`, `query inspect`, `query units`, `query text`, `query ast`, `query search`, `query similar`, `query qbe`, `query graph`, `query explain`, plus later query packs. Agents need stable JSON/JSONL, byte ranges, revision/index metadata, result IDs, pagination, score breakdowns, and skipped/non-exhaustive-result explanations more than human-friendly output.
- Observed result: the lowest-risk implementation path is to add structured JSON serializers for existing analyzer outputs first, especially `compare --output json`, then `analyze duplicates --output json` and `analyze concerns --output json`. That establishes stable IDs, schema versions, paging, and explanation fields before adding new algorithms.
- Decision: documented the detailed research in `docs/research/agent-query-interface.md` and added it to `RESEARCH.md`. Recommended roadmap: (1) JSON output for existing analyzers; (2) `query capabilities` and `query inspect`; (3) semantic search/QBE over `AnalysisContext`; (4) YAML query packs combining lexical/vector/metadata/AST retrieval; (5) Tree-sitter structural search; (6) optional symbol/reference/call graph facts later.

## 2026-07-08 GPU/NPU Backend Research

- Task: research the next optimization, GPU/NPU acceleration, with the goal of widest practical support across GPUs, NPUs, and vendor stacks while keeping the CPU default stable.
- Method: spawned four parallel research lanes: local codebase attachment points, ONNX Runtime execution-provider coverage, Rust-native/cross-platform GPU alternatives, and packaging/CI rollout guardrails. Also inspected `fastembed 5.17.2`, `ort 2.0.0-rc.12`, `Cargo.toml`, `src/embed/*`, `src/config.rs`, `docs/packaging.md`, `architecture.md`, and current ONNX Runtime/Windows ML docs.
- Current repo finding: `embedding.execution_provider` already validates `cpu`, `cuda`, `coreml`, `directml`, and `openvino`, and `ModelIdentity` already persists provider identity, but `FastembedBackend::new` hard-fails any provider except `cpu`. The low-churn hook is therefore provider-aware embedding inside the existing `Embedder`/`fastembed` path, not an analyzer rewrite.
- Observed result: ONNX Runtime execution providers are the right first portability layer; Rust-native GPU stacks and GPU/ANN vector search should be deferred behind benchmark gates.
- Decision: moved detailed notes to `docs/research/gpu-npu-backends.md`. Keep `cpu` default; implement experimental `directml`, `cuda`, `coreml`, and `openvino` provider plumbing first; benchmark provider drift before changing defaults.

## 2026-07-08 Closure Call-Context Naming: Rust Adapter + Go/JS Residuals

- Change: new `rust` language adapter names closures from their let binding (`let clamp = |v| ...` → `clamp`) or the call they are passed to (`aliases.sort_by_key(|a| ...)` → `aliases.sort_by_key(...)`), reusing `name_from_call_argument`. Go adapter extended with assignment naming (`handler := func...`, `var defaultLogFormatter = func...` — note the func literal sits under an `expression_list` inside `var_spec`/`short_var_declaration`) and an enclosing-function fallback (`BasicAuthForRealm.func`) covering returned closures and `go func(){...}()` IIFEs. JS got the same enclosing-function fallback (`shouldHaveHeaderValues.func`).
- Helper hardening driven by real-corpus eyeballing: callee text is whitespace-collapsed so multiline method chains qualify; segment trimming is bracket-depth-aware (`callee_tail`) so receivers with parenthesized args never split mid-paren (was producing `glob).map_err(...)`); a leading `self.` is stripped; string labels strip `b`/`r`/`#` sigils and are dropped if a quote survives (was producing `captures_iter("b"aa bb cc dd")`).
- Validation: extraction goldens extended with the new shapes (Rust let-closure, Go returned closure + go-IIFE) and regenerated; fmt/clippy/full tests clean.
- Real-corpus validation without re-embedding: embeddings are keyed by `(model_id, normalized_body_hash)`, so `DELETE FROM files; DELETE FROM code_units;` + re-index refreshed unit names with `embedded 0 new bodies`. (`touch` does not work — the indexer falls back to a content-hash check. CLAUDE.md corrected. Beware: the sqlite3 CLI has foreign_keys OFF, so `DELETE FROM files` alone orphans `code_units` — delete both.)
- Result: `<anonymous>` count is now **zero** in gin, ripgrep, and express reports, both arms (was: gin 39, ripgrep 60 mentions across v2 report files). Cluster hashes, ordering, unit counts, and scores are byte-identical to sweep v2 — naming is display-only, so no ranking side effects. Names read well: `HyperlinkFormat.aliases.sort_by_key(...)`, `BasicAuthForRealm.func`/`BasicAuthForProxy.func` (gin product cluster 12), `TestRunEmpty.func` family, `defaultLogFormatter`.
- Decision: TODO residual "extend call-context naming to Rust closures" is done; JS/Go/Rust now share the same three-tier naming (assignment → call argument → enclosing function).

## 2026-07-08 Idiom-Family Downrank + `#[cfg(test)]` Reclassification

- Change 1: `Cluster.name_family` — a cluster with ≥6 members, ≥3 distinct scopes, and ≥75% of members sharing one unit name is flagged as a same-name impl family (`Flag::update`, `Display::fmt`, `Serialize::serialize`) and ordered after all other clusters in its section (membership, hashes, and scores untouched). Index rows get "— same-name family (`name`), likely idiom"; cluster pages get an explanatory header. Constants in `analyze/duplicate/mod.rs`, no config knob. The 6-member floor is deliberate: flask's real `add_url_rule` triple (3 members, 3 scopes, same name) must not be flagged.
- Change 2: the Rust adapter gives `#[cfg(test)]` functions a `tests` scope (skipping doc comments between attribute and fn; `#[cfg(not(test))]` excluded), so ripgrep's top-level `#[cfg(test)] fn test_*` style — no `mod tests` — classifies as test code.
- Validation: two new analysis tests (family flagged + ordered last despite higher boosted score; no flag without scope diversity, protecting scopeless C) and one extractor test; fmt/clippy/full suite clean; report goldens unchanged.
- ripgrep/bge result: product section 131 → 119 clusters; 12 flag-test clusters (`test_block_buffered`…, now displayed `tests.test_*`) moved to Test-and-docs, 2 left Mixed. The seven `update`/`fmt`/`serialize` walls (9–16 members each) now sit at product ranks 113–119 with idiom markers; the top of the product table is real cross-name findings. Guard checks: flask `add_url_rule` still product #1, unflagged; redis flags nothing (C units have no scopes); gin flags exactly one family (9-member `Bind`, sinks to product #16); gin's 5-member `Render` family stays unflagged (below the member floor).
- redis/coderank note: analyze on HEAD yields 359 clusters vs 358 in the sweep-v2 log — deterministic on re-run and present before these changes (drift from commit a7f9dc7's post-sweep fixes); naming/idiom changes were verified structure-identical on gin/ripgrep/express.
- Decision: TODO residual "down-rank intentional-idiom families" is done via ordering + labeling rather than score penalties, so nothing is hidden and the ignore workflow still applies. If real same-name duplication ≥6 members shows up flagged on a future corpus, revisit with an exact-copy exemption.

## 2026-07-08 Product↔Product Semantic FP Gate: Negative Result

- Question: can a content-level signal over the embedding geometry (the TODO's "margin/coherence gate within clusters") separate redis's sentinel failover state-machine family (14 units in `sentinel.c`, pairwise `0.91–0.94` BGE, known false positive) from true duplicate families?
- Method: `scratchpad prominence.py` probes over the existing oss-eval DBs. Per probe cluster: top/mean in-cluster cosine, local background (members' mean cosine to same-file non-members), top out-of-cluster cosine; prominence = top_in − local_bg; out-margin = top_in − top_out. Probes: sentinel family (FP) vs redis `zrange` family, `zslIsInRange` family, `publishCommand`/`spublishCommand`, flask `add_url_rule`, gin `Bind` (all TP/idiom-but-real-duplication). Run on BGE, CodeRank, and ABTT-transformed (m=2) BGE vectors.
- BGE result: prominence FP `0.218` vs TPs `0.171–0.330` — the FP sits inside the TP range (zrange TP is `0.216`). Out-margin is worse: FP `0.077` while the `zslIsInRange` TP has `0.001`. Absolute score: the FP pair (`sentinelFailoverSelectSlave`↔`sentinelAbortFailover`, `0.9399`) outscores the `publishCommand` TP (`0.9099`).
- CodeRank result: same picture — prominence FP `0.520` vs TPs `0.507–0.585`. Only weak signal: sentinel mean in-cluster cosine `0.578` is below every TP family (`0.686–0.939`), i.e. CodeRank sees the family as less *internally coherent*; but the same statistic on BGE (`0.859` vs zrange `0.883`) does not separate, and BGE is the default model.
- ABTT (m=2, BGE) result: sentinel top_in `0.882` lands between TPs (`0.912`, `0.832`, `0.701`) — no separation; ABTT even drops a real TP below the FP.
- Conclusion: **no cluster-statistic over these embeddings separates this FP shape** — the human judgment ("state-machine orchestration, not mergeable logic") is not present in the vectors' local geometry. Do not implement a margin/coherence gate; it would cost true positives one-for-one.
- Decision: drop the gate idea from TODO. Realistic paths if this FP class matters later: (1) CodeRank as analysis model (its sweep-v2 top-20 never surfaced the family — a ranking effect, and its coherence stat is at least directionally right); (2) non-embedding features (AST statement-type histograms, call-graph overlap) as a separate rerank signal — different feature family, new experiment.

## 2026-07-08 Exact Tokenizer Counts in the Batch Packer + Budget Recalibration

- Root-cause measurement (`scratchpad token_check.py`, real tokenizers over each corpus's unit texts): the packer's chars/4 token estimate undershoots true counts on **every** OSS corpus — true/est length ratio p50 `1.27–1.62`, padded-area ratio flask `1.22–1.27`, express `1.28–1.48`, redis `1.33–1.43`, ripgrep `1.43–1.55`, gin `1.91–2.29`. This directly explains coderank/gin's `25.8 GB` (2.29x the 32M budget ≈ 73M true area). It does *not* alone explain redis/bge `16.8 GB` (redis 1.33x < gin/bge 1.91x, yet gin/bge peaked 7.7).
- Change: `Embedder::count_tokens` — fastembed's `TextEmbedding.tokenizer` is a public field, so the backend now returns exact (truncation-clamped) counts; `pack_batches` takes `(hash, text, tokens)` and pages sort by real token count. Fallback stays chars/4 for backends without a tokenizer. Also added token-length instrumentation to `embed` output: p50/p90/p99/max lengths, truncation count/rate, padding waste (new `TokenStats`).
- Re-measurement (release, `run_phase.py`):
  - coderank/gin @32M: `25.8 GB → 6.1 GB`, `387.7 → 305.3 s`. The budget now holds; **no linear activation term needed** — the overshoot was entirely estimate error.
  - redis/bge @32M: `16.8 → 10.7 GB`, `474 → 443 s` — improved but still the highest BGE run.
  - Budget bisect on redis/bge: @16M → `4.49 GB` at `424.1 s` (faster than 32M). Peak tracks budget cleanly ⇒ **no ort-arena pathology; the redis "anomaly" is closed**: redis is simply the only corpus whose batches consistently saturate the area budget (13.2% of bodies at the 512 clamp, p90 = 512; other corpora rarely fill a batch to cap), amplified by the old under-count.
  - coderank/gin @16M: `5.4 GB` at `279.8 s` — also faster than 32M.
- Decision: `embedding.max_batch_token_area` default lowered `32M → 16M` — Pareto-better (time AND memory) on both budget-saturating extremes; on CPU, larger batches buy nothing. Analyze reruns on the re-embedded DBs reproduce identical cluster counts (redis/bge 430, gin/coderank 152), confirming batch composition does not affect results.
- Follow-up: `runs/oss-eval/timings.tsv` embed rows predate exact counting; refresh on the next full sweep. Padding waste is now printed per run (redis/bge 25–31%) — worth revisiting only if profiling shows it matters.

## 2026-07-08 Execution-Provider Backend Plumbing (Stage 0)

- Scope: implement the provider plumbing from `docs/research/gpu-npu-backends.md`
  and the packaging decision in `docs/research/release-architecture.md` (one
  crate, backends as cargo features, CPU stays a single static binary). This
  is code + config + doctor only — no accelerator artifacts built or run
  (no GPU/NPU on the dev box).
- Config: new `embedding.provider_mode` (`require` default | `auto`).
  `require` fails loudly when a requested non-cpu provider is missing or
  unavailable; `auto` warns and falls back to cpu. Env overrides applied in
  `Config::load` before validation: `DECOMBINE_EXECUTION_PROVIDER=<name>`
  and `DECOMBINE_DISABLE_ACCEL=1` (disable wins). Starter template documents
  both keys.
- Backend: `FastembedBackend::new` no longer hard-fails non-cpu. New
  `resolve_providers` returns `(Vec<ExecutionProviderDispatch>, actual)`;
  cpu ⇒ empty list. `build_accelerator` (accel-gated) maps a name to an
  `ort::ep` EP, checks `is_available()` + `supported_by_platform()`, and
  marks the dispatch `error_on_failure()` in `require` mode. The **actual**
  provider (cpu after a fallback, not the requested accelerator) is what the
  model identity records — so gpu/cpu embeddings never collide on one DB row.
  EPs are passed to both the catalog and custom-model init paths.
- Cargo: `ort = "=2.0.0-rc.12"` added as an *optional* direct dep (matches
  fastembed's pin, so `ort/cuda` etc. unify onto the one shared instance).
  Features `cuda`/`directml`/`coreml`/`openvino` (each ⇒ `accel` ⇒ `dep:ort`
  + matching `ort/<ep>`), plus `load-dynamic`. Default build unchanged:
  `ldd` still shows no libonnxruntime, CPU statically linked.
- Doctor: `decombine doctor` now prints ort/fastembed versions, the effective
  provider + mode, and per-accelerator status (`accelerator_diagnostics()` in
  `embed/mod.rs` — compiled? / ORT-available? / platform-supported?).
  `doctor --provider <name>` runs a live embedding smoke test and prints the
  provider actually in effect.
- Verified on the CPU (default) build: `cargo fmt`/`clippy` clean on both
  default and `--features accel`; full suite 70+ tests pass; `--features
  accel` type-checks. Runtime: default doctor lists all four accelerators
  "not compiled in"; `require`+cuda errors with rebuild instructions before
  any model download; `auto`+cuda warns, falls back, smoke-embeds on cpu with
  "provider in effect: cpu"; `DECOMBINE_DISABLE_ACCEL=1` forces cpu over a
  `cuda` env request; a bad provider name still hits config validation.
- Untested (no hardware): actual EP registration/inference for cuda/directml/
  coreml/openvino, and the `load-dynamic` build (needs `--no-default-features`
  to avoid double-linking ORT). Next stages per the research docs: build a
  DirectML experimental artifact + CI, then the benchmark gate (vector drift,
  top-k overlap, duplicate/comparison output drift vs cpu).

## 2026-07-08 CodeRank Quantization Utility and Custom Artifact Identity

- Change: custom ONNX model identity now records content hashes for the ONNX
  file (`ModelIdentity.model_hash`) and tokenizer/config files
  (`ModelIdentity.tokenizer_hash`) instead of relying only on the custom path
  string in `revision`. This protects fp32/int8 custom artifacts from sharing a
  DB model row if a file is replaced at the same path.
- Change: added `scripts/coderank_onnx.py` with three artifact-prep commands:
  `build-corpus` creates stratified calibration/holdout JSONL from retained
  decombine DB embedding texts, `quantize` runs ONNX Runtime static int8 QDQ
  quantization and emits `decombine-model-manifest.json`, and `verify` gates
  fp32 ONNX vs Torch plus candidate ONNX vs fp32 ONNX using the TODO thresholds
  (pooled cosine, pairwise-delta, and top-10 recall).
- Validation: `python3 -m py_compile scripts/coderank_onnx.py`;
  `scripts/coderank_onnx.py --help`; `cargo test
  custom_artifact_hashes_track_model_and_tokenizer_bytes`.
- Observed result: script syntax and CLI surface are valid; the focused Rust
  unit test passes and confirms ONNX byte changes affect `model_hash` while
  tokenizer/config byte changes affect `tokenizer_hash`. No real CodeRank int8
  artifact was generated or verified in this pass.
- Decision: keep quantization/Torch verification in a source-controlled Python
  utility rather than adding Python ML dependencies to the Rust CLI. Runtime
  config continues to point at concrete custom ONNX files; production config
  cleanup remains a TODO until at least one verified int8 artifact exists.

## 2026-07-08 Case-study Corpus Selection Research

- Task: choose the first per-language open-source repositories for public
  decombine case studies, plus intentionally messy AI-assisted / rapid-build
  stress cases such as OpenClaw.
- Method: inspected supported languages from `assets/languages/`; queried
  GitHub metadata on 2026-07-08 with `gh api "repos/<owner>/<repo>" --jq
  '[.full_name,.language,(.license.spdx_id // "NOASSERTION"),.stargazers_count,.size,.pushed_at,.html_url] | @tsv'`;
  cross-checked OpenClaw's license page because GitHub API returned
  `NOASSERTION` while the repo contains an MIT license file.
- Observed result: the defensible public breadth matrix is Valkey, fmt,
  PowerShell, Gin, Express, Spring Boot, OkHttp, Laravel, Flask, Rails,
  ripgrep, and TypeScript. Current `redis/redis` metadata is not a permissive
  SPDX result, so Redis should stay internal or be pinned to a manually cleared
  permissive-era commit; Valkey is the public C replacement. OpenClaw is the
  right first AI-assisted stress target, but it needs pinned-SHA license review
  and unsupported-language excludes before publication.
- Decision: added `docs/research/case-study-corpus-selection.md`, linked it
  from `RESEARCH.md`, and amended the case-study pipeline note so the target
  matrix and Redis/Valkey caveat are explicit before any public corpus run.

## 2026-07-08 Per-Language Untruncated Token Measurement + Silent-Truncation Audit

- Task: quantify how much the embedding token cap silently truncates code
  units, per language, before choosing a mitigation (collapse_trivia vs
  chunking). Motivated by the 512-token cap on the BGE default reading as an
  unaudited, silent quality hole.
- Change: added a `tokens` subcommand + `embed::token_report`. It scans every
  distinct indexed unit body (regardless of embedding state, so it works on
  already-embedded DBs), recovers text from source under report/minimal
  retention via the existing `recover_texts_from_source` path, and counts true
  token lengths with an **untruncated** tokenizer clone
  (`Embedder::count_tokens_untruncated`; fastembed clones its inference
  tokenizer with `with_truncation(None)` so severity past the cap is visible —
  `count_tokens` clamps at the cap and hid it). Reports per-language
  p50/p90/p99/max and over-cap counts at 512 and the model's own cap.
- Method: ran `decombine --config <coderank.yaml> tokens` (CodeRank cap 2048)
  across the OSS-eval ladder (redis=C, flask=Python, express=JS, gin=Go,
  ripgrep=Rust) plus cadabra and altium (Rust).
- Observed result (units | p50 | p90 | p99 | max | >512 | >2048):
  - C (redis):        4226 | 161 | 613 | 1952 | 3298 | 13.1% | 0.8%
  - Python (flask):   1003 |  83 | 245 |  539 |  888 |  1.4% | 0.0%
  - JS (express):     2446 |  83 | 293 | 1305 | 3010 |  6.1% | 0.4%
  - Go (gin):         1494 |  89 | 278 |  857 | 2002 |  3.0% | 0.0%
  - Rust (ripgrep):   1706 | 109 | 329 |  817 | 2126 |  4.6% | 0.1%
  - Rust (cadabra):   4625 | 130 | 590 | 1757 | 3176 | 12.5% | 0.5%
  - Rust (altium):     326 | 148 | 386 | 1087 | 1145 |  6.7% | 0.0%
- Findings:
  - The BGE 512 cap is a real, previously-silent loss: 1.4% (Python) up to
    ~13% (C, heavy Rust) of units are truncated, worst on large real
    codebases. Truncation rate is driven more by codebase heft than language
    (ripgrep Rust 4.6% vs cadabra Rust 12.5%).
  - The CodeRank 2048 cap nearly eliminates it: 0.0-0.8% over 2048, max
    observed ~3300 tokens, and p99 of the heaviest corpora (redis 1952,
    cadabra 1757) sits just under 2048 — 2048 is a well-chosen cap.
  - collapse_trivia's value *for truncation* is marginal under CodeRank (the
    problem is already <1%); its payoff would be throughput/padding, a separate
    axis. The residual 0.1-0.8% genuinely-huge units are the chunking case
    (TODO: long-unit handling), not a trivia problem.
- Decision: strong quantitative support for making CodeRank (2048) the default
  over BGE (512) — the switch closes a 5-13% silent-truncation hole on the
  common heavy-codebase case. Truncation is no longer silent regardless of
  model: `tokens` audits it and `embed` already prints truncated%. Defer
  collapse_trivia as a throughput lever, not a truncation fix. Long-unit
  chunking stays scoped to the <1% tail.

## 2026-07-08 CodeRankEmbed as Default Model (Managed Auto-Download + Full Switch)

- Task: act on the truncation audit — make CodeRankEmbed (2048-token context)
  the default over BGESmallENV15 (512), which requires the CodeRank ONNX to
  reach users without hand-placement.
- Decision (user): auto-download on first use with SHA256 verification; full
  switch (model + ported thresholds), BGE kept selectable.
- Change:
  - New "managed model" concept in `config.rs` (`MANAGED_MODELS`,
    `managed_model()`): a name (`CodeRankEmbed`) backed by pinned HF files
    (`Zenabius/CodeRankEmbed-onnx`, 5 files with SHA256 + size). Validation and
    `EmbeddingConfig::dimensions()` accept managed names with no `custom` block;
    `quantized` rejected for them.
  - `fastembed_backend`: when `model` is managed and no `custom` block is set,
    materialize files into `<cache>/custom/<id>` (download via `ureq` streamed
    to `<file>.part` with streaming SHA256, promoted only on match; existing
    files verified and skipped), then load through the existing custom ONNX
    path. Identity revision is path-independent (`managed:<repo>@<rev>`) unlike
    an explicit custom block's absolute-path revision. `ureq` added as an
    optional dep gated on the `fastembed` feature.
  - `models list` now shows managed models; `models download` already routes
    through backend construction so it triggers the fetch.
  - Defaults flipped: `model` BGESmallENV15 → CodeRankEmbed; duplicate
    thresholds 0.88/0.92/0.94 → 0.70/0.81/0.85 (background-relative port).
    CONFIG_TEMPLATE + architecture.md + benchmarks.md updated; BGE documented
    as the lightweight no-download alternative.
  - Tests decoupled from product defaults: both `analysis_config()` helpers
    (analysis.rs, report_golden.rs) pinned to the historical BGE thresholds so
    the hash-backend tests and golden fixtures are stable across the flip.
- Validation: `cargo fmt --check`, `cargo clippy --all-targets --features
  fastembed -D warnings`, `cargo test` (73 lib + integration, all pass),
  `cargo check --no-default-features` (ureq/managed code correctly
  feature-gated). End-to-end offline (box already has the cached files, so the
  verify-and-skip path ran, not a network download): `doctor --provider cpu`
  smoke embed → 768 dims; full `run` on a 2-function corpus → indexed →
  embedded 3 bodies → analyzed → report, DB identity
  `managed:Zenabius/CodeRankEmbed-onnx@main` dims 768.
- Not covered: the actual network download path (no network in this env) — the
  streaming-download + hash-gate logic is exercised only via the skip-when-
  present branch; a clean-machine download remains to be smoke-tested.
- Decision/follow-ups: comparison raw-threshold defaults left at BGE scale
  (comparison is opt-in and has background calibration; its threshold porting
  belongs with the Robust Comparison Calibration block). Recalibration
  baselines and a clean-machine download test are open.

## 2026-07-08 Robust Comparison Calibration (overlap-robust anchors, sigma floors, margin gate)

- Task: TODO block #4 — make `comparison.calibration: background` robust on
  low-overlap corpora, where the top1_p95 anchor collapses and the position-
  mapped match bar sinks enough to readmit false positives.
- Change (`src/analyze/compare/mod.rs`, `config.rs`, `report/markdown.rs`):
  - Config knobs: `calibration_anchor` (top1_p95|same_name|hybrid, default
    hybrid), `min_same_name_anchors` (3), `candidate_sigma_floor` (2.0),
    `match_sigma_floor` (4.0), `strong_min_margin_sigma` (0.0, opt-in).
  - Same-name anchor: median cosine of unambiguous same-name/same-kind/same-
    language cross-project pairs (a rename proxy that does not collapse when few
    left units have real counterparts). `hybrid` = max(top1_p95, same_name)
    when count ≥ min, else top1_p95.
  - Sigma floors: effective candidate/match clamped to `bg + kσ` (calibration
    can only tighten). Surfaced in the report with which floor bound.
  - Margin gate: strong match requires mutual-best + raw ≥ match_threshold +
    top1−top2 margin ≥ `strong_min_margin_sigma·σ` in BOTH directions; near-ties
    fall to possible. Split/merge exempt; gate needs background calibration.
  - 3 new unit tests (same-name selection + ambiguity exclusion, floor clamp,
    margin demotion); existing calibration tests pinned (floors 0 on the
    bimodal synthetic backgrounds). fmt + clippy + full test suite clean.
- Validation (CodeRank, positions 0.30/0.65, robust vs legacy=top1_p95+no floors):
  - cadabra (low-overlap, 4.6k units/side): top1_p95 anchor collapses to 0.72
    (the flagged pathology); legacy match bar 0.5507 classifies the known false
    positive `face_count`↔`TopologyStore.counts` (raw 0.6278) as STRONG. Robust
    bg+4σ floor lifts the match bar to 0.6652 → demotes it to POSSIBLE. True
    probes stay covered: `Point3/Point2.vector_to` strong(6)/split(4)/merge(2),
    `Vec2.length` split(1)/merge(7). Spurious many-to-many also shrank
    (splits 114→55, merges 96→39; clean strong 33→40).
  - altium (high-overlap, 326/side): renames stay strong in every arm
    (`parse_params↔parse_entries`, `preserves_duplicate_keys↔…`); the third
    (`read_footprint_data↔decode_pcb_record`) is possible in both — no
    regression. hybrid kept top1_p95 (0.993 > same-name 0.948, n=18).
  - candidate_sigma_floor tuned 3.0→2.0 on evidence: at 3σ the candidate floor
    over-culled recall (altium possible 56→20, missing 7→46; cadabra possible
    1404, missing 2463) with NO change to strong/split/merge (those are match-
    gated). At 2σ, strong/split/merge identical (altium 13, cadabra 40/55/39)
    while recall recovers near legacy (altium possible 43/missing 23; cadabra
    possible 2938/missing 929). So the match floor is the precision lever and
    the candidate floor is a pure recall/noise guard — 2σ is Pareto-better.
- Decision: ship robust defaults (hybrid anchor, candidate 2σ / match 4σ floors,
  margin gate off). All TODO #4 success criteria met on both testbeds. Margin
  gate left opt-in (no evidence it was needed once floors fix the match bar; it
  could demote true renames if set too high). Comparison thresholds now port
  across models via calibration; the earlier "port comparison raw defaults"
  follow-up is effectively subsumed by recommending `calibration: background`.

## 2026-07-08 Compare-Phase Performance (parallel bounded top-k, scan reuse)

- Task: TODO block #5 — the compare phase was the slow path (debug cadabra
  compare, ~4.6k units/side, ran >10 min).
- Change (`src/analyze/vector_store.rs`, `src/analyze/compare/mod.rs`):
  - `top_k_between` is now parallel over `from` (rayon; queries are
    independent) with bounded top-k selection — it keeps only the running best
    k via `partition_point` insert instead of collecting every above-threshold
    hit and sorting the whole row. A `-1.0`-threshold scan (calibration) no
    longer materializes/sorts a full row. `pair_ranks_before` reproduces the
    old (score desc, `to`-index asc) order exactly.
  - Calibration scan reuse: under background calibration the left→right pass is
    scanned once unpruned; the top-1 anchor is read from it and the candidate
    edges are that same pass filtered by the calibrated threshold (filtering an
    unpruned top-k by a threshold == the thresholded top-k). Full cross-product
    scans drop 3 → 2.
  - Edge construction no longer does an O(n) `pending_left.contains(...)` per
    emitted pair — the direction is known per pass (`top_k_between` guarantees
    `a`=from, `b`=to), so left/right are assigned directly.
- Validation: behavior-preserving by construction; all 24 comparison unit
  tests pass; and on the full cadabra CodeRank corpus the new code reproduces
  the exact section counts validated for block #4 (strong 40, possible 2938,
  splits 55, merges 39, missing 929; `face_count`↔`counts` stays possible),
  deterministic across reruns and thread counts (non-timestamp report files
  byte-identical).
- Benchmark (cadabra CodeRank, 4.6k units/side, `run_phase.py` wall/RSS):
  - release, all 16 cores: 0.9 s / 68 MB.
  - release, 1 thread (`RAYON_NUM_THREADS=1`): 8.6 s → ~9.5x from parallelism.
  - debug, all cores: 25.7 s (was >10 min pre-#5; the O(n) contains removal and
    the 3→2 scan reduction dominate the extra gain in unoptimized builds).
- Decision: exact cross-product search is NOT a bottleneck (sub-second release
  on the largest standing testbed), so ANN/HNSW stays deferred per the TODO
  guard. `top_k_between` remains O(n²) in dot products but is now parallel,
  allocation-light, and scanned the minimum number of times.

## 2026-07-09 Execution-Provider Drift Gate (CPU-vs-accelerator equivalence)

- Task: TODO "Accelerated Execution Providers" — the Stage-0 EP plumbing (cargo
  lanes, `ort` dispatch, `resolve_providers`, `doctor` smoke test) already
  shipped; the missing piece is a correctness gate proving a non-CPU provider
  produces embeddings equivalent to the CPU baseline before its build is
  trusted. Chosen because it is fully testable on this CPU-only box.
- Change: `src/analyze/drift.rs` + `decombine drift --baseline A.db
  --candidate B.db`. Compares the embeddings stored under each database's model
  for their shared body hashes (model/provider-independent hashes align the two
  sets), at two levels:
  - vector: per-body cosine distribution (mean/p50/p05/min) + max abs
    per-component delta on normalized vectors;
  - structural: mean top-k nearest-neighbour recall (the signal duplicate
    detection depends on), reusing the parallel `top_k_between`.
  Gate flags `--min-cosine` (default 0.9999) and `--min-recall` (0.99); a
  failing gate exits non-zero (CI-friendly). New `db.all_embeddings(model_id)`.
  3 unit tests (identical → zero drift; small perturbation → cosine <1 but
  neighbourhoods preserved; shared-hash-only + dim-mismatch error).
- Validation (CPU, real DBs):
  - self-vs-self (altium BGE vs itself): cosine mean/min 1.000000, max Δ 0,
    top-10 recall 1.0000 → PASS, exit 0.
  - cross-model (altium BGE vs AllMiniLML6V2, same source, both 384-dim):
    cosine mean 0.191 / min 0.056, recall 0.607 → gate FAILED, exit 1. Confirms
    the gate detects real divergence; the ~0.61 recall across two unrelated
    models is the partially model-agnostic code-similarity structure.
- Not covered: an actual CPU-vs-accelerator run (no GPU on this box). The
  machinery is proven (passes identical, fails divergent); a real CUDA/DirectML
  build just needs to embed the same corpus and run `drift` against the CPU DB.
- Decision: the drift gate is the intended CI guardrail for accelerator builds.
  Default thresholds (cosine 0.9999 / recall 0.99) suit same-model
  cross-provider fp32, where near-identity is expected; loosen for int8/quantized
  accelerator paths.
