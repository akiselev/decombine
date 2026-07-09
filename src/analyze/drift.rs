//! Embedding-drift gate between two databases. The intended use is proving a
//! non-CPU execution provider (CUDA, DirectML, …) produces embeddings
//! equivalent to the CPU baseline before its build is trusted: same model,
//! same inputs, different backend. Compares the embeddings stored under each
//! database's model for the body hashes they share, at two levels — the raw
//! vectors (cosine, component delta) and the nearest-neighbour structure that
//! duplicate detection actually depends on (top-k recall).

use std::collections::HashMap;

use anyhow::{Result, bail};

use crate::analyze::vector_store::VectorStore;
use crate::db::ModelIdentity;
use crate::embed::normalize_in_place;

/// One side of a drift comparison: a labelled model and its embeddings.
pub struct DriftSide<'a> {
    pub label: &'a str,
    pub identity: &'a ModelIdentity,
    pub embeddings: Vec<(String, Vec<f32>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DriftReport {
    pub baseline_model: String,
    pub baseline_provider: String,
    pub baseline_count: usize,
    pub candidate_model: String,
    pub candidate_provider: String,
    pub candidate_count: usize,
    pub dimensions: usize,
    pub shared: usize,
    /// Cosine between the aligned vectors, over shared hashes.
    pub cosine_mean: f32,
    pub cosine_p50: f32,
    pub cosine_p05: f32,
    pub cosine_min: f32,
    /// Largest absolute per-component difference between normalized vectors.
    pub max_abs_component_delta: f32,
    pub top_k: usize,
    pub recall_queries: usize,
    /// Mean fraction of a unit's top-k neighbours preserved between the two
    /// embedding sets (1.0 = identical neighbourhoods). `1.0` with 0 queries
    /// means the metric was not computable (too few shared units).
    pub mean_neighbor_recall: f64,
}

/// Compute the drift between two embedding sets. `top_k` is the neighbourhood
/// size for the recall metric; `sample` caps how many query units the recall
/// scans (0 = all shared units).
pub fn compute_drift(
    baseline: &DriftSide,
    candidate: &DriftSide,
    top_k: usize,
    sample: usize,
) -> Result<DriftReport> {
    let dim = baseline.identity.dimensions;
    if candidate.identity.dimensions != dim {
        bail!(
            "cannot compare {}-dim {} against {}-dim {}: drift needs the same model on both sides",
            dim,
            baseline.identity.model,
            candidate.identity.dimensions,
            candidate.identity.model,
        );
    }

    // Align on shared hashes, normalized so cosine == dot and the component
    // delta is scale-free (a model with `normalize: false` still compares).
    let cand_by_hash: HashMap<&str, &[f32]> = candidate
        .embeddings
        .iter()
        .map(|(h, v)| (h.as_str(), v.as_slice()))
        .collect();
    let mut base_vecs: Vec<Vec<f32>> = Vec::new();
    let mut cand_vecs: Vec<Vec<f32>> = Vec::new();
    for (hash, bvec) in &baseline.embeddings {
        let Some(cvec) = cand_by_hash.get(hash.as_str()) else {
            continue;
        };
        let mut b = bvec.clone();
        let mut c = cvec.to_vec();
        normalize_in_place(&mut b);
        normalize_in_place(&mut c);
        base_vecs.push(b);
        cand_vecs.push(c);
    }
    let shared = base_vecs.len();
    if shared == 0 {
        bail!("the two databases share no body hashes to compare");
    }

    let mut cosines: Vec<f32> = Vec::with_capacity(shared);
    let mut max_abs_component_delta = 0.0f32;
    for (b, c) in base_vecs.iter().zip(&cand_vecs) {
        cosines.push(b.iter().zip(c).map(|(x, y)| x * y).sum());
        for (x, y) in b.iter().zip(c) {
            max_abs_component_delta = max_abs_component_delta.max((x - y).abs());
        }
    }
    cosines.sort_by(f32::total_cmp);
    let percentile = |q: f64| cosines[((q * (shared - 1) as f64).round() as usize).min(shared - 1)];
    let cosine_mean = cosines.iter().sum::<f32>() / shared as f32;

    let (recall_queries, mean_neighbor_recall) =
        neighbor_recall(dim, &base_vecs, &cand_vecs, top_k, sample);

    Ok(DriftReport {
        baseline_model: baseline.identity.model.clone(),
        baseline_provider: baseline.identity.execution_provider.clone(),
        baseline_count: baseline.embeddings.len(),
        candidate_model: candidate.identity.model.clone(),
        candidate_provider: candidate.identity.execution_provider.clone(),
        candidate_count: candidate.embeddings.len(),
        dimensions: dim,
        shared,
        cosine_mean,
        cosine_p50: percentile(0.50),
        cosine_p05: percentile(0.05),
        cosine_min: cosines[0],
        max_abs_component_delta,
        top_k,
        recall_queries,
        mean_neighbor_recall,
    })
}

/// Mean fraction of each query unit's top-k neighbours (among the shared units)
/// that survive the switch from the baseline to the candidate embeddings —
/// the structural signal duplicate detection relies on. Self is excluded.
fn neighbor_recall(
    dim: usize,
    base_vecs: &[Vec<f32>],
    cand_vecs: &[Vec<f32>],
    top_k: usize,
    sample: usize,
) -> (usize, f64) {
    let n = base_vecs.len();
    let k = top_k.min(n.saturating_sub(1));
    if k == 0 {
        return (0, 1.0);
    }
    let base = VectorStore::from_unit_vectors(dim, base_vecs.iter().cloned().map(Some).collect());
    let cand = VectorStore::from_unit_vectors(dim, cand_vecs.iter().cloned().map(Some).collect());

    // Deterministic evenly-spaced query sample.
    let queries: Vec<usize> = if sample == 0 || n <= sample {
        (0..n).collect()
    } else {
        (0..sample).map(|i| i * n / sample).collect()
    };

    // Ask for k+1 and drop self (a unit is its own nearest neighbour).
    let base_hits = base.top_k_between(&queries, &(0..n).collect::<Vec<_>>(), k + 1, -1.0);
    let cand_hits = cand.top_k_between(&queries, &(0..n).collect::<Vec<_>>(), k + 1, -1.0);
    let neighbors = |hits: &[crate::analyze::vector_store::ScoredPair], q: usize| {
        hits.iter()
            .filter(|p| p.b != q)
            .take(k)
            .map(|p| p.b)
            .collect::<std::collections::HashSet<usize>>()
    };
    let mut recall_sum = 0.0f64;
    for (i, &q) in queries.iter().enumerate() {
        let b = neighbors(&base_hits[i], q);
        let c = neighbors(&cand_hits[i], q);
        let overlap = b.intersection(&c).count();
        recall_sum += overlap as f64 / k as f64;
    }
    (queries.len(), recall_sum / queries.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(model: &str, provider: &str, dim: usize) -> ModelIdentity {
        ModelIdentity {
            backend: "test".into(),
            backend_version: "0".into(),
            runtime_version: None,
            model: model.into(),
            revision: None,
            dimensions: dim,
            tokenizer_hash: None,
            model_hash: None,
            normalize: true,
            execution_provider: provider.into(),
            quantization: None,
            cache_path: None,
        }
    }

    fn side<'a>(id: &'a ModelIdentity, e: Vec<(&str, Vec<f32>)>) -> DriftSide<'a> {
        DriftSide {
            label: "s",
            identity: id,
            embeddings: e.into_iter().map(|(h, v)| (h.to_string(), v)).collect(),
        }
    }

    #[test]
    fn identical_embeddings_have_zero_drift() {
        let id = identity("m", "cpu", 2);
        let e = vec![
            ("a", vec![1.0, 0.0]),
            ("b", vec![0.0, 1.0]),
            ("c", vec![1.0, 1.0]),
        ];
        let base = side(&id, e.clone());
        let cand = side(&id, e);
        let r = compute_drift(&base, &cand, 2, 0).unwrap();
        assert_eq!(r.shared, 3);
        assert!((r.cosine_min - 1.0).abs() < 1e-6);
        assert!(r.max_abs_component_delta < 1e-6);
        assert!((r.mean_neighbor_recall - 1.0).abs() < 1e-9);
    }

    #[test]
    fn perturbed_vectors_drift_but_keep_neighbors() {
        let id = identity("m", "cpu", 2);
        // Two well-separated pairs; each unit's nearest neighbour is its pair
        // partner. The candidate nudges every vector slightly: cosine drops
        // below 1 but the top-1 neighbourhoods are unchanged.
        let base = side(
            &id,
            vec![
                ("a", vec![1.0, 0.0]),
                ("a2", vec![0.99, 0.141]),
                ("b", vec![0.0, 1.0]),
                ("b2", vec![0.141, 0.99]),
            ],
        );
        let cand = side(
            &id,
            vec![
                ("a", vec![1.0, 0.02]),
                ("a2", vec![0.985, 0.16]),
                ("b", vec![0.02, 1.0]),
                ("b2", vec![0.16, 0.985]),
            ],
        );
        let r = compute_drift(&base, &cand, 1, 0).unwrap();
        assert!(r.cosine_min < 1.0 && r.cosine_min > 0.99);
        assert!(r.max_abs_component_delta > 0.0);
        assert!((r.mean_neighbor_recall - 1.0).abs() < 1e-9);
    }

    #[test]
    fn only_shared_hashes_are_compared_and_dims_must_match() {
        let id = identity("m", "cpu", 2);
        let base = side(&id, vec![("a", vec![1.0, 0.0]), ("x", vec![0.0, 1.0])]);
        let cand = side(&id, vec![("a", vec![1.0, 0.0]), ("y", vec![1.0, 1.0])]);
        let r = compute_drift(&base, &cand, 1, 0).unwrap();
        assert_eq!(r.shared, 1, "only hash `a` is shared");

        let id3 = identity("m", "cuda", 3);
        let wrong = side(&id3, vec![("a", vec![1.0, 0.0, 0.0])]);
        assert!(
            compute_drift(&base, &wrong, 1, 0).is_err(),
            "dim mismatch errors"
        );
    }
}
