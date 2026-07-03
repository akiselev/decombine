use std::ops::Range;

use sha2::{Digest, Sha256};

/// Remove the given byte ranges from `text` (ranges are relative to `text`).
/// Overlapping or unsorted ranges are merged first.
pub fn strip_ranges(text: &str, ranges: &[Range<usize>]) -> String {
    let merged = merge_ranges(ranges);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in merged {
        let start = range.start.min(text.len());
        let end = range.end.min(text.len());
        if start > cursor {
            out.push_str(&text[cursor..start]);
        }
        cursor = cursor.max(end);
    }
    if cursor < text.len() {
        out.push_str(&text[cursor..]);
    }
    out
}

/// Merge overlapping/adjacent byte ranges and sort them.
pub fn merge_ranges(ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut sorted: Vec<Range<usize>> = ranges.to_vec();
    sorted.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(sorted.len());
    for range in sorted {
        if range.start >= range.end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    merged
}

/// Whitespace-insensitive form used for hashing: every run of whitespace
/// becomes a single space, leading/trailing whitespace is dropped.
pub fn normalize_for_hash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_and_merges_ranges() {
        let text = "abcdefghij";
        assert_eq!(strip_ranges(text, &[2..4, 3..6, 8..10]), "abgh");
        assert_eq!(strip_ranges(text, &[]), text);
        assert_eq!(strip_ranges(text, &[0..10]), "");
        // Out-of-bounds ranges are clamped.
        assert_eq!(strip_ranges(text, &[5..99]), "abcde");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(
            normalize_for_hash("  fn   foo()\n\t{ 1 }\n"),
            "fn foo() { 1 }"
        );
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(sha256_hex("abc").len(), 64);
        assert_eq!(sha256_hex("abc"), sha256_hex("abc"));
        assert_ne!(sha256_hex("abc"), sha256_hex("abd"));
    }
}
