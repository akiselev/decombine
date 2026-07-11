# Codeindex workspace architecture

The reusable code-intelligence substrate is split by dependency and change
boundary rather than by command:

- `codeindex-core`: parser- and storage-neutral entities, spans, and textual
  representation channels.
- `codeindex-tree-sitter`: bundled grammars, language adapters,
  normalization, and parser-neutral extraction.
- `codeindex-sqlite`: the current incremental SQLite schema, migrations,
  model identities, vectors, and persistence API.
- `codeindex-indexer`: filesystem scanning, change detection, extraction,
  retention, and transactional updates into `codeindex-sqlite`.
- `codeindex-embedding`: local model execution, provider diagnostics,
  batching, source-text recovery, and resumable embedding projection.
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
