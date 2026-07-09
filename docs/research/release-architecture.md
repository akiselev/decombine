# Release architecture and backend packaging

Date: 2026-07-08

## Question

How do we architect distribution as more embedding backends arrive: cargo
install with feature flags, per-OS suffixed artifacts, or `decombine-*`
bin crates invoked like cargo subcommands? Constraint: prebuilt GitHub
release binaries so people and cloud agents install in seconds.

## Facts established

- The current release build **statically links ONNX Runtime**: `ldd
  target/release/decombine` shows no libonnxruntime, nothing is copied next
  to the binary, and the result is one self-contained ~49 MB file. (The
  "linked/copied next to the binary" note in `docs/packaging.md` is
  outdated for the CPU path.) A zero-dependency single-file default
  artifact is therefore already free.
- `ort` has a `load-dynamic` cargo feature: the binary loads *any*
  ONNX Runtime dylib at runtime via `ORT_DYLIB_PATH` or `ort::init_from()`
  instead of linking at compile time. One compiled binary can use a CPU,
  CUDA, DirectML, or OpenVINO ORT build depending on which dylib it is
  pointed at. fastembed 5.17 exposes this as `ort-load-dynamic`.
- fastembed 5.17 feature map: `ort-download-binaries` (default),
  `ort-load-dynamic`, `directml` (= `ort/directml`); its `cuda`/`metal`/
  `mkl`/`accelerate` features are **candle** features for the
  HF-native models (`qwen3`, `nomic-v2-moe`), not ORT EPs. Other ORT EP
  registration features (`ort/cuda`, `ort/coreml`, …) aren't re-exported,
  but we can enable them ourselves with a direct `ort` dependency — cargo
  feature unification applies them to the shared `ort` crate.
- `dist` (cargo-dist) is alive and maintained (releases/issues through
  2026): generates the GitHub Actions release workflow, per-target
  tarballs with standard triple naming, shell + PowerShell installers,
  and cargo-binstall-compatible artifacts.

## Decision

**One crate, one binary name, backends as cargo features, per-target
GitHub release artifacts built by `dist`.** No `decombine-*` subcommand
crates.

Rationale against the alternatives:

- **Subcommand bin crates** (`decombine-cuda` called out to like cargo
  subcommands) suit independently useful tools. Our backends share the
  entire scanner/extractor/DB/analyzer/report pipeline and the model
  identity machinery; separate bins would duplicate ~90% of the binary,
  multiply the CI matrix, and complicate the one-DB-per-model-identity
  rule for zero user benefit.
- **`cargo install --features …`** stays supported (it works today) but is
  the fallback channel, not the primary one: minutes of compile plus a
  build-time ORT download is exactly what the cloud-agent constraint
  rules out.

## Artifact lanes

### v1: CPU default (single file, static ORT)

Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. All are hosted GitHub
runners now (`ubuntu-24.04-arm` covers aarch64 Linux). Add
`x86_64-apple-darwin` only if asked; skip musl for now (static ORT +
musl is unproven and glibc covers the cloud-agent case).

Install paths, all off the same release:

1. `curl … | sh` installer (dist-generated) — the cloud-agent path.
2. `cargo binstall decombine` — resolves to the same artifacts by naming
   convention.
3. `cargo install` from source — fallback.

Runtime model download stays first-use; CI/agent images pre-seed with
`decombine models download` or by copying the cache dir (already
documented in packaging.md).

### v2: one `-accel` artifact per platform via load-dynamic

Instead of one artifact per (platform × EP) — the naive reading of the
lane plan in [gpu-npu-backends.md](gpu-npu-backends.md) — build a single
extra variant per platform with `fastembed/ort-load-dynamic` plus the ORT
EP registration features (`ort/cuda`, `ort/directml`, `ort/coreml`,
`ort/openvino`; EP features compile without the vendor runtime present).
It contains no ORT at all; at startup it resolves an ONNX Runtime dylib
from config/`ORT_DYLIB_PATH`, and the EPs available are whatever that
dylib was built with.

- Keeps the artifact matrix linear in platforms, not platforms × EPs.
- Vendor runtimes (CUDA/cuDNN etc.) were always the user's problem
  anyway; official Microsoft ORT GPU releases are the documented source.
- Optional sugar later: `decombine accel install cuda12` downloads the
  official ORT release into the cache dir, same trust model as model
  downloads.
- `decombine doctor embedding --provider X` (per gpu-npu-backends.md)
  is the support tool for this lane.

The provider policy config (`provider_mode: require|auto`), identity
recording, and staged provider rollout from gpu-npu-backends.md are
unchanged by this; load-dynamic only changes *packaging*, not plumbing.

### Candle-family backends

fastembed's `qwen3`/`nomic-v2-moe` (candle) models slot into the same
scheme: a cargo feature on the default or `-accel` artifact, selected via
the existing `embedding.backend`/model config, identity-checked by the
DB as today. Decide inclusion per-model on binary-size cost; no new
distribution mechanism needed.

## Naming

Standard dist/binstall convention:
`decombine-v<ver>-<target-triple>.tar.xz` (`.zip` on Windows), plus
`decombine-accel-v<ver>-<target-triple>.tar.xz` in v2. No ad-hoc OS
suffixes; installers and binstall parse triples.

## Blockers before first public release

- **License is still undecided** (`Cargo.toml` has `publish = false` and
  an open note). A GitHub release requires choosing one; upstream Slopo
  is AGPL and untouched, but our own license choice gates everything.
- macOS/Windows builds have never been produced or smoke-tested
  (PLAN.md Phase 10); dist's CI matrix is the cheapest way to get them.

## Rollout order

1. Choose license; set `license` + repo metadata.
2. Add `dist` config; get CPU artifacts for the four targets green in CI,
   smoke-test mac/Windows binaries (index + embed a fixture).
3. Tag v0.x, publish release with installers; verify `curl | sh` and
   `cargo binstall` cold-install times.
4. v2: `-accel` variant with load-dynamic + provider plumbing, doctor
   command, staged per gpu-npu-backends.md.
