//! Cross-directory duplication candidates: clusters whose members span
//! distant paths or multiple top-level modules. This is the baseline
//! concern-like signal (evidence-backed, no latent-basis assumptions).

use std::collections::BTreeSet;

use crate::analyze::context::CodeUnitRef;
use crate::analyze::paths::{directory_entropy, path_distance, top_level_module};

use super::Cluster;

/// Shared-scope names that suggest intentional helpers rather than
/// scattered duplication.
const GENERIC_SEGMENTS: &[&str] = &[
    "utils", "util", "common", "helpers", "helper", "shared", "lib",
];

#[derive(Debug, Clone, PartialEq)]
pub struct CrossDirectoryCandidate {
    /// Index into the surviving cluster list.
    pub cluster_index: usize,
    pub cluster_hash: String,
    /// Distinct top-level modules covered by cluster members.
    pub modules: Vec<String>,
    /// Mean nearest-neighbor path distance between cluster members.
    pub dispersion: f64,
    pub directory_entropy: f64,
    /// Distinct receiver/class scopes among members (method-logic evidence).
    pub scopes: Vec<String>,
    /// True when most members live in generic shared directories.
    pub generic_helper: bool,
    /// Final ranking score after down/up-ranking.
    pub score: f64,
}

/// Mean nearest-neighbor path distance: for each member, the path distance
/// to its closest fellow member; averaged. This is a per-cluster dispersion
/// metric, deliberately not derived from pair rerank scores.
pub fn nearest_neighbor_dispersion(units: &[&CodeUnitRef]) -> f64 {
    if units.len() < 2 {
        return 0.0;
    }
    let total: usize = units
        .iter()
        .enumerate()
        .map(|(i, a)| {
            units
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, b)| path_distance(&a.relative_path, &b.relative_path))
                .min()
                .unwrap_or(0)
        })
        .sum();
    total as f64 / units.len() as f64
}

fn is_generic_path(path: &str) -> bool {
    path.split('/')
        .rev()
        .skip(1) // the file name itself doesn't make a scope generic
        .any(|segment| GENERIC_SEGMENTS.contains(&segment.to_ascii_lowercase().as_str()))
}

/// Derive ranked cross-directory candidates from surviving clusters.
pub fn cross_directory_candidates(
    clusters: &[Cluster],
    units: &[CodeUnitRef],
) -> Vec<CrossDirectoryCandidate> {
    let mut candidates = Vec::new();
    for (cluster_index, cluster) in clusters.iter().enumerate() {
        let members: Vec<&CodeUnitRef> = cluster.members.iter().map(|&i| &units[i]).collect();
        let modules: BTreeSet<String> = members
            .iter()
            .map(|u| top_level_module(&u.relative_path).to_string())
            .collect();
        let dispersion = nearest_neighbor_dispersion(&members);
        // Local-only clusters are not cross-directory candidates.
        if modules.len() < 2 && dispersion < 1.0 {
            continue;
        }
        let entropy = directory_entropy(members.iter().map(|u| u.relative_path.as_str()));
        let scopes: BTreeSet<String> = members.iter().filter_map(|u| u.scope.clone()).collect();
        let generic_count = members
            .iter()
            .filter(|u| is_generic_path(&u.relative_path))
            .count();
        let generic_helper = generic_count * 2 > members.len();

        let mut score = dispersion + entropy + modules.len() as f64;
        if generic_helper {
            // Shared-scope helpers are usually intentional; downrank.
            score *= 0.4;
        }
        if scopes.len() >= 2 {
            // Repeated method logic across different receiver/class scopes
            // is the strongest signal; emphasize it.
            score *= 1.5;
        }
        candidates.push(CrossDirectoryCandidate {
            cluster_index,
            cluster_hash: cluster.hash.clone(),
            modules: modules.into_iter().collect(),
            dispersion,
            directory_entropy: entropy,
            scopes: scopes.into_iter().collect(),
            generic_helper,
            score,
        });
    }
    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.cluster_hash.cmp(&b.cluster_hash))
    });
    candidates
}
