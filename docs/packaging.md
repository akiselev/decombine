# Packaging and distribution

## Release builds

```sh
cargo build --release
```

The release profile enables thin LTO, a single codegen unit, and symbol
stripping (see `Cargo.toml`). The result is a single self-contained binary
plus the ONNX Runtime shared library handling described below.

Platform status:

- **Linux (x86_64)**: built and tested — this is the development platform.
- **macOS / Windows**: expected to build (all dependencies are
  cross-platform: bundled SQLite, vendored Tree-sitter parsers, `ort`
  downloads platform-appropriate ONNX Runtime binaries at build time), but
  release binaries have not been produced or tested yet. Producing and
  smoke-testing those builds is release work, tracked in PLAN.md Phase 10.

## ONNX Runtime distribution

`fastembed` depends on `ort`, which by default uses its `download-binaries`
build feature: the ONNX Runtime library for the target platform is fetched
at **build** time. For the default CPU build it is **statically linked** —
`ldd target/release/decombine` shows no `libonnxruntime`, and the result is
a single self-contained binary. Decision for v1:

- keep build-time download (no runtime network dependency, no system
  ONNX Runtime requirement);
- CPU stays statically linked (single-file artifact, the default release);
- accelerators are separate feature builds (see below), and the
  `load-dynamic` feature exists for a future single "-accel" artifact that
  loads any ONNX Runtime dylib at runtime via `ORT_DYLIB_PATH`.

Building the default `decombine` therefore needs network access once per
target; the produced binary runs offline.

## Model cache and offline use

Embedding models are downloaded on first use of `decombine embed` (or
explicitly with `decombine models download`) into, in order of precedence:

1. `embedding.cache_dir` from `decombine.yaml`
2. `$FASTEMBED_CACHE_DIR`
3. the OS cache directory (`$XDG_CACHE_HOME/decombine/models`, or
   `~/.cache/decombine/models` on Unix-like systems)

No API keys or accounts are needed; models come from public Hugging Face
repositories. After the first download, indexing, embedding, and analysis
all run fully offline. The effective cache path is persisted in the
database as part of the model identity.

To pre-seed an offline machine, copy the cache directory and set
`embedding.cache_dir` (or `FASTEMBED_CACHE_DIR`) to its location.

## Feature flags

- `fastembed` (default): the local ONNX embedding backend. Building with
  `--no-default-features` produces a smaller binary that can index and
  analyze but errors on `embed` with instructions to rebuild; the
  deterministic hash backend remains available to tests.
- `cuda`, `directml`, `coreml`, `openvino`: layer the matching ONNX Runtime
  execution provider onto the shared `ort` build. Each is its own artifact
  lane; none are in `default`, so the CPU binary stays clean. The EP
  registration code compiles without the vendor runtime present, but the
  vendor runtime (CUDA/cuDNN, OpenVINO, …) must be installed to *run*.
- `accel`: umbrella feature the four above imply; brings `ort` into scope
  without selecting an EP. Rarely used directly.
- `load-dynamic`: load ONNX Runtime from a dylib at runtime (`ORT_DYLIB_PATH`)
  instead of linking it in — the basis for a single "-accel" artifact whose
  available providers come from whichever ORT build it is pointed at. Build
  with `--no-default-features --features load-dynamic,fastembed` so the
  static download-binaries path does not also link in.

## Execution providers

Select an accelerator with `embedding.execution_provider` (`cpu` default, or
`cuda` | `directml` | `coreml` | `openvino`). `embedding.provider_mode`
controls what happens when the requested provider cannot be used:

- `require` (default): fail with a clear error — the provider is either not
  compiled into this binary, or ONNX Runtime reports it unavailable.
- `auto`: warn once and fall back to CPU.

CPU fallback is never silent: the model identity records the provider
**actually** used, so GPU and CPU embeddings never share a database row (a
DB is bound to one model identity). Environment overrides, applied before
validation:

- `DECOMBINE_EXECUTION_PROVIDER=<name>` overrides the configured provider.
- `DECOMBINE_DISABLE_ACCEL=1` forces `cpu` (wins over the variable above).

`decombine doctor` reports the ONNX Runtime version, which accelerator
features are compiled in, and their live availability. `decombine doctor
--provider <name>` runs an embedding smoke test with that provider and
prints the provider that actually took effect.

## Common errors

| Symptom | Cause / fix |
| --- | --- |
| `this binary was built without the fastembed feature` | Rebuild with default features. |
| `loading fastembed model ...` network error | First run needs internet; pre-seed the cache for offline machines. |
| `execution provider "cuda" is not compiled into this binary` | This is a CPU (or different-accelerator) build; use the `cuda` artifact, rebuild with `--features cuda`, or set `provider_mode: auto` to fall back. |
| `execution provider "cuda" is not available` | Compiled in, but ONNX Runtime/this platform can't offer it — install the vendor runtime, or fall back. |
| `unknown language id` | Check `languages.enabled` against `decombine languages list`. |
| `setting ... is fixed once the database is created` | Immutable index/embedding settings changed; delete the `.db` file to reindex. |
