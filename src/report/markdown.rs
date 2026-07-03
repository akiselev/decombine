//! Markdown report generation. All output is deterministic for a fixed
//! `ReportMeta` (the caller supplies the timestamp).

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result};

use crate::analyze::compare::{ComparisonReport, MatchClass, MatchRecord};
use crate::analyze::concerns::ConcernReport;
use crate::analyze::context::{AnalysisContext, CodeUnitRef};
use crate::analyze::duplicate::DuplicateReport;
use crate::config::{AnalysisConfig, RetentionMode};
use crate::db::ModelIdentity;

#[derive(Debug, Clone)]
pub struct ReportMeta {
    pub identity: ModelIdentity,
    pub analysis: AnalysisConfig,
    pub retention: RetentionMode,
    /// Supplied by the caller so tests stay deterministic.
    pub timestamp: String,
    /// (label, source root) pairs in scope.
    pub projects: Vec<(String, String)>,
    /// Path of the ignore file, for copyable instructions.
    pub ignore_file: String,
}

fn header(out: &mut String, title: &str, meta: &ReportMeta) {
    let _ = writeln!(out, "# {title}\n");
    let _ = writeln!(
        out,
        "- Model: `{}` backend `{}` v{} ({} dims, provider {}{})",
        meta.identity.model,
        meta.identity.backend,
        meta.identity.backend_version,
        meta.identity.dimensions,
        meta.identity.execution_provider,
        meta.identity
            .quantization
            .as_deref()
            .map(|q| format!(", {q}"))
            .unwrap_or_default(),
    );
    let _ = writeln!(
        out,
        "- Thresholds: candidate {} / similarity {} / rerank {}",
        meta.analysis.candidate_threshold,
        meta.analysis.similarity_threshold,
        meta.analysis.rerank_threshold
    );
    let _ = writeln!(out, "- Retention: {}", meta.retention.as_str());
    let _ = writeln!(out, "- Run: {}", meta.timestamp);
    for (label, root) in &meta.projects {
        let _ = writeln!(out, "- Project `{label}`: {root}");
    }
    let _ = writeln!(out);
}

/// Source text for a unit: stored display source when retained, otherwise
/// reread from disk (`minimal` retention; may degrade if files changed).
fn unit_source(meta: &ReportMeta, unit: &CodeUnitRef) -> String {
    if let Some(source) = &unit.display_source {
        return source.clone();
    }
    let Some((_, root)) = meta.projects.iter().find(|(l, _)| *l == unit.project_label) else {
        return "(source unavailable: unknown project)".to_string();
    };
    let path = Path::new(root).join(&unit.relative_path);
    match std::fs::read_to_string(&path) {
        Ok(text) if unit.end_byte <= text.len() => text[unit.start_byte..unit.end_byte].to_string(),
        _ => "(source unavailable: file changed or missing since indexing)".to_string(),
    }
}

fn location(unit: &CodeUnitRef) -> String {
    format!(
        "`{}:{}` lines {}-{}",
        unit.project_label, unit.relative_path, unit.start_line, unit.end_line
    )
}

fn display_name(unit: &CodeUnitRef) -> String {
    match &unit.scope {
        Some(scope) => format!("{scope}.{}", unit.name),
        None => unit.name.clone(),
    }
}

