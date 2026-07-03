//! Reusable structural metrics over project-relative paths.

/// Directory components of a relative path (excludes the file name).
fn directories(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

/// The directory portion of a relative path ("" for root files).
pub fn directory_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

/// First path component, or the file name for root-level files.
pub fn top_level_module(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// Directory hops between two relative paths: components above the deepest
/// common ancestor. Same directory = 0.
pub fn path_distance(a: &str, b: &str) -> usize {
    let dirs_a = directories(a);
    let dirs_b = directories(b);
    let common = dirs_a
        .iter()
        .zip(dirs_b.iter())
        .take_while(|(x, y)| x == y)
        .count();
    (dirs_a.len() - common) + (dirs_b.len() - common)
}

/// Do two byte ranges intersect?
pub fn byte_ranges_overlap(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Line gap between two same-file units (0 when they touch or overlap).
pub fn line_distance(a: (usize, usize), b: (usize, usize)) -> usize {
    if a.1 < b.0 {
        b.0 - a.1
    } else {
        a.0.saturating_sub(b.1)
    }
}

/// Shannon entropy (bits) of the directory distribution of `paths`.
/// 0.0 when everything lives in one directory; grows with scatter.
pub fn directory_entropy<'a>(paths: impl Iterator<Item = &'a str>) -> f64 {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for path in paths {
        *counts.entry(directory_of(path)).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    counts
        .values()
        .map(|&count| {
            let p = count as f64 / total as f64;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_distances() {
        assert_eq!(path_distance("a/b/x.rs", "a/b/y.rs"), 0);
        assert_eq!(path_distance("a/b/x.rs", "a/c/y.rs"), 2);
        assert_eq!(path_distance("a/x.rs", "a/b/c/y.rs"), 2);
        assert_eq!(path_distance("x.rs", "y.rs"), 0);
        assert_eq!(path_distance("m1/x.rs", "m2/y.rs"), 2);
    }

    #[test]
    fn overlap_uses_bytes() {
        assert!(byte_ranges_overlap((0, 100), (50, 150)));
        assert!(byte_ranges_overlap((50, 60), (0, 100)));
        assert!(!byte_ranges_overlap((0, 100), (100, 200)));
        assert!(!byte_ranges_overlap((100, 200), (0, 50)));
    }

    #[test]
    fn line_distances() {
        assert_eq!(line_distance((1, 10), (20, 30)), 10);
        assert_eq!(line_distance((20, 30), (1, 10)), 10);
        assert_eq!(line_distance((1, 10), (5, 15)), 0);
    }

    #[test]
    fn entropy() {
        assert_eq!(directory_entropy(["a/x.rs", "a/y.rs"].into_iter()), 0.0);
        let spread = directory_entropy(["a/x.rs", "b/y.rs", "c/z.rs", "d/w.rs"].into_iter());
        assert!((spread - 2.0).abs() < 1e-9);
        assert_eq!(directory_entropy(std::iter::empty()), 0.0);
    }
}
