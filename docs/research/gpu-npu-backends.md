# GPU/NPU Backend Support Research

Date: 2026-07-08

## Question

What is the next optimization path for GPU/NPU support if the goal is the
widest practical hardware support while keeping the CPU default stable?

## Method

Spawned four parallel research lanes:

- local codebase attachment points
- ONNX Runtime execution-provider coverage
- Rust-native and cross-platform GPU alternatives
- packaging, CI, and rollout guardrails

Also inspected local repo files and dependencies:

- `fastembed 5.17.2`
- `ort 2.0.0-rc.12`
- `Cargo.toml`
- `src/embed/*`
- `src/config.rs`
- `docs/packaging.md`
- `architecture.md`

External sources consulted:

- ONNX Runtime execution-provider docs:
  <https://onnxruntime.ai/docs/execution-providers/>
- Rust `ort` execution-provider docs:
  <https://docs.rs/ort/latest/ort/ep/index.html>
- ONNX Runtime releases:
  <https://github.com/microsoft/onnxruntime/releases>
- Windows ML execution-provider docs:
  <https://learn.microsoft.com/en-us/windows/ai/new-windows-ml/supported-execution-providers>
- FastEmbed Rust package/docs:
  <https://crates.io/crates/fastembed>
- Rust `ort` crate/docs:
  <https://docs.rs/ort>

## Current Repo Fit

`decombine2` already has the right high-level shape:

- `embedding.execution_provider` validates `cpu`, `cuda`, `coreml`,
  `directml`, and `openvino`.
- `ModelIdentity` already persists execution-provider identity.
- The production embedder is `FastembedBackend`, wrapping
  `fastembed::TextEmbedding`.
- The current runtime blocks acceleration by hard-failing any provider except
  `cpu` in `FastembedBackend::new`.
- Custom local ONNX loading already exists through fastembed's user-defined
  model path, so CodeRankEmbed-style models do not require a new model-loading
  architecture.

The lowest-churn implementation hook is provider-aware embedding inside the
existing `Embedder`/`fastembed` path. It is not an analyzer rewrite.

Vector search should stay separate. Current analysis loads SQLite vectors into
`VectorStore` and runs exact cosine/top-k scans. Recent OSS runs showed
`analyze` was negligible compared with embedding, so GPU/ANN vector search is
not the first optimization target.

## Main Recommendation

Use ONNX Runtime execution providers as the first portability layer.

Reasons:

- ONNX Runtime is already in the dependency tree through `fastembed` and `ort`.
- `fastembed` already accepts execution-provider lists in its init options.
- `ort` exposes Rust bindings for the relevant EPs.
- ONNX Runtime has the broadest practical hardware coverage for local ONNX
  embedding models.
- The existing model identity and DB schema already have a place to record the
  selected provider.

Do not adopt Burn, CubeCL, raw `wgpu`, or Vulkan for transformer inference now.
They may become useful for custom kernels or future native model paths, but they
would make decombine own too much model/runtime correctness too early.

## Provider Priority

1. `cpu`

   Keep as the default, required fallback, and benchmark baseline.

2. `directml`

   Best first Windows accelerator artifact. It covers DirectX 12-class GPUs
   across NVIDIA, AMD, Intel, and Qualcomm/Adreno machines. It has the least
   vendor-specific user-install burden on Windows, though Windows ML is the
   newer Microsoft direction for new Windows AI work.

3. `cuda`

   Best high-throughput NVIDIA path on Linux/Windows for users who already have
   NVIDIA runtime stacks. Operational pain is CUDA/cuDNN major-version
   alignment and dynamic-library search paths.

4. `coreml`

   Best Apple Silicon path. Keep macOS-only and explicit. It is the natural
   route to Apple GPU/Neural Engine acceleration.

5. `openvino`

   Best Intel CPU/GPU/NPU path. Valuable for Intel laptops/desktops and newer
   NPU machines. It should be packaged separately because OpenVINO support
   tracks vendor runtime releases.

6. `tensorrt`

   Later NVIDIA power-user mode. Pair with CUDA fallback because ORT may assign
   unsupported nodes to CUDA. Expect engine compilation/cache behavior and
   extra support burden.

7. `migraphx`

   Later AMD Linux path. Prefer this over legacy ROCm EP. Treat as
   operationally heavier than DirectML.

8. `qnn`

   Later Qualcomm/Windows ARM or Android path. Practical HTP/NPU acceleration
   usually means quantized models, static/fixed shapes, and platform-specific
   packaging.

9. `nnapi`

   Android-only. Defer unless Android becomes a target.

10. `webgpu`

   Treat as a separate web/JS path, not native CLI embedding.

## Packaging Strategy

Do not ship one binary with every accelerator.

Recommended artifact lanes:

- `decombine` - CPU default
- `decombine-directml` - Windows DirectML
- `decombine-coreml` - macOS CoreML
- `decombine-openvino` - Intel OpenVINO
- `decombine-cuda12` - later NVIDIA CUDA artifact

This matches the dependency reality: accelerator EPs often need vendor runtime
libraries, version alignment, and platform-specific dynamic libraries. A single
"all EPs" binary increases startup and support failures for users who only need
CPU.

