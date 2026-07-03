//! Extension point for future approximate-nearest-neighbor backends.
//!
//! Exact all-pairs cosine stays the deterministic default. An ANN
//! implementation (`hnsw_rs` sidecar, SQLite `vec1`/`sqlite-vec`, or Qdrant
//! Edge) may be added behind a feature once benchmarks identify a concrete
//! trigger point (indexed unit count, exact-search wall time, or peak
//! memory — see docs/benchmarks.md) and it reproduces exact-search clusters
//! within an agreed recall tolerance.

use anyhow::Result;

use crate::analyze::vector_store::{ScoredPair, VectorStore};

pub trait SimilarityIndex {
    /// All unit pairs at or above `threshold`, each unordered pair once,
    /// deterministic order.
    fn similar_pairs(&self, threshold: f32, block_size: usize) -> Result<Vec<ScoredPair>>;

    /// Top-k neighbors in `to` for every unit in `from`.
    fn top_k_between(
        &self,
        from: &[usize],
        to: &[usize],
        k: usize,
        threshold: f32,
    ) -> Result<Vec<Vec<ScoredPair>>>;
}

/// The default exact implementation, backed by `VectorStore`.
pub struct ExactFlat<'v> {
    pub vectors: &'v VectorStore,
}

impl SimilarityIndex for ExactFlat<'_> {
    fn similar_pairs(&self, threshold: f32, block_size: usize) -> Result<Vec<ScoredPair>> {
        Ok(self.vectors.similar_pairs(threshold, block_size))
    }

    fn top_k_between(
        &self,
        from: &[usize],
        to: &[usize],
        k: usize,
        threshold: f32,
    ) -> Result<Vec<Vec<ScoredPair>>> {
        Ok(self.vectors.top_k_between(from, to, k, threshold))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_flat_delegates_to_vector_store() {
        let store = VectorStore::from_unit_vectors(
            2,
            vec![
                Some(vec![1.0, 0.0]),
                Some(vec![1.0, 0.0]),
                Some(vec![0.0, 1.0]),
            ],
        );
        let index = ExactFlat { vectors: &store };
        let pairs = index.similar_pairs(0.9, 100).unwrap();
        assert_eq!(pairs, store.similar_pairs(0.9, 100));
        assert_eq!(pairs.len(), 1);
        let top = index.top_k_between(&[0], &[1, 2], 1, 0.0).unwrap();
        assert_eq!(top[0][0].b, 1);
    }
}
