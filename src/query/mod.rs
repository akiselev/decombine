//! Agent-facing query command orchestration over reusable codeindex-query primitives.

use std::path::Path;

use anyhow::{Result, ensure};
use codeindex_query::WhereFilter;
pub use codeindex_query::{unit_id, unit_line};
use codeindex_search::resolve_selector;
use serde_json::{Value, json};

use crate::analyze::context::{AnalysisContext, CodeUnitRef, load_projects_and_units};
use crate::cli::{CapabilitiesArgs, InspectArgs, SearchArgs, SimilarArgs, UnitsArgs};
use crate::config::Config;
use crate::db::{Db, ModelIdentity, Project};

pub const QUERY_SCHEMA_VERSION: &str = "decombine.query.v1";
pub const CAPABILITIES_SCHEMA_VERSION: &str = "decombine.capabilities.v1";

pub fn unit_json(unit: &CodeUnitRef) -> Value {
    json!({
        "unit_id": unit_id(unit),
        "db_unit_id": unit.id,
        "project": unit.project_label,
        "path": unit.relative_path,
        "language": unit.language_id,
        "kind": unit.kind,
        "name": unit.name,
        "scope": unit.scope,
        "byte_range": [unit.start_byte, unit.end_byte],
        "line_range": [unit.start_line, unit.end_line],
        "body_node_count": unit.body_node_count,
        "body_hash": unit.normalized_body_hash,
    })
}

pub fn model_json(identity: &ModelIdentity) -> Value {
    json!({
        "backend": identity.backend,
        "backend_version": identity.backend_version,
        "model": identity.model,
        "dimensions": identity.dimensions,
        "execution_provider": identity.execution_provider,
        "quantization": identity.quantization,
    })
}

pub fn summary_json(matched: usize, returned: usize) -> Value {
    json!({
        "matched": matched,
        "returned": returned,
        "exhaustive": returned >= matched,
        "has_more": returned < matched,
    })
}

fn envelope(
    kind: &str,
    model: Option<&ModelIdentity>,
    query: Value,
    summary: Value,
    items: Vec<Value>,
) -> Value {
    json!({
        "schema_version": QUERY_SCHEMA_VERSION,
        "decombine_version": env!("CARGO_PKG_VERSION"),
        "kind": kind,
        "model": model.map(model_json),
        "query": query,
        "summary": summary,
        "items": items,
    })
}

