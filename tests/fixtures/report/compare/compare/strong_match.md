# Strong matches

## Examples

### Example 1

- handle (`v1:src/api.rs` lines 1-9) ↔ handle_req (`v2:lib/api.rs` lines 1-9) (score 0.9976, hints +0.01)

**Left:** `v1:src/api.rs` lines 1-9

```rust
fn handle(input: u32) -> u32 {
    let doubled = input * 2;
    doubled + 1
}
```

**Right:** `v2:lib/api.rs` lines 1-9

```rust
fn handle_req(input: u32) -> u32 {
    let doubled = input * 2;
    doubled + 1
}
```

## All records

- handle (`v1:src/api.rs` lines 1-9) ↔ handle_req (`v2:lib/api.rs` lines 1-9) (score 0.9976, hints +0.01)
