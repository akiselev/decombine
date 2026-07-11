# Operations: models, health checks, drift, token audit, DB maintenance

## Models

```sh
decombine models list        # managed + catalog models with dims/context
decombine models download    # pre-download the configured model into cache
```

- `CodeRankEmbed` (default): 2048-token context, ~550 MB, downloaded and
  SHA256-verified on first use. Best quality; use for real analyses.
- `BGESmallENV15` and other fastembed catalog models: 512-token context —
  they silently truncate long functions (~5–13% of units on heavy C/Rust
  codebases), which weakens matching on exactly the large units you care
  about. Fine for quick/lightweight runs.
- Custom ONNX models go under `embedding.custom` (dir, onnx_file,
  dimensions, pooling, max_length). Verify any custom/quantized export
  against reference embeddings before trusting results — broken exports
  produce plausible-looking garbage.

## Model/database binding (important)

A database is permanently bound to one embedding model identity
(model, version, provider, quantization, file hashes — all of it).
Consequences:

- Changing `embedding.*` against an existing DB fails with an immutability
  error. The fix is a fresh `db_file` per model arm, not editing settings.
- Upgrading the decombine binary or model files can change the identity
  (e.g. tokenizer/model hashes appear); `query search` and
  `analyze concerns` will then refuse with a field-by-field diff of what
  changed. Re-embed into a fresh DB to proceed.

## Doctor: health check

```sh
decombine doctor                    # config, projects, model, runtime, accelerators
decombine doctor --provider cuda    # + live embedding smoke test on that provider
```

Run this first when anything model- or provider-related misbehaves.

## Token audit: find silent truncation

```sh
decombine tokens
```

Per-language token-length distribution (p50/p90/p99/max) over all indexed
units, plus how many exceed 512 and the configured model's cap. High
over-cap counts mean the model never sees the end of those functions —
switch to a longer-context model rather than trusting weak matches.
`decombine embed` prints the same stats for newly embedded bodies.

## Drift gate: compare two embedding runs

```sh
decombine drift --baseline cpu.db --candidate gpu.db
```

Compares embeddings shared by two databases (per-body cosine distribution,
max component delta, top-k neighbor recall) and **exits non-zero** if
drift exceeds the gates (`--min-cosine`, default 0.9999; `--min-recall`,
default 0.99). Use it to validate accelerator builds, quantized models, or
toolchain upgrades against a CPU baseline before adopting them.

## Incremental indexing and cheap refreshes

- `index` skips unchanged files (mtime/size, then content hash). `touch`
  does not force re-extraction.
- Extraction changes (unit naming, queries, adapters) therefore do NOT
  refresh already-indexed units. Cheap refresh without re-embedding:

  ```sh
  sqlite3 decombine.db "DELETE FROM files; DELETE FROM code_units;"
  decombine index && decombine embed
  ```

  Embeddings are keyed by (model, body-hash) and survive — only changed
  bodies are re-embedded.
- Embedding saturates all CPU cores; serialize embed runs. Peak RSS is
  bounded by `embedding.max_batch_token_area` (default 16M ≈ 8 GB); lower
  it on small machines — on CPU, smaller budgets are also *faster*, so
  don't raise it chasing throughput.

## Misc

- `decombine show-config` prints the effective config after defaults.
- `decombine languages list` shows bundled languages and enabled state.
- A markdown `analyze duplicates` run wipes and rewrites the report dir;
  `--json` mode writes nothing to disk.
