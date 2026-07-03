//! Project comparison: left is the reference project, right is the
//! candidate implementation. Only cross-project edges are generated.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::{Result, bail, ensure};

use crate::analyze::context::{AnalysisContext, Analyzer, CodeUnitRef};
use crate::analyze::paths::{directory_of, top_level_module};
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

#[derive(Debug, Default)]
pub struct ComparisonReport {
    pub left_label: String,
    pub right_label: String,
    pub matches: Vec<MatchRecord>,
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

        // Exact copies first: same normalized body hash on both sides.
        let mut right_by_hash: HashMap<&str, Vec<usize>> = HashMap::new();
        for &r in &right_units {
            right_by_hash
                .entry(ctx.units[r].normalized_body_hash.as_str())
                .or_default()
                .push(r);
        }
        let mut matches: Vec<MatchRecord> = Vec::new();
        let mut matched_left: HashSet<usize> = HashSet::new();
        let mut matched_right: HashSet<usize> = HashSet::new();
        for &l in &left_units {
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
        let pending_left: Vec<usize> = left_units
            .iter()
            .copied()
            .filter(|l| !matched_left.contains(l))
            .collect();
        let pending_right: Vec<usize> = right_units
            .iter()
            .copied()
            .filter(|r| !matched_right.contains(r))
            .collect();
        let threshold = config.candidate_threshold as f32;
        let lr = ctx.vectors.top_k_between(
            &pending_left,
            &pending_right,
            config.top_k_per_unit,
            threshold,
        );
        let rl = ctx.vectors.top_k_between(
            &pending_right,
            &pending_left,
            config.top_k_per_unit,
            threshold,
        );

        // Deduplicate edges (kept from either direction), attach hints.
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

        // Split/merge detection runs before one-to-one matching: a left
        // unit that several rights claim as their best match is a split,
        // even if one of those rights would also be its mutual nearest.
        let match_threshold = config.match_threshold as f32;
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

        Ok(ComparisonReport {
            left_label: self.left_label.clone(),
            right_label: self.right_label.clone(),
            matches,
            coverage_by_directory,
            coverage_by_language,
        })
    }
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
