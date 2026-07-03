# Candidate concern: error-handling

Query: “error handling and retries”

Structural spread: 2 files, 2 directories, 2 top-level modules, entropy 1.00, dispersion 2.00.

## Top units

- 0.9988 — retry_call (`main:api/retry.rs` lines 1-9)
- 0.7433 — with_backoff (`main:worker/backoff.rs` lines 1-9)

## Representative unit

```rust
fn retry_call(input: u32) -> u32 {
    let doubled = input * 2;
    doubled + 1
}
```
