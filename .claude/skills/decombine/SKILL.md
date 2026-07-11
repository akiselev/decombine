---
name: decombine
description: Find non-exact code duplication with decombine (tree-sitter + local embeddings + SQLite). Use when asked to find duplicate/copy-pasted/similar code in a codebase, audit a rewrite against the original, semantically search indexed code, or run/interpret any decombine command. Covers setup, the duplicates workflow, and (in reference files) comparison, semantic query, concerns, and operations.
---

# decombine: find non-exact code duplication

decombine indexes code units (functions, methods, closures) with
tree-sitter, embeds them with a local ONNX model (no API keys, no network
after the model download), and reports clusters of semantically similar
code. Supported languages: C, C++, C#, Go, Java, JavaScript, Kotlin, PHP,
Python, Ruby, Rust, TypeScript.

Get a binary: `decombine` on PATH, or build from this repo with
`env RUSTC_WRAPPER= cargo build --release` → `target/release/decombine`.

## Core workflow: find duplicates

```sh
decombine init            # writes decombine.yaml — edit source_dir first
decombine index           # extract code units into decombine.db
decombine embed           # embed distinct bodies (downloads model on first use)
decombine analyze         # duplicate clusters → decombine-report/
# or all three at once:
decombine run
```

For machine-readable results (preferred when you are the consumer), skip
the markdown report:

```sh
decombine analyze duplicates --json > dupes.json
jq '.summary' dupes.json          # matched/returned/exhaustive + candidate_pairs
jq '.items[0]' dupes.json         # best cluster: members, pairs, scores
```

`--json` prints one envelope on stdout (progress goes to stderr); `--limit N`
bounds items and the summary reports `has_more` honestly. Never parse the
markdown report — the JSON carries the same content with stable IDs.

## Configuration essentials (decombine.yaml)

- `source_dir: .` for one project, or labeled `projects:` for several
  (required for comparison). Exclude vendored trees via `source_dir_exclude`.
- Default model is `CodeRankEmbed` (2048-token context, ~550 MB download,
  hash-verified). `BGESmallENV15` is the lightweight no-download-pain
  alternative but truncates ~5–13% of units on heavy codebases.
- **Thresholds do not port across models.** CodeRank duplicate defaults are
  `candidate/similarity/rerank = 0.70/0.81/0.85`; the BGE-scale equivalents
  are `0.88/0.92/0.94`. If you change `embedding.model`, change the
  thresholds too.
- A database is bound to one model identity forever. To switch models,
  use a new `db_file` — do not fight the immutability error.

## Reading duplicate results

Clusters are sectioned by member location, in priority order:

1. **Product code clusters** — the findings that matter; refactor candidates.
2. **Product ↔ test/docs clusters** — usually an implementation matched to
   its own test; a recurring false-positive shape. Deprioritize.
3. **Test and docs clusters** — boilerplate; usually ignorable.

Within a section, clusters marked with a `name_family` (one method name
repeated across many receiver scopes, e.g. `Display::fmt` × 16) are
correctly-matched interface idioms with near-zero refactor value — they
sort last on purpose. `exact_groups` folds identical bodies: many copies of
one body are one finding. `cross_directory` lists clusters whose members
span modules — the strongest "extract a shared helper" signals.

Scores: `raw` is cosine similarity; `boosted` adds a distance rerank
(far-apart copies rank higher because they are more likely unintentional).

## Ignore workflow (suppress reviewed clusters)

Cluster IDs are stable over (path, body-hash) pairs — they survive
re-indexing but change when the code changes. After reviewing a cluster as
intentional, append its bare 16-hex hash (the `cluster:` prefix stripped)
to the ignore file (`.decombineignore` by default), one per line, `#`
comments allowed:

```text
dc02682f3e10951a  # sort_by ranking idiom, reviewed 2026-07-09
```

Re-running `analyze` then drops it and reports it under `ignored`.

## Pitfalls

- `analyze` fails with "no embeddings found" → run `decombine embed` first.
  `query capabilities` shows counts and pending (un-embedded) bodies.
- Incremental indexing skips unchanged files by mtime/size then content
  hash — `touch` does NOT force re-extraction.
- Findings never affect the exit code; non-zero means the command itself
  failed. Check the JSON `summary` for results.
- Embedding saturates all CPU cores; on big corpora run `embed` once, not
  in parallel with other embed runs.

## Other features

Read the matching reference file before using these:

| Task | File |
| --- | --- |
| Compare two projects / audit a rewrite (`compare`) | [comparing.md](comparing.md) |
| Explore an indexed codebase: semantic search, neighbors, unit lookup (`query`) | [querying.md](querying.md) |
| Score code against named concern queries (`analyze concerns`) | [concerns.md](concerns.md) |
| Models, doctor, drift gate, token audit, DB maintenance | [operations.md](operations.md) |