## Config and UX

Add provider policy, not just provider name:

```yaml
embedding:
  execution_provider: directml
  provider_mode: require # require | auto
```

Semantics:

- `cpu` remains default.
- `require` fails clearly if the requested provider cannot initialize.
- `auto` warns once and falls back to CPU.
- CPU fallback must never be silent in experiment output or model identity.
- Add environment overrides:
  - `DECOMBINE_EXECUTION_PROVIDER=cpu`
  - `DECOMBINE_DISABLE_ACCEL=1`

Provider registration and node fallback are separate concepts. ORT can use a
priority list where an EP is first and CPU is last, but a failed provider
initialization must be surfaced differently from a graph node falling back to
CPU.

## Doctor Command

Add a smoke path before real accelerator benchmarks:

```sh
decombine doctor embedding --provider directml
```

It should report:

- compiled features
- `ort`/ONNX Runtime version
- provider feature availability
- dynamic library load result
- provider registration result
- selected device
- model session creation result
- one real embedding batch
- actual provider recorded in `ModelIdentity`

Do not trust "provider is available" alone. The test should create a real
session with the selected embedding model or a tiny representative ONNX model.

## Model and Batch Guardrails

- Record actual provider used in `ModelIdentity`; never let GPU and CPU
  embeddings silently share identity.
- Treat quantized catalog models as CPU-only until each model/provider pair is
  proven. FastEmbed notes that CPU-optimized quantized models can fail on GPU
  paths.
- Keep batch memory knobs provider-scoped. GPU OOM should suggest lowering
  `batch_size` or `max_batch_token_area`; it should not silently switch provider
  unless `provider_mode: auto` is set.
- Expect different providers to produce small floating-point drift. Benchmark
  downstream duplicate/comparison drift, not just embedding throughput.

## Rust-Native Alternatives

### Candle

Keep as a selective backend for models that are not clean ONNX fits, especially
Qwen-style safetensors/HF-native models. It is useful, but it expands the
backend identity surface because decombine would own more model and pooling
logic.

### Burn / CubeCL

Good future research track for cross-vendor native GPU work, but not first.
ONNX import/operator coverage and generated-model maintenance make it a larger
commitment than ORT EPs for this workload.

### wgpu / Vulkan

Not recommended for transformer inference. Reasonable only for isolated vector
kernels after a benchmark proves those kernels matter.

### tract

Watch, but do not switch. It may be useful if ONNX Runtime packaging becomes
unacceptable, but the current `fastembed`/`ort` path fits better.

### SQLite-local vector extensions

Keep `sqlite-vec`/`vec1` or vectorlite as future benchmark candidates when
exact vector search becomes a bottleneck. Current evidence points to embedding,
not analysis, as the optimization target.

## Staged Rollout

### Stage 0: CPU Baseline Hardening

- Keep CPU default.
- Add provider identity assertions.
- Add doctor command design.
- Do not build accelerator artifacts yet.

### Stage 1: DirectML Experimental Artifact

- Windows-only feature/artifact.
- Support `embedding.execution_provider: directml`.
- Support `provider_mode: require|auto`.
- Validate session creation, one embedding batch, and fallback messaging.

### Stage 2: CoreML and OpenVINO Experimental Artifacts

- CoreML for macOS arm64 first.
- OpenVINO for Intel CPU/GPU/NPU with explicit device selection such as `CPU`,
  `GPU`, `NPU`, or `AUTO`.
- Keep both experimental until CI and benchmarks are stable.

### Stage 3: Benchmark Gate

Use fixed corpora, fixed model cache state, and fixed config. Compare CPU vs
provider on:

- cold load time
- warm load time
- embeddings/sec
- peak RSS and VRAM when available
- failure rate and failure text
- vector drift
- top-k neighbor overlap
- duplicate/comparison output drift

### Stage 4: CUDA Artifact

- Add only after an NVIDIA runner exists.
- Pin CUDA/cuDNN major line.
- Publish exact runtime prerequisites.
- Keep as opt-in artifact such as `decombine-cuda12`.

### Stage 5: Stable Acceleration UX

Promote a provider only when it has:

- reproducible install
- reliable doctor output
- no output regressions beyond defined cosine/top-k tolerance
- documented fallback behavior
- at least one maintained CI or hardware test lane

## CI Strategy

- CPU: normal required GitHub Actions.
- Feature compilation: build provider features without running hardware tests
  where possible.
- Provider smoke: self-hosted Windows DirectML, macOS CoreML, Intel OpenVINO,
  and NVIDIA CUDA runners. Make them nightly/non-blocking first.
- Regression set: compare CPU vs provider embeddings on a small checked-in
  corpus; assert dimensions, normalization, finite vectors, stable identity,
  and top-k neighbor overlap.
- Packaging CI: verify each artifact starts on a machine without the provider
  runtime and emits a useful error or falls back only under `auto`.

## Decision

Implement provider plumbing behind `fastembed` first for `directml`, `cuda`,
`coreml`, and `openvino`. Require explicit provider selection. Report actual
registered/used provider. Keep CPU as fallback but never hide fallback in
experiment output.
