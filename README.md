# decombine

A Rust CLI for detecting non-exact code duplication using embeddings. Inspired by [Slopo](https://github.com/rafal-qa/slopo) 

`decombine` indexes code units with Tree-sitter, embeds them with a local ONNX model (no API
keys), stores everything in a single SQLite file, and writes Markdown reports.

## Installation

Clone and run `cargo install --path decombine` (will be published to Crates.io later)

## Commands

```text
decombine init                 # write a starter decombine.yaml
decombine show-config          # print effective configuration
decombine index                # extract code units into SQLite
decombine embed                # embed distinct bodies with the local model
decombine analyze [duplicates] # duplicate cluster analysis (default)
decombine analyze concerns     # concern query projection analysis
decombine compare              # compare two indexed projects
decombine run                  # index + embed + analyze
decombine query capabilities   # what this config/database can answer
decombine query units          # list units, filtered by metadata (--where)
decombine query inspect        # resolve a unit:<id> selector (+ --source)
decombine query similar        # vector neighbors of a unit
decombine query search         # semantic search from natural language
decombine query qbe            # query by example (same engine as similar)
decombine languages list
decombine models list
decombine models download
decombine doctor
```

`analyze`, `compare`, and every `query` command take `--json` for a
machine-readable envelope on stdout (progress stays on stderr) and
`--limit` with honest `exhaustive`/`has_more` reporting — see
[docs/query-interface.md](docs/query-interface.md) for the full reference,
with examples run against this repository.

## Quick start

```sh
decombine init          # writes decombine.yaml (edit source_dir)
decombine index         # extract code units into decombine.db
decombine embed         # downloads the model on first run, then offline
decombine analyze       # writes decombine-report/index.md + cluster files
```

Supported languages: C, C++, C#, Go, Java, JavaScript, Kotlin, PHP, Python,
Ruby, Rust, TypeScript. Adding a language is mostly declarative: a parser crate plus
`assets/languages/<id>.toml` and `assets/languages/<id>/units.scm`.

For comparing two codebases (e.g. a rewrite against the original), declare
labeled `projects` in the config, index and embed both, then run
`decombine compare --left v1 --right v2`.

## Development

CI commands (run locally before pushing):

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The `fastembed` feature (enabled by default) pulls in ONNX Runtime for local
embedding inference. `cargo build --no-default-features` gives a faster build
with only the deterministic test embedding backend, useful for development.

Tests never download models; embedding-dependent tests use a deterministic
local backend. First real use of `decombine embed` downloads the configured
model into the local cache (see `decombine models download`).
