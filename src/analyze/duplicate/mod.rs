//! Duplicate-cluster analysis: exact block-wise cosine search, distance
//! reranking, union-find clustering, exact-copy folding, cross-directory
//! candidates, and the ignore workflow.

pub mod clustering;
pub mod cross_directory;
pub mod ignore;
pub mod rerank;

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;

use crate::analyze::context::{AnalysisContext, Analyzer};
use crate::analyze::paths::byte_ranges_overlap;
use crate::config::AnalysisConfig;
use crate::index::normalizer::sha256_hex;

pub use cross_directory::CrossDirectoryCandidate;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RankedPair {
    /// Indices into `AnalysisContext::units`.
    pub a: usize,
    pub b: usize,
    pub raw: f32,
    pub boosted: f32,
}

/// A group of units sharing one normalized body hash (exact copies).
#[derive(Debug, Clone, PartialEq)]
pub struct ExactGroup {
    pub normalized_body_hash: String,
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// Unit indices, sorted.
    pub members: Vec<usize>,
    /// Surviving pairs inside this cluster, best first.
    pub pairs: Vec<RankedPair>,
    /// Stable id over (relative path, body hash) of members.
    pub hash: String,
    pub top_raw: f32,
    pub top_boosted: f32,
    /// Exact-copy folding: one entry per distinct body hash.
    pub exact_groups: Vec<ExactGroup>,
}

#[derive(Debug, Default)]
pub struct DuplicateReport {
    /// Surviving clusters, ranked by top boosted score.
    pub clusters: Vec<Cluster>,
    pub cross_directory: Vec<CrossDirectoryCandidate>,
    /// Cluster hashes suppressed by the ignore file.
    pub ignored: Vec<String>,
    pub candidate_pairs: usize,
}

pub struct DuplicateAnalyzer {
    /// Reviewed cluster hashes to suppress.
    pub ignored_hashes: HashSet<String>,
}

impl Analyzer for DuplicateAnalyzer {
    type Config = AnalysisConfig;
    type Output = DuplicateReport;

    fn run(&self, ctx: &AnalysisContext, config: &AnalysisConfig) -> Result<DuplicateReport> {
        // 1. Candidate pairs from a lower threshold so the distance boost
        //    can rescue far-apart near-misses before final filtering.
        let candidates = ctx
            .vectors
            .similar_pairs(config.candidate_threshold as f32, config.block_size);
        let candidate_pairs = candidates.len();

        // 2. Exclude overlapping same-file byte ranges (nested units), then
        //    rerank and filter.
        let mut surviving: Vec<RankedPair> = Vec::new();
        for pair in candidates {
            let (a, b) = (&ctx.units[pair.a], &ctx.units[pair.b]);
            if a.project_label == b.project_label
                && a.relative_path == b.relative_path
                && byte_ranges_overlap((a.start_byte, a.end_byte), (b.start_byte, b.end_byte))
            {
                continue;
            }
            let exact_copy = a.normalized_body_hash == b.normalized_body_hash;
            if !exact_copy
                && (a.body_node_count < config.min_semantic_body_node_count
                    || b.body_node_count < config.min_semantic_body_node_count)
            {
                continue;
            }
            let boosted = rerank::boosted_score(pair.score, a, b);
            let keep = pair.score >= config.similarity_threshold as f32
                || boosted >= config.rerank_threshold as f32;
            if keep {
                surviving.push(RankedPair {
                    a: pair.a,
                    b: pair.b,
                    raw: pair.score,
                    boosted,
                });
            }
        }
        let surviving = limit_edges_per_unit(ctx, surviving, config.max_edges_per_unit);

        // 3. Union-find connected components, bounded so one bridge-heavy
        // component cannot become an unreviewable report page.
        let components = bounded_components(ctx.units.len(), &surviving, config.max_cluster_size);

        // 4. Build clusters with exact-copy folding and stable hashes.
        let mut clusters: Vec<Cluster> = components
            .into_iter()
            .map(|members| build_cluster(ctx, members, &surviving))
            .collect();
        clusters.sort_by(|x, y| {
            y.top_boosted
                .total_cmp(&x.top_boosted)
                .then(x.hash.cmp(&y.hash))
        });

        // 5. Apply reviewed-cluster hashes.
        let mut ignored = Vec::new();
        clusters.retain(|cluster| {
            if self.ignored_hashes.contains(&cluster.hash) {
                ignored.push(cluster.hash.clone());
                false
            } else {
                true
            }
        });
        ignored.sort();

        // 6. Cross-directory duplication candidates from what survived.
        let cross_directory = cross_directory::cross_directory_candidates(&clusters, &ctx.units);

        Ok(DuplicateReport {
            clusters,
            cross_directory,
            ignored,
            candidate_pairs,
        })
    }
}

