# Benchmarks and calibration (baseline)

Baseline measurements from 2026-07-03 on the development machine
(Linux/WSL2, 16 cores, 32 GB RAM), release build, CPU execution provider.
Reproduce with:

```sh
cargo build --release
scripts/benchmark.py --binary target/release/decombine \
    --models BGESmallENV15 BGEBaseENV15 JinaEmbeddingsV2BaseCode \
    --sizes 200 1000
```

## Synthetic fixture results

The harness generates a repo with 10 known scattered near-duplicate pairs
(same logic, renamed identifiers/fields/comments), 5 exact-copy pairs, and
N filler functions drawn from 4 rotating templates.

| Model | Units | Index s | Embed s | Analyze s | Embed RSS MB | DB MB | Dup pairs found | Filler template groups |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| BGESmallENV15 | 230 | 3.2 | 17.6 | 0.05 | 618 | 0.75 | 15/15 | 4 |
| BGESmallENV15 | 1030 | 18.8 | 39.6 | 3.6 | 954 | 3.1 | 15/15 | 4 |
| BGEBaseENV15 | 230 | 10.5 | 91.7 | 0.16 | 1332 | 1.2 | 15/15 | 4 |
| BGEBaseENV15 | 1030 | 36.6 | 194.5 | 2.0 | 2024 | 5.2 | 15/15 | 4 |
| JinaEmbeddingsV2BaseCode | 230 | 6.6 | 93.7 | 0.19 | 2024 | 1.2 | 15/15 | 4 |
| JinaEmbeddingsV2BaseCode | 1030 | 41.0 | 258.3 | 1.7 | 2873 | 5.2 | 15/15 | 4 |

Notes on reading the table:

- Indexing is model-independent; the per-row variation is filesystem noise
  and one-file-per-function fixture overhead. Real repos index much faster
  per unit (see below).
- "Filler template groups" counts clusters that join filler functions from
  the same generator template. Because fillers repeat 4 templates with only
  name/comment changes, these are *true* near-duplicates, and every model
  correctly finds exactly the 4 template groups — the fixture does not
  produce false positives to discriminate models on.
- Embed RSS is cumulative child peak including ONNX Runtime; treat as an
  upper bound.

## Real-repository data point

Two related production Rust codebases (a solid-modeling kernel and its
rewrite) indexed into one database and compared:

- 592 files, 4 902 units, 4 625 distinct bodies
- index: 48 s (cold cache, 20 GB of `target/` trees to skip)
- embed (BGESmallENV15, CPU, 16 threads): 17 min (~4.5 bodies/s/thread)
- compare (exact top-k over 4145×757 cross pairs): 4.5 s
- classification: 3 exact copies, 55 strong matches, 57 splits, 39 merges,
  2 718 possible matches, 1 128 possible-missing, 238 possible-new
- spot checks: exact copies were genuinely copied helper traits; top strong
  matches were true ports (`Point3.vector_to`, `Cone3.new` → `Cone.new`);
  splits correctly grouped a v1 `Vec2.length` against the v2 family of
  `length`/`length_squared` methods.

## Default model decision

**Default (2026-07-08 onward): `CodeRankEmbed`.** The token-truncation audit
(EXPERIMENTS.md 2026-07-08) showed BGE's 512-token cap silently truncates
5-13% of units on heavy real codebases; CodeRankEmbed's 2048-token context
drops that to <1%. It is a managed model (downloaded + hash-verified on first
use). `BGESmallENV15` stays selectable as the lightweight, no-download option
and remains the baseline in the tables below.

**Historical default: `BGESmallENV15`.** On these fixtures every candidate
found all known duplicates, so quality did not yet justify a slower model:

| | BGESmallENV15 | BGEBaseENV15 | JinaEmbeddingsV2BaseCode |
| --- | --- | --- | --- |
| Dims | 384 | 768 | 768 |
| Embed speed | 1× | ~4.9× slower | ~6.5× slower |
| Peak memory | 1× | ~2.1× | ~3.0× |
| DB size | 1× | ~1.7× | ~1.7× |
| Download size | ~130 MB | ~440 MB | ~640 MB |
| Code-specific training | no | no | yes |

