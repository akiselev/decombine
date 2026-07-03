//! Distance-aware reranking: pairs that live far apart in the repository
//! are boosted, because remote duplication is more actionable than
//! neighboring near-copies.

use crate::analyze::context::CodeUnitRef;
use crate::analyze::paths::{line_distance, path_distance};

/// Maximum boost for cross-directory distance (matches upstream's 15%).
const MAX_PATH_BOOST: f32 = 0.15;
/// Boost per directory hop.
const PATH_BOOST_PER_HOP: f32 = 0.03;
/// Maximum boost for same-file line distance (matches upstream's 10%).
const MAX_LINE_BOOST: f32 = 0.10;
/// Line gap at which the same-file boost saturates.
const LINE_BOOST_SATURATION: usize = 200;

/// Multiplicative boost factor for a pair of units.
pub fn distance_boost(a: &CodeUnitRef, b: &CodeUnitRef) -> f32 {
    if a.project_label == b.project_label && a.relative_path == b.relative_path {
        let gap = line_distance((a.start_line, a.end_line), (b.start_line, b.end_line));
        let fraction = (gap as f32 / LINE_BOOST_SATURATION as f32).min(1.0);
        fraction * MAX_LINE_BOOST
    } else {
        let hops = path_distance(&a.relative_path, &b.relative_path);
        (hops as f32 * PATH_BOOST_PER_HOP).min(MAX_PATH_BOOST)
    }
}

/// Raw cosine score with the distance boost applied.
pub fn boosted_score(raw: f32, a: &CodeUnitRef, b: &CodeUnitRef) -> f32 {
    raw * (1.0 + distance_boost(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(path: &str, start_line: usize, end_line: usize) -> CodeUnitRef {
        CodeUnitRef {
            id: 0,
            project_label: "main".into(),
            relative_path: path.into(),
            language_id: "rust".into(),
            kind: "function".into(),
            name: "f".into(),
            scope: None,
            start_byte: start_line * 100,
            end_byte: end_line * 100,
            start_line,
            end_line,
            body_node_count: 10,
            normalized_body_hash: "h".into(),
            display_source: None,
        }
    }

    #[test]
    fn cross_directory_boost_saturates() {
        let a = unit("a/b/x.rs", 1, 10);
        let near = unit("a/b/y.rs", 1, 10);
        let far = unit("z/deep/nested/very/far.rs", 1, 10);
        assert_eq!(distance_boost(&a, &near), 0.0);
        assert!((distance_boost(&a, &far) - MAX_PATH_BOOST).abs() < 1e-6);
    }

    #[test]
    fn same_file_boost_grows_with_line_gap() {
        let a = unit("x.rs", 1, 10);
        let close = unit("x.rs", 12, 20);
        let far = unit("x.rs", 500, 520);
        assert!(distance_boost(&a, &close) < 0.01);
        assert!((distance_boost(&a, &far) - MAX_LINE_BOOST).abs() < 1e-6);
    }
}
