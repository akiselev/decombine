# Cluster 1 — `5946569cd5430ef8`

3 units, top similarity 1.0000 (boosted 1.0600). Ignore with:

```
5946569cd5430ef8
```

## Top pairs

- 1.0000 (boosted 1.0600): `main:billing/invoice.rs` lines 1-9 ↔ `main:shipping/label.rs` lines 1-9
- 0.9976 (boosted 1.0574): `main:billing/invoice.rs` lines 1-9 ↔ `main:reports/summary.rs` lines 1-9
- 0.9976 (boosted 1.0574): `main:shipping/label.rs` lines 1-9 ↔ `main:reports/summary.rs` lines 1-9

## Invoice.send_total

- `main:billing/invoice.rs` lines 1-9
- `main:shipping/label.rs` lines 1-9

2 exact copies of this body.

```rust
fn send_total(input: u32) -> u32 {
    let doubled = input * 2;
    doubled + 1
}
```

## send_summary

- `main:reports/summary.rs` lines 1-9

```rust
fn send_summary(input: u32) -> u32 {
    let doubled = input * 2;
    doubled + 1
}
```

