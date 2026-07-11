# Comparing two projects (rewrite/port audits)

`decombine compare` maps every code unit in a reference project (`left`)
to its counterpart in a candidate project (`right`) — use it to audit a
rewrite, a fork, or a port: what survived, what changed, what was dropped,
what is new.

## Setup

Both projects must be indexed and embedded into the same database with the
same model. Declare them as labeled projects:

```yaml
projects:
  - label: v1
    source_dir: ../old-project
  - label: v2
    source_dir: .

comparison:
  left: v1        # reference
  right: v2       # candidate
  calibration: background   # keep this on (see Thresholds below)
```

Then:

```sh
decombine index && decombine embed
decombine compare --json > compare.json     # or --left/--right to override config
```

## Reading the results

Each match record has a `class`:

| class | meaning |
| --- | --- |
| `exact_copy` | identical normalized body on both sides |
| `strong_match` | mutual nearest neighbors above the match threshold |
| `possible_match` | a candidate edge exists but wasn't mutual/strong |
| `split` | one left unit became several right units |
| `merge` | several left units became one right unit |
| `missing_in_right` | left unit with no counterpart — **dropped functionality?** |
| `new_in_right` | right unit with no counterpart — new work |

The JSON `summary` counts every class. Coverage tables
(`coverage_by_directory`, `coverage_by_language`) aggregate left-side
covered/possible/missing — the fastest way to see *where* a rewrite is
incomplete.

**Ordering gotcha:** match records sort by class, exact copies first and
`missing_in_right`/`new_in_right` last. A `--limit` truncates the tail
classes first, so when filtering by class take the full list (no
`--limit`):

```sh
jq -r '.items[] | select(.class == "missing_in_right") | .left[0].path' compare.json
jq '.summary' compare.json
```

`score` is the raw cosine of the best edge; `hint_bonus` is a bounded
(≤0.04) name/path bonus that can reorder candidates but never turns a weak
edge into a strong match.

## Thresholds and calibration

Raw comparison thresholds (`candidate_threshold`, `match_threshold`) are
positions in the model's cosine scale and do not port across models. With
`calibration: background` (recommended, especially on CodeRankEmbed)
decombine samples cross-project background similarity and rescales the
thresholds automatically; the JSON `calibration` object records what was
applied (background mean/std, anchor used, effective thresholds, sigma
floors, strong-match margin). If `calibration.applied` is `false`, the
corpus had no usable signal range and the raw configured thresholds were
used as-is.

`suppressed_right_candidates` lists right-side units that too many left
units claimed as best match (generic helpers); they are excluded from
matching so they don't vacuum up everything.

## Follow-up on a specific match

Use the query interface (see querying.md) to pull the source of both
sides:

```sh
decombine query inspect unit:<left-id> --source
decombine query inspect unit:<right-id> --source
```
