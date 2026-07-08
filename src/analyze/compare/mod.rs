//! Project comparison: left is the reference project, right is the
//! candidate implementation. Only cross-project edges are generated.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Result, bail, ensure};

use crate::analyze::context::{AnalysisContext, Analyzer, CodeUnitRef};
use crate::analyze::paths::{directory_of, top_level_module};
use crate::analyze::vector_store::VectorStore;
use crate::config::ComparisonConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchClass {
    ExactCopy,
    StrongMatch,
    PossibleMatch,
    Split,
    Merge,
    MissingInRight,
    NewInRight,
}

impl MatchClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchClass::ExactCopy => "exact_copy",
            MatchClass::StrongMatch => "strong_match",
            MatchClass::PossibleMatch => "possible_match",
            MatchClass::Split => "split",
            MatchClass::Merge => "merge",
            MatchClass::MissingInRight => "missing_in_right",
            MatchClass::NewInRight => "new_in_right",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchRecord {
    pub class: MatchClass,
    /// Unit indices on the left/right side (splits and merges have several
    /// on one side).
    pub left: Vec<usize>,
    pub right: Vec<usize>,
    /// Best raw cosine among the involved edges (1.0 for exact copies).
    pub score: f32,
    /// Bounded name/path hint bonus that influenced candidate ranking.
    pub hint_bonus: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CoverageRow {
    pub group: String,
    pub total_left: usize,
    pub covered: usize,
    pub possible: usize,
    pub missing: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedCandidate {
    pub right: usize,
    pub fanout: usize,
}

/// Result of mapping normalized threshold positions into this run's raw
/// cosine scale. Background similarity anchors 0.0; the 95th percentile of
/// per-left-unit top-1 scores anchors 1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationInfo {
    pub applied: bool,
    pub sampled_pairs: usize,
    pub background_mean: f32,
    pub background_std: f32,
    pub top1_anchor: f32,
    pub effective_candidate_threshold: f32,
    pub effective_match_threshold: f32,
}

#[derive(Debug, Default)]
pub struct ComparisonReport {
    pub left_label: String,
    pub right_label: String,
    pub config: ComparisonConfig,
    pub matches: Vec<MatchRecord>,
    /// Right-side units suppressed because too many left units selected them
    /// as the best semantic candidate.
    pub suppressed_right_candidates: Vec<SuppressedCandidate>,
    /// Present when `comparison.calibration` is not `none`.
    pub calibration: Option<CalibrationInfo>,
    /// Coverage aggregated by left-side directory and by language.
    pub coverage_by_directory: Vec<CoverageRow>,
    pub coverage_by_language: Vec<CoverageRow>,
}

impl ComparisonReport {
    pub fn count(&self, class: MatchClass) -> usize {
        self.matches.iter().filter(|m| m.class == class).count()
    }
}

pub struct CompareAnalyzer {
    pub left_label: String,
    pub right_label: String,
}

/// Bounded hint bonus: never enough to lift a weak edge to strong.
pub const MAX_HINT_BONUS: f32 = 0.04;

fn hint_bonus(config: &ComparisonConfig, a: &CodeUnitRef, b: &CodeUnitRef) -> f32 {
    let mut bonus: f32 = 0.0;
    if config.use_name_hints {
        let (name_a, name_b) = (a.name.to_lowercase(), b.name.to_lowercase());
        if name_a == name_b {
            bonus += 0.02;
        } else if name_a.contains(&name_b) || name_b.contains(&name_a) {
            bonus += 0.01;
        }
    }
    if config.use_path_hints {
        if directory_of(&a.relative_path) == directory_of(&b.relative_path) {
            bonus += 0.02;
        } else if top_level_module(&a.relative_path) == top_level_module(&b.relative_path) {
            bonus += 0.01;
        }
    }
    bonus.min(MAX_HINT_BONUS)
}

fn eligible_for_matching(config: &ComparisonConfig, unit: &CodeUnitRef) -> bool {
    unit.body_node_count >= config.min_body_node_count
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    left: usize,
    right: usize,
    raw: f32,
    hint: f32,
}

impl Edge {
    fn ranked(&self) -> f32 {
        self.raw + self.hint
    }
}

impl Analyzer for CompareAnalyzer {
    type Config = ComparisonConfig;
    type Output = ComparisonReport;

    fn run(&self, ctx: &AnalysisContext, config: &ComparisonConfig) -> Result<ComparisonReport> {
        self.run_with_progress(ctx, config, |_| {})
    }
}

impl CompareAnalyzer {
    pub fn run_with_progress(
        &self,
        ctx: &AnalysisContext,
        config: &ComparisonConfig,
        mut progress: impl FnMut(&str),
    ) -> Result<ComparisonReport> {
        ensure!(
            self.left_label != self.right_label,
            "comparison `left` and `right` must be different projects"
        );
        if ctx.projects.len() != 2 {
            bail!(
                "comparison needs a context with exactly two projects, got {}",
                ctx.projects.len()
            );
        }
        for label in [&self.left_label, &self.right_label] {
            if !ctx.projects.iter().any(|p| &p.label == label) {
                bail!("project {label:?} is not part of this analysis context");
            }
        }

        let left_units = ctx.unit_indices_for_project(&self.left_label);
        let right_units = ctx.unit_indices_for_project(&self.right_label);
        let eligible_left: Vec<usize> = left_units
            .iter()
            .copied()
            .filter(|&l| eligible_for_matching(config, &ctx.units[l]))
            .collect();
        let eligible_right: Vec<usize> = right_units
            .iter()
            .copied()
            .filter(|&r| eligible_for_matching(config, &ctx.units[r]))
            .collect();
        progress("classifying exact copies");

        // Exact copies first: same normalized body hash on both sides.
        let mut right_by_hash: HashMap<&str, Vec<usize>> = HashMap::new();
        for &r in &eligible_right {
            right_by_hash
                .entry(ctx.units[r].normalized_body_hash.as_str())
                .or_default()
                .push(r);
        }
        let mut matches: Vec<MatchRecord> = Vec::new();
        let mut matched_left: HashSet<usize> = HashSet::new();
        let mut matched_right: HashSet<usize> = HashSet::new();
        for &l in &eligible_left {
            if let Some(rights) = right_by_hash.get(ctx.units[l].normalized_body_hash.as_str()) {
                matched_left.insert(l);
                matched_right.extend(rights);
                matches.push(MatchRecord {
                    class: MatchClass::ExactCopy,
                    left: vec![l],
                    right: rights.clone(),
                    score: 1.0,
                    hint_bonus: 0.0,
                });
            }
        }

        // Cross-project candidate edges only, top-k in both directions.
        progress("building cross-project candidate edges");
        let pending_left: Vec<usize> = eligible_left
            .iter()
            .copied()
            .filter(|l| !matched_left.contains(l))
            .collect();
        let pending_right: Vec<usize> = eligible_right
            .iter()
            .copied()
            .filter(|r| !matched_right.contains(r))
            .collect();
        let transformed = (config.abtt_directions > 0).then(|| {
            progress("removing common embedding directions (abtt)");
            abtt_store(ctx, config.abtt_directions)
        });
        let vectors: &VectorStore = transformed.as_ref().unwrap_or(&ctx.vectors);

        let calibration = if config.calibration == "background" {
            progress("calibrating thresholds against background similarity");
            calibrate(vectors, config, &pending_left, &pending_right)
        } else {
            None
        };
        let (threshold, match_threshold) = match &calibration {
            Some(cal) if cal.applied => (
                cal.effective_candidate_threshold,
                cal.effective_match_threshold,
            ),
            _ => (
                config.candidate_threshold as f32,
                config.match_threshold as f32,
            ),
        };
        let lr = vectors.top_k_between(
            &pending_left,
            &pending_right,
            config.top_k_per_unit,
            threshold,
        );
        progress("building reverse candidate edges");
        let rl = vectors.top_k_between(
            &pending_right,
            &pending_left,
            config.top_k_per_unit,
            threshold,
        );

        // Deduplicate edges (kept from either direction), attach hints.
        progress("ranking candidate edges");
        let mut edges: BTreeMap<(usize, usize), Edge> = BTreeMap::new();
        for hits in lr.iter().chain(rl.iter()) {
            for pair in hits {
                // top_k_between emits (from=a, to=b); normalize to (l, r).
                let (l, r) = if pending_left.contains(&pair.a) {
                    (pair.a, pair.b)
                } else {
                    (pair.b, pair.a)
                };
                edges.entry((l, r)).or_insert_with(|| Edge {
                    left: l,
                    right: r,
                    raw: pair.score,
                    hint: hint_bonus(config, &ctx.units[l], &ctx.units[r]),
                });
            }
        }

        // Best candidate per unit uses hint-adjusted ranking; strong-match
        // classification requires the *raw* score to clear the threshold,
        // so hints can reorder candidates but never rescue weak edges.
        let mut best_right_for_left: HashMap<usize, Edge> = HashMap::new();
        let mut best_left_for_right: HashMap<usize, Edge> = HashMap::new();
        for edge in edges.values() {
            let better =
                |current: Option<&Edge>| current.is_none_or(|c| edge.ranked() > c.ranked());
            if better(best_right_for_left.get(&edge.left)) {
                best_right_for_left.insert(edge.left, *edge);
            }
            if better(best_left_for_right.get(&edge.right)) {
                best_left_for_right.insert(edge.right, *edge);
            }
        }
        let mut suppressed_right_candidates = Vec::new();
        if config.max_right_candidate_fanout > 0 {
            progress("suppressing high-fanout candidate targets");
            let mut fanout: HashMap<usize, usize> = HashMap::new();
            for edge in best_right_for_left.values() {
                *fanout.entry(edge.right).or_default() += 1;
            }
            suppressed_right_candidates = fanout
                .into_iter()
                .filter_map(|(right, count)| {
                    (count > config.max_right_candidate_fanout).then_some(SuppressedCandidate {
                        right,
                        fanout: count,
                    })
                })
                .collect();
            suppressed_right_candidates
                .sort_by(|a, b| b.fanout.cmp(&a.fanout).then_with(|| a.right.cmp(&b.right)));
            let suppressed: HashSet<usize> = suppressed_right_candidates
                .iter()
                .map(|candidate| candidate.right)
                .collect();
            best_right_for_left.retain(|_, edge| !suppressed.contains(&edge.right));
            best_left_for_right.retain(|right, _| !suppressed.contains(right));
        }

        // Split/merge detection runs before one-to-one matching: a left
        // unit that several rights claim as their best match is a split,
        // even if one of those rights would also be its mutual nearest.
        progress("classifying split and merge candidates");
        let mut rights_claiming_left: BTreeMap<usize, Vec<(usize, f32, f32)>> = BTreeMap::new();
        for (&r, edge) in &best_left_for_right {
            if edge.raw >= match_threshold {
                rights_claiming_left
                    .entry(edge.left)
                    .or_default()
                    .push((r, edge.raw, edge.hint));
            }
        }
        for (l, mut rights) in rights_claiming_left {
            rights.retain(|(r, _, _)| !matched_right.contains(r));
            if rights.len() < 2 || matched_left.contains(&l) {
                continue;
            }
            rights.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
            matched_left.insert(l);
            matched_right.extend(rights.iter().map(|(r, _, _)| *r));
            matches.push(MatchRecord {
                class: MatchClass::Split,
                left: vec![l],
                score: rights[0].1,
                hint_bonus: rights[0].2,
                right: {
                    let mut members: Vec<usize> = rights.into_iter().map(|(r, _, _)| r).collect();
                    members.sort_unstable();
                    members
                },
            });
        }
        let mut lefts_claiming_right: BTreeMap<usize, Vec<(usize, f32, f32)>> = BTreeMap::new();
        for (&l, edge) in &best_right_for_left {
            if edge.raw >= match_threshold && !matched_left.contains(&l) {
                lefts_claiming_right
                    .entry(edge.right)
                    .or_default()
                    .push((l, edge.raw, edge.hint));
            }
        }
        for (r, mut lefts) in lefts_claiming_right {
            lefts.retain(|(l, _, _)| !matched_left.contains(l));
            if lefts.len() < 2 || matched_right.contains(&r) {
                continue;
            }
            lefts.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
            matched_right.insert(r);
            matched_left.extend(lefts.iter().map(|(l, _, _)| *l));
            matches.push(MatchRecord {
                class: MatchClass::Merge,
                right: vec![r],
                score: lefts[0].1,
                hint_bonus: lefts[0].2,
                left: {
                    let mut members: Vec<usize> = lefts.into_iter().map(|(l, _, _)| l).collect();
                    members.sort_unstable();
                    members
                },
            });
        }

        // Mutual nearest neighbors among what remains → strong matches.
        progress("classifying mutual strong matches");
        for &l in &pending_left {
            if matched_left.contains(&l) {
                continue;
            }
            let Some(edge) = best_right_for_left.get(&l) else {
                continue;
            };
            if matched_right.contains(&edge.right) {
                continue;
            }
            let mutual = best_left_for_right
                .get(&edge.right)
                .is_some_and(|back| back.left == l);
            if mutual && edge.raw >= match_threshold {
                matched_left.insert(l);
                matched_right.insert(edge.right);
                matches.push(MatchRecord {
                    class: MatchClass::StrongMatch,
                    left: vec![l],
                    right: vec![edge.right],
                    score: edge.raw,
                    hint_bonus: edge.hint,
                });
            }
        }

        // Possible matches: candidate edge exists but nothing was strong.
        progress("classifying possible matches and leftovers");
        for &l in &pending_left {
            if matched_left.contains(&l) {
                continue;
            }
            if let Some(edge) = best_right_for_left.get(&l) {
                matched_left.insert(l);
                matches.push(MatchRecord {
                    class: MatchClass::PossibleMatch,
                    left: vec![l],
                    right: vec![edge.right],
                    score: edge.raw,
                    hint_bonus: edge.hint,
                });
            }
        }

        // Unmatched leftovers.
        for &l in &left_units {
            if !matched_left.contains(&l) {
                matches.push(MatchRecord {
                    class: MatchClass::MissingInRight,
                    left: vec![l],
                    right: vec![],
                    score: 0.0,
                    hint_bonus: 0.0,
                });
            }
        }
        for &r in &right_units {
            if !matched_right.contains(&r)
                && !matches
                    .iter()
                    .any(|m| m.class == MatchClass::PossibleMatch && m.right.contains(&r))
            {
                matches.push(MatchRecord {
                    class: MatchClass::NewInRight,
                    left: vec![],
                    right: vec![r],
                    score: 0.0,
                    hint_bonus: 0.0,
                });
            }
        }

        matches.sort_by(|x, y| {
            x.class
                .cmp(&y.class)
                .then(y.score.total_cmp(&x.score))
                .then(x.left.cmp(&y.left))
                .then(x.right.cmp(&y.right))
        });

        let coverage_by_directory = coverage(ctx, &matches, &left_units, |u| {
            directory_of(&u.relative_path).to_string()
        });
        let coverage_by_language = coverage(ctx, &matches, &left_units, |u| u.language_id.clone());

        progress("finished comparison classification");
        Ok(ComparisonReport {
            left_label: self.left_label.clone(),
            right_label: self.right_label.clone(),
            config: config.clone(),
            matches,
            suppressed_right_candidates,
            calibration,
            coverage_by_directory,
            coverage_by_language,
        })
    }
}

/// All-but-the-top: subtract the corpus mean, project out the top `m`
/// principal directions of the centered vectors, and renormalize. Removes
/// the shared "boilerplate + model anisotropy" component so cosine scores
/// spread over the full range. Top directions come from deterministic power
/// iteration with deflation on the implicit covariance (no dense `d x d`
/// matrix, no external linear-algebra dependency).
fn abtt_store(ctx: &AnalysisContext, m: usize) -> VectorStore {
    let d = ctx.vectors.dimensions();
    let rows: Vec<(usize, &[f32])> = (0..ctx.units.len())
        .filter_map(|u| {
            ctx.vectors
                .row_for_unit(u)
                .map(|r| (u, ctx.vectors.vector(r)))
        })
        .collect();
    if rows.is_empty() || d == 0 {
        return VectorStore::from_unit_vectors(d, vec![None; ctx.units.len()]);
    }

    let mut mean = vec![0f64; d];
    for (_, v) in &rows {
        for (acc, x) in mean.iter_mut().zip(v.iter()) {
            *acc += f64::from(*x);
        }
    }
    for x in &mut mean {
        *x /= rows.len() as f64;
    }
    let centered: Vec<Vec<f64>> = rows
        .iter()
        .map(|(_, v)| {
            v.iter()
                .zip(&mean)
                .map(|(x, mu)| f64::from(*x) - mu)
                .collect()
        })
        .collect();

    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
    let mut directions: Vec<Vec<f64>> = Vec::with_capacity(m);
    for k in 0..m.min(d) {
        // Deterministic pseudo-random start, orthogonal to found directions.
        let mut w: Vec<f64> = (0..d)
            .map(|i| {
                let h = (i as u64)
                    .wrapping_mul(0x9e3779b97f4a7c15)
                    .wrapping_add(k as u64 + 1);
                (h >> 33) as f64 / f64::from(u32::MAX) - 0.25
            })
            .collect();
        for _ in 0..100 {
            // y = C w without forming C: sum over centered rows.
            let mut y = vec![0f64; d];
            for c in &centered {
                let proj = dot(c, &w);
                for (yi, ci) in y.iter_mut().zip(c) {
                    *yi += proj * ci;
                }
            }
            for u in &directions {
                let proj = dot(&y, u);
                for (yi, ui) in y.iter_mut().zip(u) {
                    *yi -= proj * ui;
                }
            }
            let norm = dot(&y, &y).sqrt();
            if norm < 1e-12 {
                break;
            }
            for yi in &mut y {
                *yi /= norm;
            }
            let converged = dot(&y, &w).abs() > 1.0 - 1e-10;
            w = y;
            if converged {
                break;
            }
        }
        directions.push(w);
    }

    let mut out: Vec<Option<Vec<f32>>> = vec![None; ctx.units.len()];
    for ((unit, _), c) in rows.iter().zip(&centered) {
        let mut v = c.clone();
        for u in &directions {
            let proj = dot(&v, u);
            for (vi, ui) in v.iter_mut().zip(u) {
                *vi -= proj * ui;
            }
        }
        let norm = dot(&v, &v).sqrt();
        let projected: Vec<f32> = if norm > 1e-9 {
            v.iter().map(|x| (x / norm) as f32).collect()
        } else {
            // Degenerate: the vector was entirely inside the removed
            // subspace; it can no longer match anything semantically.
            vec![0.0; d]
        };
        out[*unit] = Some(projected);
    }
    VectorStore::from_unit_vectors(d, out)
}

/// Anchors must be separated by at least this much raw cosine before the
/// normalized thresholds are trusted; otherwise the corpus has no usable
/// signal range and calibration falls back to the configured raw cutoffs.
const MIN_CALIBRATION_RANGE: f32 = 0.05;

fn calibrate(
    vectors: &VectorStore,
    config: &ComparisonConfig,
    pending_left: &[usize],
    pending_right: &[usize],
) -> Option<CalibrationInfo> {
    let left_rows: Vec<usize> = pending_left
        .iter()
        .filter_map(|&l| vectors.row_for_unit(l))
        .collect();
    let right_rows: Vec<usize> = pending_right
        .iter()
        .filter_map(|&r| vectors.row_for_unit(r))
        .collect();
    if left_rows.is_empty() || right_rows.is_empty() {
        return None;
    }

    // Background: deterministic LCG sample over the cross product (or the
    // full cross product when it is small enough).
    let total = left_rows.len().saturating_mul(right_rows.len());
    let sample = config.calibration_sample_pairs.min(total);
    let mut background: Vec<f32> = Vec::with_capacity(sample);
    if total <= config.calibration_sample_pairs {
        for &a in &left_rows {
            for &b in &right_rows {
                background.push(vectors.dot(a, b));
            }
        }
    } else {
        let mut state: u64 = 0x9e3779b97f4a7c15 ^ (total as u64);
        for _ in 0..sample {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let a = left_rows[(state >> 33) as usize % left_rows.len()];
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let b = right_rows[(state >> 33) as usize % right_rows.len()];
            background.push(vectors.dot(a, b));
        }
    }
    let n = background.len() as f32;
    let mean = background.iter().sum::<f32>() / n;
    let variance = background.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / n;
    let std = variance.sqrt();

    // Anchor: 95th percentile of per-left-unit top-1 scores, robust to a
    // handful of outlier near-duplicates while tracking the score scale the
    // model assigns to its best available matches.
    let mut top1: Vec<f32> = vectors
        .top_k_between(pending_left, pending_right, 1, -1.0)
        .iter()
        .filter_map(|hits| hits.first().map(|pair| pair.score))
        .collect();
    if top1.is_empty() {
        return None;
    }
    top1.sort_by(f32::total_cmp);
    let anchor = top1[((top1.len() - 1) as f32 * 0.95).round() as usize];

    let range = anchor - mean;
    let applied = range >= MIN_CALIBRATION_RANGE;
    let (candidate, match_threshold) = if applied {
        (
            mean + config.candidate_threshold as f32 * range,
            mean + config.match_threshold as f32 * range,
        )
    } else {
        (
            config.candidate_threshold as f32,
            config.match_threshold as f32,
        )
    };
    Some(CalibrationInfo {
        applied,
        sampled_pairs: background.len(),
        background_mean: mean,
        background_std: std,
        top1_anchor: anchor,
        effective_candidate_threshold: candidate,
        effective_match_threshold: match_threshold,
    })
}

fn coverage(
    ctx: &AnalysisContext,
    matches: &[MatchRecord],
    left_units: &[usize],
    group_of: impl Fn(&CodeUnitRef) -> String,
) -> Vec<CoverageRow> {
    let mut class_of_left: HashMap<usize, MatchClass> = HashMap::new();
    for record in matches {
        for &l in &record.left {
            // The classification pass assigns each left unit exactly once.
            class_of_left.entry(l).or_insert(record.class);
        }
    }
    let mut rows: BTreeMap<String, CoverageRow> = BTreeMap::new();
    for &l in left_units {
        let group = group_of(&ctx.units[l]);
        let row = rows.entry(group.clone()).or_insert_with(|| CoverageRow {
            group,
            ..CoverageRow::default()
        });
        row.total_left += 1;
        match class_of_left.get(&l) {
            Some(
                MatchClass::ExactCopy
                | MatchClass::StrongMatch
                | MatchClass::Split
                | MatchClass::Merge,
            ) => row.covered += 1,
            Some(MatchClass::PossibleMatch) => row.possible += 1,
            _ => row.missing += 1,
        }
    }
    rows.into_values().collect()
}