fn emit(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn resolve_unit(units: &[CodeUnitRef], selector: &str) -> Result<usize> {
    resolve_selector(units, selector)
}

fn unit_source(projects: &[Project], unit: &CodeUnitRef) -> Option<String> {
    if let Some(source) = &unit.display_source {
        return Some(source.clone());
    }
    let project = projects
        .iter()
        .find(|project| project.label == unit.project_label)?;
    let bytes = std::fs::read(Path::new(&project.source_dir).join(&unit.relative_path)).ok()?;
    let slice = bytes.get(unit.start_byte..unit.end_byte)?;
    Some(String::from_utf8_lossy(slice).into_owned())
}

pub fn capabilities(
    config: &Config,
    config_path: &Path,
    db: &Db,
    args: &CapabilitiesArgs,
) -> Result<()> {
    let projects = db.list_projects()?;
    let mut project_rows = Vec::new();
    for project in &projects {
        project_rows.push((
            project,
            db.list_files(project.id)?.len(),
            db.count_units_for_project(project.id)?,
        ));
    }
    let mut statement = db.conn().prepare(
        "SELECT language_id, COUNT(*) FROM code_units GROUP BY language_id ORDER BY language_id",
    )?;
    let languages: Vec<(String, i64)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let models = db.list_models()?;
    let model = models.first();
    let embeddings = model
        .map(|model| -> Result<(i64, i64)> {
            Ok((
                db.count_embeddings(model.id)?,
                db.count_unembedded_hashes(model.id)?,
            ))
        })
        .transpose()?;
    let display_source: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM code_units WHERE display_source IS NOT NULL)",
        [],
        |row| row.get(0),
    )?;
    let comparison = config.comparison.as_ref();

    if args.json {
        return emit(&json!({
            "schema_version": CAPABILITIES_SCHEMA_VERSION,
            "decombine_version": env!("CARGO_PKG_VERSION"),
            "kind": "capabilities",
            "config_path": config_path.display().to_string(),
            "db_path": config.db_file.display().to_string(),
            "report_dir": config.report_dir.display().to_string(),
            "retention": config.index.retention.as_str(),
            "projects": project_rows.iter().map(|(project, files, units)| json!({
                "label": project.label,
                "source_dir": project.source_dir,
                "files": files,
                "units": units,
            })).collect::<Vec<_>>(),
            "languages": languages.iter().map(|(language, units)| json!({
                "language": language,
                "units": units,
            })).collect::<Vec<_>>(),
            "model": model.map(|model| model_json(&model.identity)),
            "embeddings": embeddings.map(|(count, pending)| json!({
                "count": count,
                "pending_bodies": pending,
            })),
            "display_source_available": display_source,
            "analyzers": {
                "duplicates": true,
                "concerns": {
                    "enabled": config.analysis.concerns.enabled,
                    "queries": config.analysis.concerns.queries.len(),
                },
                "compare": comparison.map(|value| json!({
                    "left": value.left,
                    "right": value.right,
                })),
            },
            "query_commands": {
                "units": true,
                "inspect": true,
                "similar": embeddings.is_some(),
                "search": embeddings.is_some(),
                "qbe": embeddings.is_some(),
            },
        }));
    }

    println!("config: {}", config_path.display());
    println!("db: {}", config.db_file.display());
    println!("retention: {}", config.index.retention.as_str());
    for (project, files, units) in &project_rows {
        println!(
            "project {}: {} ({} files, {} units)",
            project.label, project.source_dir, files, units
        );
    }
    for (language, units) in &languages {
        println!("language {language}: {units} units");
    }
    match model {
        Some(model) => println!(
            "model: {} ({} dims, provider {})",
            model.identity.model, model.identity.dimensions, model.identity.execution_provider
        ),
        None => println!("model: none (run `decombine embed`)"),
    }
    if let Some((count, pending)) = embeddings {
        println!("embeddings: {count} bodies ({pending} pending)");
    }
    println!(
        "concerns: {} ({} queries)",
        if config.analysis.concerns.enabled {
            "enabled"
        } else {
            "disabled"
        },
        config.analysis.concerns.queries.len()
    );
    Ok(())
}