Caveat: the synthetic fixture is easy (rename-level edits). A future
quality pass should use harder real-world pairs — restructured control
flow and cross-language ports — before treating this as settled;
`JinaEmbeddingsV2BaseCode` is the candidate to promote if code-specific
quality wins there. Cross-language comparison quality is **unmeasured**;
do not advertise cross-language parity checks yet (PLAN.md open decision).

## Exact-search limits and the ANN trigger

Exact all-pairs cosine at 1 030 units takes 3.6 s (release, 16 threads);
cost grows quadratically, projecting to roughly 6 min at 10 k units and
against 100 k units becoming impractical (~10 h scale). Embedding time
dominates far below that point (17 min for 4.6 k bodies on CPU).

Trigger point for `SimilarityIndex` ANN work (Phase 9): repositories above
**~20 k embedded units** or exact `analyze` wall time above ~60 s. Below
that, exact search stays the deterministic default. The `hnsw_rs` sidecar
rebuilt from SQLite embeddings is the first candidate; it must reproduce
exact-search clusters within an agreed recall tolerance on these fixtures
before being enabled.

## Threshold calibration notes

Defaults are now candidate 0.70 / similarity 0.81 / rerank 0.85 for the
`CodeRankEmbed` default, ported from the BGE values by background-relative
position (altium backgrounds: BGE 0.698, CodeRank 0.261). Raw cosine
thresholds do not transfer across models; raise all three to the BGE values
below when selecting `BGESmallENV15`.

The BGE calibration (candidate 0.88 / similarity 0.92 / rerank 0.94): on the
fixtures, rename-level duplicates score ≥0.95 and
unrelated code scores ≤0.85, so the current gap cleanly separates them
while letting the distance boost rescue far-apart pairs in the 0.88–0.92
band. Comparison defaults (candidate 0.78 / match 0.86) produced useful
strong-match precision on the real-repo run; the large possible-match band
is expected for two codebases sharing a domain vocabulary.

## 2026-07-08 addendum: token-area batch packing

The tables above predate the token-area batch packer and overstate embed
cost and memory. Since `embedding.max_batch_token_area` (default 32M,
packing `items × longest_item_tokens²` with length-sorted pages):

- Embed throughput improved 2–4x from reduced padding waste: on the OSS
  eval ladder (`runs/oss-eval/`), BGE/redis 677→474 s, CodeRank/redis
  7413→1741 s (4 226 bodies), CodeRank/flask 497→185 s.
- BGE embed peak RSS dropped from ~15.5–16.8 GB to ~7.5–7.7 GB (exception:
  redis stayed ~16.8 GB — open investigation, see TODO.md). CodeRank
  (max_length 2048) peaks 8.8–25.8 GB, bounded but above target; the
  budget lacks a linear activation term.
- Per-body embed rates: BGE ~6 bodies/s → ~13–14 bodies/s on small/medium
  corpora; CodeRank 0.6–2.0 → 2.4–5.4 bodies/s.

Current end-to-end numbers live in `runs/oss-eval/timings.tsv` and the
2026-07-08 entries in `EXPERIMENTS.md`.

### Exact tokenizer counts in the packer (same day, follow-up)

The packer initially estimated tokens as chars/4, which real tokenizers
undershoot by 1.3–2.3x in padded area on the OSS corpora (Go worst). The
packer now asks the model's own tokenizer (`Embedder::count_tokens`), which
made the budget honest:

- CodeRank/gin: 25.8 GB → 6.1 GB peak at 32M, 387 → 305 s.
- BGE/redis: 16.8 GB → 10.7 GB peak at 32M, 474 → 443 s.
- The default budget is now **16M**, which measured *faster and smaller*
  than 32M on both extremes (BGE/redis 424 s / 4.5 GB; CodeRank/gin
  280 s / 5.4 GB) — peak RSS tracks the budget cleanly, closing the redis
  RSS anomaly (its long-body profile simply saturates the budget: 13% of
  bodies at the 512-token clamp).
- `embed` now prints token-length percentiles, truncation rate, and padding
  waste after each run.
