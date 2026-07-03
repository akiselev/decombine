# Duplicate code report

- Model: `fixture` backend `test` v1.0 (4 dims, provider cpu)
- Thresholds: candidate 0.88 / similarity 0.92 / rerank 0.94
- Retention: report
- Run: 2026-07-03 00:00:00Z
- Project `main`: /repo/main

3 candidate pairs, 1 clusters (0 ignored).

## Clusters

| # | Cluster | Units | Top raw | Boosted | Members |
| --- | --- | --- | --- | --- | --- |
| 1 | [`5946569cd5430ef8`](cluster-01.md) | 3 | 1.0000 | 1.0600 | Invoice.send_total, Label.send_total, send_summary |

## Cross-directory duplication

Clusters whose members span distant paths or multiple top-level modules. Scope evidence lists distinct receiver/class scopes; generic shared-scope helpers are downranked.

| Cluster | Modules | Dispersion | Entropy | Scopes | Generic helper | Score |
| --- | --- | --- | --- | --- | --- | --- |
| `5946569cd5430ef8` | billing, reports, shipping | 2.00 | 1.58 | Invoice, Label | no | 9.88 |

## Ignoring reviewed clusters

Append a cluster hash to `.decombineignore` to suppress it. It reappears if the code or its location changes.
