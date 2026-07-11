# Concern analysis: where does a concept live, and is it scattered?

`decombine analyze concerns` projects every embedded code unit onto named
natural-language queries and measures how structurally scattered the
high-scoring units are. Use it to answer "where is error handling / auth /
retry logic implemented, and is it centralized or smeared across the
codebase?"

Results are **candidate** concerns — evidence for review, not proof.

## Setup

Configure queries under `analysis.concerns` (the queries are embedded with
the same model as the code, so wording matters — describe the code you
expect, not abstract goals):

```yaml
analysis:
  concerns:
    enabled: true              # also runs during `decombine run`
    min_projection: 0.45       # score floor (model-specific scale)
    top_units_per_concern: 50
    queries:
      - name: error-handling
        query: "error handling, retries, and failure reporting"
      - name: input-validation
        query: "validating and sanitizing untrusted user input"
```

Run it (works with `enabled: false` too — the flag only gates `run`):

```sh
decombine analyze concerns --json > concerns.json
jq '.items[] | {name, spread}' concerns.json
```

## Reading the results

Each finding lists the top units above `min_projection` (best first, with
scores) plus spread metrics over those units:

- `files`, `directories`, `top_level_modules` — raw fan-out counts.
- `directory_entropy` — high = evenly smeared across directories,
  low = concentrated in one place.
- `dispersion` — mean nearest-neighbor path distance; high = the hits are
  far apart in the tree.

A concern with many hits, high entropy, and high dispersion is scattered —
a centralization/refactor candidate. Few hits in one module = healthy.

## Pitfalls

- `min_projection` is in the model's cosine scale and does not port across
  models (the 0.45 default is tuned for the shipped defaults). If a
  concern returns nothing, lower it before concluding the concept is absent.
- One-off exploration ("where is X?") is faster via
  `decombine query search --text "X"` (see querying.md); concerns are for
  repeatable, configured, spread-measured projections.
