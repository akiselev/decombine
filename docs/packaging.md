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
at **build** time and linked/copied next to the binary. Decision for v1:

- keep build-time download (no runtime network dependency, no system
  ONNX Runtime requirement);
- revisit static linking or a bundled feature if binary distribution
  demands a single file.

Building `decombine` therefore needs network access once per target; the
produced binary runs offline.

## Model cache and offline use

Embedding models are downloaded on first use of `decombine embed` (or
explicitly with `decombine models download`) into, in order of precedence:

1. `embedding.cache_dir` from `decombine.yaml`
2. `$FASTEMBED_CACHE_DIR`
3. `.fastembed_cache` in the working directory

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

## Common errors

| Symptom | Cause / fix |
| --- | --- |
| `this binary was built without the fastembed feature` | Rebuild with default features. |
| `loading fastembed model ...` network error | First run needs internet; pre-seed the cache for offline machines. |
| `execution provider "cuda" is not compiled into this binary` | v1 ships CPU-only inference; set `embedding.execution_provider: cpu`. |
| `unknown language id` | Check `languages.enabled` against `decombine languages list`. |
| `setting ... is fixed once the database is created` | Immutable index/embedding settings changed; delete the `.db` file to reindex. |
