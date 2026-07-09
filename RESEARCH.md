# Research

This index tracks durable research notes that are too detailed for
`EXPERIMENTS.md`. The experiment log should keep the method, observed result,
and decision; these child files keep the wider background, option tables, and
rollout guardrails.

## Current Notes

- [GPU/NPU Backend Support](docs/research/gpu-npu-backends.md) - execution-provider strategy for broad accelerator support across GPUs, NPUs, and vendor stacks.
- [Code-Embedding Exploration](docs/research/code-embedding-exploration.md) - detailed research on semantic axes, query-by-example, hybrid retrieval, relevance feedback, and topic-map experiments for future code exploration features.
- [Name/Code Embedding Consistency](docs/research/name-code-embedding-consistency.md) - research on separate name/code embedding channels, misleading-name candidates, naming inconsistency reports, and local rename-suggestion experiments.
- [Agent Query Interface](docs/research/agent-query-interface.md) - research on a machine-readable query CLI for coding agents, including JSON/JSONL schemas, stable selectors, explainable results, query packs, and phased command design.
- [Case-study Corpus Selection](docs/research/case-study-corpus-selection.md) - per-language repository matrix for public case studies, plus scale alternates and AI-assisted rapid-build stress targets such as OpenClaw.
- [Quantized Model Distribution](docs/research/quantized-model-distribution.md) - measured quantization results (int8 rejected, fp16 shippable) and the upload+pin workflow for distributing compressed models via the managed-model mechanism.

## Policy

- Keep benchmark commands, corpus results, and final decisions in
  `EXPERIMENTS.md`.
- Put long-form research synthesis in `docs/research/`.
- When a research note leads to an implementation experiment, add the concrete
  run and result back to `EXPERIMENTS.md`.
