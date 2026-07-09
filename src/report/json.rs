//! JSON serializers for analyzer reports, emitted on stdout via `--json`
//! beside (not instead of) the markdown renderer. Schema versions are the
//! stable machine contract; items reuse the query interface's stable IDs.
//! Output is deterministic for a fixed `ReportMeta` (caller supplies the
//! timestamp), matching the markdown renderer's discipline.

use serde_json::{Value, json};

use crate::analyze::compare::{CalibrationInfo, ComparisonReport, MatchClass, MatchRecord};
use crate::analyze::concerns::ConcernReport;
use crate::analyze::context::AnalysisContext;
use crate::analyze::duplicate::{Cluster, ClusterKind, DuplicateReport};
use crate::index::normalizer::sha256_hex;
use crate::query::{model_json, summary_json, unit_id, unit_json};
use crate::report::markdown::ReportMeta;

pub const DUPLICATES_SCHEMA_VERSION: &str = "decombine.duplicates.v1";
pub const CONCERNS_SCHEMA_VERSION: &str = "decombine.concerns.v1";
pub const COMPARE_SCHEMA_VERSION: &str = "decombine.compare.v1";

fn base_envelope(kind: &str, schema_version: &str, meta: &ReportMeta) -> Value {
    json!({
        "schema_version": schema_version,
        "decombine_version": env!("CARGO_PKG_VERSION"),
        "kind": kind,
        "model": model_json(&meta.identity),
        "retention": meta.retention.as_str(),
        "timestamp": meta.timestamp,
        "projects": meta.projects.iter().map(|(label, root)| json!({
            "label": label,
            "source_dir": root,
        })).collect::<Vec<_>>(),
    })
}

fn cluster_kind_str(kind: ClusterKind) -> &'static str {
    match kind {
        ClusterKind::Product => "product",
        ClusterKind::Mixed => "mixed",
        ClusterKind::TestOrDocs => "test_or_docs",
    }
}

