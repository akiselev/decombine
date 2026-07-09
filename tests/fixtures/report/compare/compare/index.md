# Comparison: `v1` (reference) vs `v2` (candidate)

- Model: `fixture` backend `test` v1.0 (4 dims, provider cpu)
- Comparison thresholds: candidate 0.78 / match 0.86 / top-k 5 / min nodes 0 / right fanout 0 / abtt 0
- Hints: names on / paths on
- Retention: report
- Run: 2026-07-03 00:00:00Z
- Project `v1`: /repo/v1
- Project `v2`: /repo/v2

Semantic coverage of the reference project by the candidate. Matches are embedding evidence, not proof of behavioral equivalence.

| Class | Count |
| --- | --- |
| [Exact copies](exact_copy.md) | 1 |
| [Strong matches](strong_match.md) | 1 |
| Possible matches | 0 |
| Splits (one → many) | 0 |
| Merges (many → one) | 0 |
| [Possible missing coverage](missing_in_right.md) | 1 |
| [Possible new behavior](new_in_right.md) | 1 |

## Coverage by directory

| Group | Left units | Covered | Possible | Missing |
| --- | --- | --- | --- | --- |
| `src` | 3 | 2 | 0 | 1 |

## Coverage by language

| Group | Left units | Covered | Possible | Missing |
| --- | --- | --- | --- | --- |
| `rust` | 3 | 2 | 0 | 1 |