/// Write `index.md` and `cluster-N.md` files. Returns the file count.
pub fn write_duplicate_report(
    dir: &Path,
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &DuplicateReport,
) -> Result<usize> {
    std::fs::create_dir_all(dir)?;
    let mut index = String::new();
    header(&mut index, "Duplicate code report", meta);
    let _ = writeln!(
        index,
        "{} candidate pairs, {} clusters ({} ignored).\n",
        report.candidate_pairs,
        report.clusters.len(),
        report.ignored.len()
    );

    if report.clusters.is_empty() {
        let _ = writeln!(index, "No duplicate clusters above the thresholds.");
    } else {
        let _ = writeln!(index, "## Clusters\n");
        let _ = writeln!(
            index,
            "| # | Cluster | Units | Top raw | Boosted | Members |"
        );
        let _ = writeln!(index, "| --- | --- | --- | --- | --- | --- |");
        for (rank, cluster) in report.clusters.iter().enumerate() {
            let names: Vec<String> = cluster
                .members
                .iter()
                .take(3)
                .map(|&m| display_name(&ctx.units[m]))
                .collect();
            let suffix = if cluster.members.len() > 3 {
                ", ..."
            } else {
                ""
            };
            let _ = writeln!(
                index,
                "| {n} | [`{hash}`](cluster-{n:02}.md) | {units} | {raw:.4} | {boosted:.4} | {names}{suffix} |",
                n = rank + 1,
                hash = cluster.hash,
                units = cluster.members.len(),
                raw = cluster.top_raw,
                boosted = cluster.top_boosted,
                names = names.join(", "),
            );
        }
        let _ = writeln!(index);
    }

    if !report.cross_directory.is_empty() {
        let _ = writeln!(index, "## Cross-directory duplication\n");
        let _ = writeln!(
            index,
            "Clusters whose members span distant paths or multiple top-level \
             modules. Scope evidence lists distinct receiver/class scopes; \
             generic shared-scope helpers are downranked.\n"
        );
        let _ = writeln!(
            index,
            "| Cluster | Modules | Dispersion | Entropy | Scopes | Generic helper | Score |"
        );
        let _ = writeln!(index, "| --- | --- | --- | --- | --- | --- | --- |");
        for candidate in &report.cross_directory {
            let _ = writeln!(
                index,
                "| `{hash}` | {modules} | {dispersion:.2} | {entropy:.2} | {scopes} | {generic} | {score:.2} |",
                hash = candidate.cluster_hash,
                modules = candidate.modules.join(", "),
                dispersion = candidate.dispersion,
                entropy = candidate.directory_entropy,
                scopes = if candidate.scopes.is_empty() {
                    "-".to_string()
                } else {
                    candidate.scopes.join(", ")
                },
                generic = if candidate.generic_helper {
                    "yes"
                } else {
                    "no"
                },
                score = candidate.score,
            );
        }
        let _ = writeln!(index);
    }

    let _ = writeln!(index, "## Ignoring reviewed clusters\n");
    let _ = writeln!(
        index,
        "Append a cluster hash to `{}` to suppress it. It reappears if the \
         code or its location changes.",
        meta.ignore_file
    );
    if !report.ignored.is_empty() {
        let _ = writeln!(index, "Currently ignored:\n");
        for hash in &report.ignored {
            let _ = writeln!(index, "- `{hash}`");
        }
        let _ = writeln!(index);
    }
    std::fs::write(dir.join("index.md"), index).context("writing index.md")?;

    for (rank, cluster) in report.clusters.iter().enumerate() {
        let mut page = String::new();
        let _ = writeln!(page, "# Cluster {} — `{}`\n", rank + 1, cluster.hash);
        let _ = writeln!(
            page,
            "{} units, top similarity {:.4} (boosted {:.4}). Ignore with:\n",
            cluster.members.len(),
            cluster.top_raw,
            cluster.top_boosted
        );
        let _ = writeln!(page, "```\n{}\n```\n", cluster.hash);

        let _ = writeln!(page, "## Top pairs\n");
        for pair in cluster.pairs.iter().take(5) {
            let _ = writeln!(
                page,
                "- {:.4} (boosted {:.4}): {} ↔ {}",
                pair.raw,
                pair.boosted,
                location(&ctx.units[pair.a]),
                location(&ctx.units[pair.b]),
            );
        }
        let _ = writeln!(page);

        // Exact duplicates are written once with every location listed.
        for group in &cluster.exact_groups {
            let first = &ctx.units[group.members[0]];
            let _ = writeln!(page, "## {}\n", display_name(first));
            for &member in &group.members {
                let _ = writeln!(page, "- {}", location(&ctx.units[member]));
            }
            if group.members.len() > 1 {
                let _ = writeln!(page, "\n{} exact copies of this body.", group.members.len());
            }
            let _ = writeln!(page, "\n```{}", first.language_id);
            let _ = writeln!(page, "{}", unit_source(meta, first).trim_end());
            let _ = writeln!(page, "```\n");
        }
        std::fs::write(dir.join(format!("cluster-{:02}.md", rank + 1)), page)?;
    }
    Ok(report.clusters.len() + 1)
}

/// Write `concerns/index.md` and one file per concern.
pub fn write_concern_report(
    dir: &Path,
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &ConcernReport,
) -> Result<()> {
    let concerns_dir = dir.join("concerns");
    std::fs::create_dir_all(&concerns_dir)?;
    let mut index = String::new();
    header(&mut index, "Candidate concerns", meta);
    let _ = writeln!(
        index,
        "Projection of code units onto configured concern queries. These are \
         **candidate** concerns: embedding similarity is evidence for human \
         review, not proof that a concern exists or is misplaced.\n"
    );
    let _ = writeln!(
        index,
        "| Concern | Units | Files | Dirs | Modules | Entropy | Dispersion |"
    );
    let _ = writeln!(index, "| --- | --- | --- | --- | --- | --- | --- |");
    for finding in &report.findings {
        let _ = writeln!(
            index,
            "| [{name}]({name}.md) | {units} | {files} | {dirs} | {modules} | {entropy:.2} | {dispersion:.2} |",
            name = finding.name,
            units = finding.units.len(),
            files = finding.spread.files,
            dirs = finding.spread.directories,
            modules = finding.spread.top_level_modules,
            entropy = finding.spread.directory_entropy,
            dispersion = finding.spread.dispersion,
        );
    }
    std::fs::write(concerns_dir.join("index.md"), index)?;

    for finding in &report.findings {
        let mut page = String::new();
        let _ = writeln!(page, "# Candidate concern: {}\n", finding.name);
        let _ = writeln!(page, "Query: “{}”\n", finding.query);
        let _ = writeln!(
            page,
            "Structural spread: {} files, {} directories, {} top-level modules, \
             entropy {:.2}, dispersion {:.2}.\n",
            finding.spread.files,
            finding.spread.directories,
            finding.spread.top_level_modules,
            finding.spread.directory_entropy,
            finding.spread.dispersion
        );
        let _ = writeln!(page, "## Top units\n");
        for scored in &finding.units {
            let unit = &ctx.units[scored.unit];
            let _ = writeln!(
                page,
                "- {:.4} — {} ({})",
                scored.projection,
                display_name(unit),
                location(unit)
            );
        }
        if let Some(top) = finding.units.first() {
            let unit = &ctx.units[top.unit];
            let _ = writeln!(page, "\n## Representative unit\n");
            let _ = writeln!(page, "```{}", unit.language_id);
            let _ = writeln!(page, "{}", unit_source(meta, unit).trim_end());
            let _ = writeln!(page, "```");
        }
        std::fs::write(concerns_dir.join(format!("{}.md", finding.name)), page)?;
    }
    Ok(())
}