fn cluster_json(ctx: &AnalysisContext, cluster: &Cluster) -> Value {
    json!({
        "cluster_id": format!("cluster:{}", cluster.hash),
        "kind": cluster_kind_str(cluster.kind),
        "top_raw": cluster.top_raw,
        "top_boosted": cluster.top_boosted,
        "name_family": cluster.name_family,
        "members": cluster.members.iter().map(|&i| unit_json(&ctx.units[i])).collect::<Vec<_>>(),
        "pairs": cluster.pairs.iter().map(|p| json!({
            "a": unit_id(&ctx.units[p.a]),
            "b": unit_id(&ctx.units[p.b]),
            "raw": p.raw,
            "boosted": p.boosted,
        })).collect::<Vec<_>>(),
        "exact_groups": cluster.exact_groups.iter().map(|g| json!({
            "body_hash": g.normalized_body_hash,
            "members": g.members.iter().map(|&i| unit_id(&ctx.units[i])).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub fn duplicate_json(
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &DuplicateReport,
    limit: Option<usize>,
) -> Value {
    let matched = report.clusters.len();
    let returned = limit.unwrap_or(matched).min(matched);
    let mut value = base_envelope("duplicate_report", DUPLICATES_SCHEMA_VERSION, meta);
    value["analysis"] = json!({
        "candidate_threshold": meta.analysis.candidate_threshold,
        "similarity_threshold": meta.analysis.similarity_threshold,
        "rerank_threshold": meta.analysis.rerank_threshold,
    });
    value["summary"] = summary_json(matched, returned);
    value["summary"]["candidate_pairs"] = json!(report.candidate_pairs);
    value["summary"]["ignored_clusters"] = json!(report.ignored.len());
    value["items"] = Value::Array(
        report.clusters[..returned]
            .iter()
            .map(|c| cluster_json(ctx, c))
            .collect(),
    );
    value["cross_directory"] = Value::Array(
        report
            .cross_directory
            .iter()
            .map(|c| {
                json!({
                    "cluster_id": format!("cluster:{}", c.cluster_hash),
                    "modules": c.modules,
                    "dispersion": c.dispersion,
                    "directory_entropy": c.directory_entropy,
                    "scopes": c.scopes,
                    "generic_helper": c.generic_helper,
                    "score": c.score,
                })
            })
            .collect(),
    );
    value["ignored"] = json!(
        report
            .ignored
            .iter()
            .map(|h| format!("cluster:{h}"))
            .collect::<Vec<_>>()
    );
    value
}

pub fn concern_json(
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &ConcernReport,
    limit: Option<usize>,
) -> Value {
    let matched = report.findings.len();
    let returned = limit.unwrap_or(matched).min(matched);
    let mut value = base_envelope("concern_report", CONCERNS_SCHEMA_VERSION, meta);
    value["summary"] = summary_json(matched, returned);
    value["items"] = Value::Array(
        report.findings[..returned]
            .iter()
            .map(|finding| {
                json!({
                    "name": finding.name,
                    "query": finding.query,
                    "spread": {
                        "files": finding.spread.files,
                        "directories": finding.spread.directories,
                        "top_level_modules": finding.spread.top_level_modules,
                        "directory_entropy": finding.spread.directory_entropy,
                        "dispersion": finding.spread.dispersion,
                    },
                    "units": finding.units.iter().map(|scored| {
                        let mut item = unit_json(&ctx.units[scored.unit]);
                        item["score"] = json!(scored.projection);
                        item
                    }).collect::<Vec<_>>(),
                })
            })
            .collect(),
    );
    value
}

/// Stable over the same (class, member paths, member bodies) regardless of
/// unit index numbering, mirroring the cluster-hash discipline.
fn match_id(ctx: &AnalysisContext, record: &MatchRecord) -> String {
    let side = |members: &[usize]| {
        members
            .iter()
            .map(|&i| {
                let unit = &ctx.units[i];
                format!(
                    "{}\u{0}{}\u{0}{}",
                    unit.project_label, unit.relative_path, unit.normalized_body_hash
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let fingerprint = format!(
        "{}\u{1}{}\u{1}{}",
        record.class.as_str(),
        side(&record.left),
        side(&record.right)
    );
    format!("match:{}", &sha256_hex(&fingerprint)[..16])
}

fn calibration_json(cal: &CalibrationInfo) -> Value {
    json!({
        "applied": cal.applied,
        "sampled_pairs": cal.sampled_pairs,
        "background_mean": cal.background_mean,
        "background_std": cal.background_std,
        "top1_anchor": cal.top1_anchor,
        "same_name_anchor": cal.same_name_anchor,
        "same_name_count": cal.same_name_count,
        "anchor_source": cal.anchor_source,
        "effective_anchor": cal.effective_anchor,
        "candidate_floor": cal.candidate_floor,
        "match_floor": cal.match_floor,
        "candidate_floored": cal.candidate_floored,
        "match_floored": cal.match_floored,
        "margin_required": cal.margin_required,
        "effective_candidate_threshold": cal.effective_candidate_threshold,
        "effective_match_threshold": cal.effective_match_threshold,
    })
}

pub fn comparison_json(
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &ComparisonReport,
    limit: Option<usize>,
) -> Value {
    let matched = report.matches.len();
    let returned = limit.unwrap_or(matched).min(matched);
    let mut value = base_envelope("comparison_report", COMPARE_SCHEMA_VERSION, meta);
    value["left"] = json!(report.left_label);
    value["right"] = json!(report.right_label);
    value["comparison"] = serde_json::to_value(&report.config).unwrap_or(Value::Null);
    value["summary"] = summary_json(matched, returned);
    let counts: Vec<(MatchClass, usize)> = [
        MatchClass::ExactCopy,
        MatchClass::StrongMatch,
        MatchClass::PossibleMatch,
        MatchClass::Split,
        MatchClass::Merge,
        MatchClass::MissingInRight,
        MatchClass::NewInRight,
    ]
    .into_iter()
    .map(|class| (class, report.count(class)))
    .collect();
    for (class, count) in counts {
        value["summary"][class.as_str()] = json!(count);
    }
    value["items"] = Value::Array(
        report.matches[..returned]
            .iter()
            .map(|record| {
                json!({
                    "match_id": match_id(ctx, record),
                    "class": record.class.as_str(),
                    "score": record.score,
                    "hint_bonus": record.hint_bonus,
                    "left": record.left.iter().map(|&i| unit_json(&ctx.units[i])).collect::<Vec<_>>(),
                    "right": record.right.iter().map(|&i| unit_json(&ctx.units[i])).collect::<Vec<_>>(),
                })
            })
            .collect(),
    );
    value["calibration"] = report
        .calibration
        .as_ref()
        .map(calibration_json)
        .unwrap_or(Value::Null);
    value["suppressed_right_candidates"] = Value::Array(
        report
            .suppressed_right_candidates
            .iter()
            .map(|s| {
                let mut item = unit_json(&ctx.units[s.right]);
                item["fanout"] = json!(s.fanout);
                item
            })
            .collect(),
    );
    let coverage_rows = |rows: &[crate::analyze::compare::CoverageRow]| {
        Value::Array(
            rows.iter()
                .map(|row| {
                    json!({
                        "group": row.group,
                        "total_left": row.total_left,
                        "covered": row.covered,
                        "possible": row.possible,
                        "missing": row.missing,
                    })
                })
                .collect(),
        )
    };
    value["coverage_by_directory"] = coverage_rows(&report.coverage_by_directory);
    value["coverage_by_language"] = coverage_rows(&report.coverage_by_language);
    value
}
