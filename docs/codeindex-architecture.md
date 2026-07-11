# Codeindex workspace architecture

The reusable code-intelligence substrate is split by dependency and change
boundary rather than by command:

- `codeindex-core`: parser- and storage-neutral entities, spans, textual
  representation channels, and the embedding `ModelIdentity` shared between the
  backends that produce it and the store that persists it.
- `codeindex-tree-sitter`: bundled grammars, language adapters,
  normalization, and parser-neutral extraction.
- `codeindex-sqlite`: the current incremental SQLite schema, migrations,
  model identities, vectors, the persistence API, and the single
  `ExtractedEntity → NewCodeUnit` channel→column projection (`From`).
- `codeindex-indexer`: filesystem scanning, change detection, extraction,
  retention, transactional updates into `codeindex-sqlite`, and the workflow
  that embeds a *stored corpus* — resumable projection, source-text recovery
  under lean retention, and offline token reports.
- `codeindex-embedding`: local model execution, provider diagnostics, batch
  packing, normalization, and token instrumentation. Deliberately free of
  storage and parser dependencies (only `codeindex-core`) so it can back a
  lightweight notebook binding without compiling SQLite or the grammars; the
  corpus-embedding workflow that needs both lives in `codeindex-indexer`.
- `codeindex-query`: stable selectors, metadata filtering, identity
  diagnostics, and deterministic vector ranking.
- `codeindex`: a thin facade for applications that prefer one dependency.

`decombine` keeps configuration, CLI commands, duplicate/concern/comparison
analyzers, and report rendering. Its `db`, `index`, `embed`, and `query`
modules are compatibility adapters over the reusable crates, preserving the
existing CLI and database behavior while allowing future binaries to consume
the substrate directly.

The current SQLite schema remains intentionally compatible in this
extraction. Entity-version and multi-representation persistence are a
separate schema migration, not hidden inside the crate move.