fn record_line(ctx: &AnalysisContext, record: &MatchRecord) -> String {
    let side = |units: &[usize]| -> String {
        if units.is_empty() {
            "—".to_string()
        } else {
            units
                .iter()
                .map(|&u| {
                    format!(
                        "{} ({})",
                        display_name(&ctx.units[u]),
                        location(&ctx.units[u])
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    format!(
        "- {} ↔ {} (score {:.4}{})",
        side(&record.left),
        side(&record.right),
        record.score,
        if record.hint_bonus > 0.0 {
            format!(", hints +{:.2}", record.hint_bonus)
        } else {
            String::new()
        }
    )
}

/// Write `compare/index.md` plus one detail file per match class.
pub fn write_comparison_report(
    dir: &Path,
    meta: &ReportMeta,
    ctx: &AnalysisContext,
    report: &ComparisonReport,
) -> Result<()> {
    let compare_dir = dir.join("compare");
    std::fs::create_dir_all(&compare_dir)?;

    const CLASSES: &[(MatchClass, &str)] = &[
        (MatchClass::ExactCopy, "Exact copies"),
        (MatchClass::StrongMatch, "Strong matches"),
        (MatchClass::PossibleMatch, "Possible matches"),
        (MatchClass::Split, "Splits (one → many)"),
        (MatchClass::Merge, "Merges (many → one)"),
        (MatchClass::MissingInRight, "Possible missing coverage"),
        (MatchClass::NewInRight, "Possible new behavior"),
    ];

    let mut index = String::new();
    header(
        &mut index,
        &format!(
            "Comparison: `{}` (reference) vs `{}` (candidate)",
            report.left_label, report.right_label
        ),
        meta,
    );
    let _ = writeln!(
        index,
        "Semantic coverage of the reference project by the candidate. \
         Matches are embedding evidence, not proof of behavioral equivalence.\n"
    );
    let _ = writeln!(index, "| Class | Count |");
    let _ = writeln!(index, "| --- | --- |");
    for (class, title) in CLASSES {
        let count = report.count(*class);
        if count > 0 {
            let _ = writeln!(
                index,
                "| [{title}]({file}.md) | {count} |",
                file = class.as_str()
            );
        } else {
            let _ = writeln!(index, "| {title} | 0 |");
        }
    }
    let _ = writeln!(index);

    for (title, rows) in [
        ("Coverage by directory", &report.coverage_by_directory),
        ("Coverage by language", &report.coverage_by_language),
    ] {
        let _ = writeln!(index, "## {title}\n");
        let _ = writeln!(
            index,
            "| Group | Left units | Covered | Possible | Missing |"
        );
        let _ = writeln!(index, "| --- | --- | --- | --- | --- |");
        for row in rows {
            let _ = writeln!(
                index,
                "| `{}` | {} | {} | {} | {} |",
                if row.group.is_empty() {
                    "."
                } else {
                    &row.group
                },
                row.total_left,
                row.covered,
                row.possible,
                row.missing
            );
        }
        let _ = writeln!(index);
    }
    std::fs::write(compare_dir.join("index.md"), index)?;

    for (class, title) in CLASSES {
        let records: Vec<&MatchRecord> = report
            .matches
            .iter()
            .filter(|m| m.class == *class)
            .collect();
        if records.is_empty() {
            continue;
        }
        let mut page = String::new();
        let _ = writeln!(page, "# {title}\n");
        match class {
            MatchClass::MissingInRight => {
                let _ = writeln!(
                    page,
                    "Reference units with no adequate candidate in `{}`. \
                     Possible missing coverage — verify before relying on it.\n",
                    report.right_label
                );
            }
            MatchClass::NewInRight => {
                let _ = writeln!(
                    page,
                    "Candidate units with no adequate counterpart in `{}`. \
                     Possible new behavior.\n",
                    report.left_label
                );
            }
            _ => {}
        }
        for record in records {
            let _ = writeln!(page, "{}", record_line(ctx, record));
        }
        std::fs::write(compare_dir.join(format!("{}.md", class.as_str())), page)?;
    }
    Ok(())
}