fn build_cluster(ctx: &AnalysisContext, members: Vec<usize>, pairs: &[RankedPair]) -> Cluster {
    let member_set: HashSet<usize> = members.iter().copied().collect();
    let mut cluster_pairs: Vec<RankedPair> = pairs
        .iter()
        .filter(|p| member_set.contains(&p.a) && member_set.contains(&p.b))
        .copied()
        .collect();
    cluster_pairs.sort_by(|x, y| {
        y.boosted
            .total_cmp(&x.boosted)
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });

    // Stable hash: sorted (relative path, body hash) fingerprints, so the
    // hash survives reindexing but changes when code moves or changes.
    let mut fingerprints: Vec<String> = members
        .iter()
        .map(|&i| {
            let unit = &ctx.units[i];
            format!("{}\u{0}{}", unit.relative_path, unit.normalized_body_hash)
        })
        .collect();
    fingerprints.sort();
    let hash = sha256_hex(&fingerprints.join("\n"))[..16].to_string();

    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for &member in &members {
        groups
            .entry(ctx.units[member].normalized_body_hash.as_str())
            .or_default()
            .push(member);
    }
    let exact_groups = groups
        .into_iter()
        .map(|(hash, members)| ExactGroup {
            normalized_body_hash: hash.to_string(),
            members,
        })
        .collect();

    Cluster {
        top_raw: cluster_pairs.iter().map(|p| p.raw).fold(0.0, f32::max),
        top_boosted: cluster_pairs.iter().map(|p| p.boosted).fold(0.0, f32::max),
        members,
        pairs: cluster_pairs,
        hash,
        exact_groups,
    }
}

fn limit_edges_per_unit(
    ctx: &AnalysisContext,
    mut pairs: Vec<RankedPair>,
    max_edges_per_unit: usize,
) -> Vec<RankedPair> {
    pairs.sort_by(|x, y| {
        y.boosted
            .total_cmp(&x.boosted)
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });
    let mut counts = vec![0_usize; ctx.units.len()];
    let mut kept = Vec::new();
    for pair in pairs {
        let exact_copy =
            ctx.units[pair.a].normalized_body_hash == ctx.units[pair.b].normalized_body_hash;
        if exact_copy
            || (counts[pair.a] < max_edges_per_unit && counts[pair.b] < max_edges_per_unit)
        {
            counts[pair.a] += 1;
            counts[pair.b] += 1;
            kept.push(pair);
        }
    }
    kept
}

fn bounded_components(
    size: usize,
    pairs: &[RankedPair],
    max_cluster_size: usize,
) -> Vec<Vec<usize>> {
    let mut ordered = pairs.to_vec();
    ordered.sort_by(|x, y| {
        y.boosted
            .total_cmp(&x.boosted)
            .then(x.a.cmp(&y.a))
            .then(x.b.cmp(&y.b))
    });
    let mut parent: Vec<usize> = (0..size).collect();
    let mut rank = vec![0_u8; size];
    let mut component_size = vec![1_usize; size];
    let mut present = vec![false; size];

    for pair in ordered {
        present[pair.a] = true;
        present[pair.b] = true;
        let root_a = find(&mut parent, pair.a);
        let root_b = find(&mut parent, pair.b);
        if root_a == root_b {
            continue;
        }
        if component_size[root_a] + component_size[root_b] > max_cluster_size {
            continue;
        }
        let (new_root, old_root) = match rank[root_a].cmp(&rank[root_b]) {
            std::cmp::Ordering::Less => (root_b, root_a),
            std::cmp::Ordering::Greater => (root_a, root_b),
            std::cmp::Ordering::Equal => {
                rank[root_a] += 1;
                (root_a, root_b)
            }
        };
        parent[old_root] = new_root;
        component_size[new_root] += component_size[old_root];
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (id, is_present) in present.iter().copied().enumerate() {
        if is_present {
            let root = find(&mut parent, id);
            groups.entry(root).or_default().push(id);
        }
    }
    let mut components: Vec<Vec<usize>> = groups.into_values().collect();
    components.retain(|members| members.len() > 1);
    components.sort_by_key(|c| c[0]);
    components
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut current = x;
    while parent[current] != root {
        let next = parent[current];
        parent[current] = root;
        current = next;
    }
    root
}
