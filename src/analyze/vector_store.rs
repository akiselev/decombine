//! Dense storage for normalized unit vectors plus the shared similarity
//! kernels. Rows are aligned with `AnalysisContext::units` through
//! `row_for_unit`; units without an embedding have no row.

use rayon::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoredPair {
    /// Indices into `AnalysisContext::units`.
    pub a: usize,
    pub b: usize,
    pub score: f32,
}

pub struct VectorStore {
    dimensions: usize,
    /// Row-major, one row per embedded unit.
    data: Vec<f32>,
    /// Unit index (into the context's unit list) for each row.
    row_units: Vec<usize>,
    /// Row for each unit index; `None` when the unit has no embedding.
    unit_rows: Vec<Option<usize>>,
}

impl VectorStore {
    /// Build from per-unit vectors (`None` for units without embeddings).
    pub fn from_unit_vectors(dimensions: usize, vectors: Vec<Option<Vec<f32>>>) -> Self {
        let mut data = Vec::new();
        let mut row_units = Vec::new();
        let mut unit_rows = Vec::with_capacity(vectors.len());
        for (unit_index, vector) in vectors.into_iter().enumerate() {
            match vector {
                Some(vector) => {
                    assert_eq!(vector.len(), dimensions, "vector dimension mismatch");
                    unit_rows.push(Some(row_units.len()));
                    row_units.push(unit_index);
                    data.extend_from_slice(&vector);
                }
                None => unit_rows.push(None),
            }
        }
        Self {
            dimensions,
            data,
            row_units,
            unit_rows,
        }
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Number of embedded units (rows).
    pub fn len(&self) -> usize {
        self.row_units.len()
    }

    pub fn is_empty(&self) -> bool {
        self.row_units.is_empty()
    }

    pub fn row_for_unit(&self, unit_index: usize) -> Option<usize> {
        self.unit_rows.get(unit_index).copied().flatten()
    }

    pub fn unit_for_row(&self, row: usize) -> usize {
        self.row_units[row]
    }

    pub fn vector(&self, row: usize) -> &[f32] {
        &self.data[row * self.dimensions..(row + 1) * self.dimensions]
    }

    pub fn dot(&self, row_a: usize, row_b: usize) -> f32 {
        dot(self.vector(row_a), self.vector(row_b))
    }

    /// All unit pairs with cosine >= `threshold`, computed in row blocks to
    /// bound intermediate memory. Emits each unordered pair once (a < b by
    /// unit index), never self-pairs, sorted by descending score then unit
    /// indices for determinism.
    pub fn similar_pairs(&self, threshold: f32, block_size: usize) -> Vec<ScoredPair> {
        let n = self.len();
        let block_size = block_size.max(1);
        let block_starts: Vec<usize> = (0..n).step_by(block_size).collect();
        let mut pairs: Vec<ScoredPair> = block_starts
            .par_iter()
            .flat_map_iter(|&start| {
                let end = (start + block_size).min(n);
                let mut local = Vec::new();
                for i in start..end {
                    for j in (i + 1)..n {
                        let score = self.dot(i, j);
                        if score >= threshold {
                            local.push(ScoredPair {
                                a: self.unit_for_row(i),
                                b: self.unit_for_row(j),
                                score,
                            });
                        }
                    }
                }
                local
            })
            .collect();
        pairs.sort_by(|x, y| {
            y.score
                .total_cmp(&x.score)
                .then(x.a.cmp(&y.a))
                .then(x.b.cmp(&y.b))
        });
        pairs
    }

    /// For every unit in `from`, the top-k most similar units in `to` with
    /// cosine >= `threshold`. Indices are unit indices.
    pub fn top_k_between(
        &self,
        from: &[usize],
        to: &[usize],
        k: usize,
        threshold: f32,
    ) -> Vec<Vec<ScoredPair>> {
        from.iter()
            .map(|&a| {
                let Some(row_a) = self.row_for_unit(a) else {
                    return Vec::new();
                };
                let mut scored: Vec<ScoredPair> = to
                    .iter()
                    .filter_map(|&b| {
                        let row_b = self.row_for_unit(b)?;
                        let score = self.dot(row_a, row_b);
                        (score >= threshold).then_some(ScoredPair { a, b, score })
                    })
                    .collect();
                scored.sort_by(|x, y| y.score.total_cmp(&x.score).then(x.b.cmp(&y.b)));
                scored.truncate(k);
                scored
            })
            .collect()
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit 1 has no embedding; 0/2 point the same way, 3 is orthogonal.
    fn store() -> VectorStore {
        VectorStore::from_unit_vectors(
            2,
            vec![
                Some(vec![1.0, 0.0]),
                None,
                Some(vec![1.0, 0.0]),
                Some(vec![0.0, 1.0]),
            ],
        )
    }

    #[test]
    fn sparse_unit_ids_map_to_rows() {
        let store = store();
        assert_eq!(store.len(), 3);
        assert_eq!(store.row_for_unit(0), Some(0));
        assert_eq!(store.row_for_unit(1), None);
        assert_eq!(store.row_for_unit(3), Some(2));
        assert_eq!(store.unit_for_row(2), 3);
    }

    #[test]
    fn similar_pairs_threshold_sorting_no_self_pairs() {
        let store = store();
        let pairs = store.similar_pairs(0.5, 2);
        assert_eq!(pairs.len(), 1);
        assert_eq!((pairs[0].a, pairs[0].b), (0, 2));
        assert!((pairs[0].score - 1.0).abs() < 1e-6);

        // Lower threshold picks up the orthogonal pair boundary case.
        let pairs = store.similar_pairs(0.0, 1);
        assert_eq!(pairs.len(), 3);
        assert!(pairs.iter().all(|p| p.a != p.b));
        assert!(pairs[0].score >= pairs[1].score);
    }

    #[test]
    fn block_size_does_not_change_results() {
        let store = store();
        for block in [1, 2, 3, 100] {
            assert_eq!(store.similar_pairs(0.0, block).len(), 3);
        }
    }

    #[test]
    fn top_k_between_respects_k_and_threshold() {
        let store = store();
        let hits = store.top_k_between(&[0], &[2, 3], 5, 0.5);
        assert_eq!(hits[0].len(), 1);
        assert_eq!(hits[0][0].b, 2);
        // Units without embeddings produce no hits.
        let hits = store.top_k_between(&[1], &[0], 5, 0.0);
        assert!(hits[0].is_empty());
    }
}