pub fn units(db: &Db, args: &UnitsArgs) -> Result<()> {
    let (_, all_units) = load_projects_and_units(db, &[])?;
    let filter = WhereFilter::parse(args.r#where.as_deref())?;
    let matching: Vec<&CodeUnitRef> = all_units
        .iter()
        .filter(|unit| filter.matches(*unit))
        .collect();
    let matched = matching.len();
    let returned = args.limit.unwrap_or(matched).min(matched);
    if args.json {
        let items = matching[..returned]
            .iter()
            .map(|unit| unit_json(unit))
            .collect();
        return emit(&envelope(
            "query_result",
            None,
            json!({"mode": "units", "args": {"where": args.r#where, "limit": args.limit}}),
            summary_json(matched, returned),
            items,
        ));
    }
    for unit in &matching[..returned] {
        println!("{}", unit_line(*unit));
    }
    if returned < matched {
        eprintln!("({returned} of {matched} shown; raise --limit for the rest)");
    }
    Ok(())
}

pub fn inspect(db: &Db, args: &InspectArgs) -> Result<()> {
    let (projects, all_units) = load_projects_and_units(db, &[])?;
    let index = resolve_unit(&all_units, &args.selector)?;
    let unit = &all_units[index];
    let source = args.source.then(|| unit_source(&projects, unit));
    if args.json {
        let mut item = unit_json(unit);
        if let Some(source) = &source {
            item["source"] = json!(source);
        }
        return emit(&envelope(
            "query_result",
            None,
            json!({"mode": "inspect", "args": {"selector": args.selector, "source": args.source}}),
            summary_json(1, 1),
            vec![item],
        ));
    }
    println!("{}", unit_line(unit));
    println!(
        "bytes {}-{}, {} body nodes, body hash {}",
        unit.start_byte, unit.end_byte, unit.body_node_count, unit.normalized_body_hash
    );
    match source {
        Some(Some(source)) => println!("\n{source}"),
        Some(None) => println!("\n(source unavailable: not retained and not readable on disk)"),
        None => {}
    }
    Ok(())
}

pub fn similar(db: &Db, args: &SimilarArgs, mode: &str) -> Result<()> {
    let ctx = AnalysisContext::load(db, &[])?;
    let query_index = resolve_unit(&ctx.units, &args.unit)?;
    ensure!(
        ctx.vectors.row_for_unit(query_index).is_some(),
        "{} has no stored embedding; run `decombine embed` first",
        args.unit
    );
    let filter = WhereFilter::parse(args.r#where.as_deref())?;
    let threshold = args.threshold.unwrap_or(-1.0);
    let results = ctx.similar_to_unit(query_index, &filter, args.limit, threshold)?;
    let matched = results.matched;
    let hits = &results.hits;

    if args.json {
        let items = hits
            .iter()
            .map(|hit| {
                let mut item = unit_json(&ctx.units[hit.index]);
                item["score"] = json!(hit.score);
                if args.why {
                    item["why"] = json!({
                        "scores": {"cosine": hit.score},
                        "evidence": [{"source": "vector", "unit": args.unit}],
                        "decision": {
                            "reason": "vector_rank",
                            "filters": filter.clauses(),
                            "threshold": args.threshold,
                            "suppressed": false,
                        },
                    });
                }
                item
            })
            .collect();
        return emit(&envelope(
            "query_result",
            Some(&ctx.identity),
            json!({
                "mode": mode,
                "args": {
                    "unit": args.unit,
                    "limit": args.limit,
                    "threshold": args.threshold,
                    "where": args.r#where,
                },
            }),
            summary_json(matched, hits.len()),
            items,
        ));
    }
    println!("query: {}", unit_line(&ctx.units[query_index]));
    for hit in hits {
        println!("{:.4} {}", hit.score, unit_line(&ctx.units[hit.index]));
    }
    if hits.len() < matched {
        eprintln!(
            "(top {} of {matched} candidates; raise --limit for more)",
            hits.len()
        );
    }
    Ok(())
}

pub fn search(config: &Config, db: &Db, args: &SearchArgs) -> Result<()> {
    let ctx = AnalysisContext::load(db, &[])?;
    let mut embedder = crate::embed::embedder_from_config(config)?;
    let filter = WhereFilter::parse(args.r#where.as_deref())?;
    let results = ctx.search_text(embedder.as_mut(), &args.text, &filter, args.limit)?;
    let matched = results.matched;
    let hits = &results.hits;

    if args.json {
        let items = hits
            .iter()
            .map(|hit| {
                let mut item = unit_json(&ctx.units[hit.index]);
                item["score"] = json!(hit.score);
                if args.why {
                    item["why"] = json!({
                        "scores": {"cosine": hit.score},
                        "evidence": [{"source": "vector", "query": args.text}],
                        "decision": {
                            "reason": "vector_rank",
                            "filters": filter.clauses(),
                            "suppressed": false,
                        },
                    });
                }
                item
            })
            .collect();
        return emit(&envelope(
            "query_result",
            Some(&ctx.identity),
            json!({
                "mode": "search",
                "args": {"text": args.text, "limit": args.limit, "where": args.r#where},
            }),
            summary_json(matched, hits.len()),
            items,
        ));
    }
    for hit in hits {
        println!("{:.4} {}", hit.score, unit_line(&ctx.units[hit.index]));
    }
    if hits.len() < matched {
        eprintln!(
            "(top {} of {matched} embedded units; raise --limit for more)",
            hits.len()
        );
    }
    Ok(())
}
