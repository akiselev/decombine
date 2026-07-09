# Quantized / compressed model distribution

How a smaller-than-fp32 model reaches users. The mechanism already exists — the
same **managed model** path that auto-downloads the default `CodeRankEmbed`
(`config::MANAGED_MODELS`, verified from Hugging Face by pinned SHA256). Shipping
a compressed variant is therefore *upload + one data entry*, no new code.

## What compresses acceptably (measured 2026-07-09, see EXPERIMENTS.md)

CodeRankEmbed is nomic-bert; verification is the whole game. Gate: int8-vs-fp32
mean cosine ≥ 0.999, min ≥ 0.995, p95 pairwise Δ ≤ 0.005, max Δ ≤ 0.02, top-10
recall ≥ 0.98 (`scripts/coderank_onnx.py verify`).

| variant | size | mean cos | top-10 recall | verdict |
| --- | --- | --- | --- | --- |
| fp32 (baseline) | 548 MB | — | — | ship (default) |
| int8 static (minmax, QDQ) | 139 MB | 0.569 | 0.493 | **reject** — broken |
| int8 dynamic, per-tensor | 138 MB | 0.925 | 0.804 | **reject** — degraded |
| int8 dynamic, per-channel | 139 MB | 0.039 | 0.044 | **reject** — catastrophic |
| **fp16** | **275 MB** | **0.999998** | **0.9983** | **ship** — near-lossless |

int8 is not viable for this model at the quality bar (matches the earlier
community-int8 finding). **fp16 halves the download** with no measurable quality
loss. On CPU fp16 throughput is neutral (ORT upcasts to fp32); the wins are
download/storage size and GPU inference (the accelerator lanes).

fp16 recipe (deterministic; no calibration corpus needed):

```python
import onnx
from onnxruntime.transformers.float16 import convert_float_to_float16
m = onnx.load("onnx/model.onnx")
onnx.save(convert_float_to_float16(m, keep_io_types=True), "out/onnx/model.onnx")
# copy tokenizer.json, config.json, special_tokens_map.json, tokenizer_config.json unchanged
```

`keep_io_types=True` keeps the int64 inputs / fp32 outputs, so the artifact
loads through the unchanged fastembed custom path.

## Distribution steps (upload + pin)

1. **Produce + verify** the artifact locally (recipe above, then
   `scripts/coderank_onnx.py verify --candidate-dir … --torch-reference ""`
   against a language-balanced holdout). Do not upload anything that fails the
   gate.
2. **Upload** the 5 files (`onnx/model.onnx` + the 4 tokenizer files) to a
   Hugging Face repo we own, e.g. `decombine/CodeRankEmbed-fp16`. This is the
   only step that needs HF credentials — it is a maintainer action, not
   something the CLI does:
   ```
   huggingface-cli upload decombine/CodeRankEmbed-fp16 <local-dir> .
   ```
3. **Pin** the uploaded files by SHA256 in a new `MANAGED_MODELS` entry
   (`src/config.rs`). Reuse the CodeRankEmbed entry as the template — same
   dims/pooling/max_length, new `name`/`cache_id`/`repo` and hashes. Integrity
   is gated by the per-file SHA256, so pin a commit `revision` when possible.
4. Users select it with `embedding.model: CodeRankEmbedFp16`; the managed path
   downloads + verifies + loads it exactly like the fp32 default. `models list`
   shows it automatically.

The current local fp16 artifact (`~/.cache/decombine/custom/coderankembed-fp16`,
pending upload) hashes to:

```
onnx/model.onnx           274797617  8a79adde61e2375c05007de89c208d7beac61d62bd5b2605a0418569ac428b45
tokenizer.json               711649  91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854
config.json                    1525  5ff856a41d0f53ef2d74520627d464bd75c2efd8f26f381bd528654895c29b6c
special_tokens_map.json         695  5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a
tokenizer_config.json          1417  7809f768ee3614618b3f1b91dcbfab4f6a9d4b79fb1ad5d17feb65a7c1bb5b7a
```

(The tokenizer/config bytes are identical to the fp32 export — only the ONNX
weights change — so the fp16 repo can share those four files verbatim.)

## Open

- The fp16 ONNX is produced but **not yet uploaded** (needs an HF repo +
  credentials) and the `MANAGED_MODELS` entry is not yet added (would 404 until
  the upload exists). Both are a small follow-up once the repo is created.
- fp16-on-GPU throughput is unmeasured here (CPU-only box) — worth a number once
  an accelerator lane exists, since that is fp16's compute win.
