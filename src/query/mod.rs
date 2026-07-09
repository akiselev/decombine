//! Agent-facing query surface (see docs/research/agent-query-interface.md):
//! stable unit selectors, `--where` metadata filtering, JSON envelopes, and
//! the embeddings-native commands `capabilities`, `inspect`, `units`,
//! `similar`, `search`, and `qbe`. Default output is human-readable text;
//! `--json` puts a bounded envelope on stdout (progress stays on stderr).

use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Value, json};

use crate::analyze::context::{AnalysisContext, CodeUnitRef, load_projects_and_units};
use crate::analyze::vector_store::dot;
use crate::cli::{CapabilitiesArgs, InspectArgs, SearchArgs, SimilarArgs, UnitsArgs};
use crate::config::Config;
use crate::db::{Db, ModelIdentity, Project};
use crate::embed::normalize_in_place;
use crate::index::normalizer::sha256_hex;

pub const QUERY_SCHEMA_VERSION: &str = "decombine.query.v1";
pub const CAPABILITIES_SCHEMA_VERSION: &str = "decombine.capabilities.v1";

// ----- stable IDs and unit serialization -----

/// Deterministic per-index-generation unit selector. Any edit to the unit
/// (content, location, name) changes the ID by design; agents re-resolve
/// after re-indexing.
pub fn unit_id(unit: &CodeUnitRef) -> String {
    let ingredients = [
        unit.project_label.as_str(),
        unit.relative_path.as_str(),
        &unit.start_byte.to_string(),
        &unit.end_byte.to_string(),
        unit.normalized_body_hash.as_str(),
        unit.name.as_str(),
        unit.scope.as_deref().unwrap_or(""),
        unit.language_id.as_str(),
    ]
    .join("\u{0}");
    format!("unit:{}", &sha256_hex(&ingredients)[..16])
}

/// The stable unit object shared by every machine output surface.
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

/// One-line human rendering of a unit, used by the text output modes.
pub fn unit_line(unit: &CodeUnitRef) -> String {
    let scope = unit
        .scope
        .as_deref()
        .map(|s| format!(" ({s})"))
        .unwrap_or_default();
    format!(
        "{} {}:{}:{}-{} {} {}{}",
        unit_id(unit),
        unit.project_label,
        unit.relative_path,
        unit.start_line,
        unit.end_line,
        unit.kind,
        unit.name,
        scope,
    )
}

// ----- --where filter -----

/// Metadata filter parsed from `--where 'key=value key=value ...'`.
/// Different keys AND together; repeating a key ORs its values.
/// `path`/`name`/`scope` accept globs, the rest match exactly.
#[derive(Default)]
pub struct WhereFilter {
    /// Original clauses, for `--why` explanations.
    clauses: Vec<String>,
    project: Vec<String>,
    language: Vec<String>,
    kind: Vec<String>,
    name: Vec<GlobMatcher>,
    scope: Vec<GlobMatcher>,
    path: Vec<GlobMatcher>,
    min_nodes: Option<usize>,
}

impl WhereFilter {
    pub fn parse(expression: Option<&str>) -> Result<WhereFilter> {
        let mut filter = WhereFilter::default();
        let Some(expression) = expression else {
            return Ok(filter);
        };
        for clause in expression.split_whitespace() {
            let Some((key, value)) = clause.split_once('=') else {
                bail!("malformed --where clause {clause:?}: expected key=value");
            };
            match key {
                "project" => filter.project.push(value.to_string()),
                "language" => filter.language.push(value.to_string()),
                "kind" => filter.kind.push(value.to_string()),
                "name" => filter.name.push(glob(value, false)?),
                "scope" => filter.scope.push(glob(value, false)?),
                // `/` is a literal separator so `src/*` means direct children
                // and `src/**` means the whole subtree.
                "path" => filter.path.push(glob(value, true)?),
                "min_nodes" => {
                    filter.min_nodes =
                        Some(value.parse().with_context(|| {
                            format!("min_nodes wants an integer, got {value:?}")
                        })?)
                }
                _ => bail!(
                    "unknown --where key {key:?} (supported: project, language, kind, \
                     name, scope, path, min_nodes)"
                ),
            }
            filter.clauses.push(clause.to_string());
        }
        Ok(filter)
    }

    pub fn matches(&self, unit: &CodeUnitRef) -> bool {
        let any_eq = |list: &[String], v: &str| list.is_empty() || list.iter().any(|x| x == v);
        let any_glob =
            |list: &[GlobMatcher], v: &str| list.is_empty() || list.iter().any(|g| g.is_match(v));
        any_eq(&self.project, &unit.project_label)
            && any_eq(&self.language, &unit.language_id)
            && any_eq(&self.kind, &unit.kind)
            && any_glob(&self.name, &unit.name)
            && (self.scope.is_empty()
                || unit
                    .scope
                    .as_deref()
                    .is_some_and(|s| self.scope.iter().any(|g| g.is_match(s))))
            && any_glob(&self.path, &unit.relative_path)
            && self.min_nodes.is_none_or(|n| unit.body_node_count >= n)
    }

    /// The clauses as given, for explanations.
    pub fn clauses(&self) -> &[String] {
        &self.clauses
    }
}

fn glob(pattern: &str, literal_separator: bool) -> Result<GlobMatcher> {
    Ok(GlobBuilder::new(pattern)
        .literal_separator(literal_separator)
        .build()
        .with_context(|| format!("invalid glob {pattern:?}"))?
        .compile_matcher())
}

// ----- envelope helpers -----

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

/// Result bounding is always reported honestly: `matched` counts everything
/// the query selected, `returned` what the limit let through.
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

/// `stored -> current` renderings of every identity field that differs, so
/// mismatch errors say *which* part of the identity moved (the usual case is
/// a toolchain upgrade behind an identical model name).
fn identity_diff(stored: &ModelIdentity, current: &ModelIdentity) -> Vec<String> {
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".into());
    let mut diffs = Vec::new();
    let mut field = |name: &str, a: String, b: String| {
        if a != b {
            diffs.push(format!("{name} ({a:?} -> {b:?})"));
        }
    };
    field("backend", stored.backend.clone(), current.backend.clone());
    field(
        "backend_version",
        stored.backend_version.clone(),
        current.backend_version.clone(),
    );
    field(
        "runtime_version",
        opt(&stored.runtime_version),
        opt(&current.runtime_version),
    );
    field("model", stored.model.clone(), current.model.clone());
    field("revision", opt(&stored.revision), opt(&current.revision));
    field(
        "dimensions",
        stored.dimensions.to_string(),
        current.dimensions.to_string(),
    );
    field(
        "tokenizer_hash",
        opt(&stored.tokenizer_hash),
        opt(&current.tokenizer_hash),
    );
    field(
        "model_hash",
        opt(&stored.model_hash),
        opt(&current.model_hash),
    );
    field(
        "normalize",
        stored.normalize.to_string(),
        current.normalize.to_string(),
    );
    field(
        "execution_provider",
        stored.execution_provider.clone(),
        current.execution_provider.clone(),
    );
    field(
        "quantization",
        opt(&stored.quantization),
        opt(&current.quantization),
    );
    field(
        "cache_path",
        opt(&stored.cache_path),
        opt(&current.cache_path),
    );
    diffs
}

// ----- selector resolution -----

fn resolve_unit(units: &[CodeUnitRef], selector: &str) -> Result<usize> {
    ensure!(
        selector.starts_with("unit:"),
        "selector {selector:?} is not a unit selector (expected `unit:<id>` \
         as printed by query/report output)"
    );
    units
        .iter()
        .position(|u| unit_id(u) == selector)
        .with_context(|| {
            format!(
                "{selector} not found in the current index. Unit IDs are \
                 deterministic per index generation and change when code is \
                 re-indexed; re-run the query that produced the ID, or list \
                 units with `decombine query units`."
            )
        })
}

/// Unit source text: stored display source when retention kept it, else
/// recovered from the project's source tree by byte range.
fn unit_source(projects: &[Project], unit: &CodeUnitRef) -> Option<String> {
    if let Some(source) = &unit.display_source {
        return Some(source.clone());
    }
    let project = projects.iter().find(|p| p.label == unit.project_label)?;
    let bytes = std::fs::read(Path::new(&project.source_dir).join(&unit.relative_path)).ok()?;
    let slice = bytes.get(unit.start_byte..unit.end_byte)?;
    Some(String::from_utf8_lossy(slice).into_owned())
}

// ----- commands -----

pub fn capabilities(
    config: &Config,
    config_path: &Path,
    db: &Db,
    args: &CapabilitiesArgs,
) -> Result<()> {
    let projects = db.list_projects()?;
    let mut project_rows = Vec::new();
    for project in &projects {
        let files = db.list_files(project.id)?.len();
        let units = db.count_units_for_project(project.id)?;
        project_rows.push((project, files, units));
    }
    let mut stmt = db.conn().prepare(
        "SELECT language_id, COUNT(*) FROM code_units GROUP BY language_id ORDER BY language_id",
    )?;
    let languages: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let models = db.list_models()?;
    let model = models.first();
    let embeddings = model
        .map(|m| -> Result<(i64, i64)> {
            Ok((
                db.count_embeddings(m.id)?,
                db.count_unembedded_hashes(m.id)?,
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
        let value = json!({
            "schema_version": CAPABILITIES_SCHEMA_VERSION,
            "decombine_version": env!("CARGO_PKG_VERSION"),
            "kind": "capabilities",
            "config_path": config_path.display().to_string(),
            "db_path": config.db_file.display().to_string(),
            "report_dir": config.report_dir.display().to_string(),
            "retention": config.index.retention.as_str(),
            "projects": project_rows.iter().map(|(p, files, units)| json!({
                "label": p.label,
                "source_dir": p.source_dir,
                "files": files,
                "units": units,
            })).collect::<Vec<_>>(),
            "languages": languages.iter().map(|(language, units)| json!({
                "language": language,
                "units": units,
            })).collect::<Vec<_>>(),
            "model": model.map(|m| model_json(&m.identity)),
            "embeddings": embeddings.map(|(count, pending)| json!({
                "count": count,
                // Distinct indexed bodies the model has not embedded yet:
                // non-zero means vector results are stale for recent edits.
                "pending_bodies": pending,
            })),
            "display_source_available": display_source,
            "analyzers": {
                "duplicates": true,
                "concerns": {
                    "enabled": config.analysis.concerns.enabled,
                    "queries": config.analysis.concerns.queries.len(),
                },
                "compare": comparison.map(|c| json!({
                    "left": c.left,
                    "right": c.right,
                })),
            },
            "query_commands": {
                "units": true,
                "inspect": true,
                "similar": embeddings.is_some(),
                "search": embeddings.is_some(),
                "qbe": embeddings.is_some(),
            },
        });
        return emit(&value);
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
        Some(m) => println!(
            "model: {} ({} dims, provider {})",
            m.identity.model, m.identity.dimensions, m.identity.execution_provider
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
    let matching: Vec<&CodeUnitRef> = all_units.iter().filter(|u| filter.matches(u)).collect();
    let matched = matching.len();
    let returned = args.limit.unwrap_or(matched).min(matched);

    if args.json {
        let items = matching[..returned].iter().map(|u| unit_json(u)).collect();
        return emit(&envelope(
            "query_result",
            None,
            json!({"mode": "units", "args": {"where": args.r#where, "limit": args.limit}}),
            summary_json(matched, returned),
            items,
        ));
    }
    for unit in &matching[..returned] {
        println!("{}", unit_line(unit));
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

/// Shared engine for `similar` and `qbe` (query by example is the same
/// vector-neighbor lookup with the doc's QBE surface).
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
    let query_row = ctx
        .vectors
        .row_for_unit(query_index)
        .expect("checked above");
    let query_vector = ctx.vectors.vector(query_row).to_vec();
    let mut scored: Vec<(usize, f32)> = (0..ctx.units.len())
        .filter(|&i| i != query_index && filter.matches(&ctx.units[i]))
        .filter_map(|i| {
            let row = ctx.vectors.row_for_unit(i)?;
            let score = dot(&query_vector, ctx.vectors.vector(row));
            (score >= threshold).then_some((i, score))
        })
        .collect();
    scored.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    let matched = scored.len();
    scored.truncate(args.limit);

    if args.json {
        let items = scored
            .iter()
            .map(|&(i, score)| {
                let mut item = unit_json(&ctx.units[i]);
                item["score"] = json!(score);
                if args.why {
                    item["why"] = json!({
                        "scores": {"cosine": score},
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
            summary_json(matched, scored.len()),
            items,
        ));
    }
    println!("query: {}", unit_line(&ctx.units[query_index]));
    for (i, score) in &scored {
        println!("{score:.4} {}", unit_line(&ctx.units[*i]));
    }
    if scored.len() < matched {
        eprintln!(
            "(top {} of {matched} candidates; raise --limit for more)",
            scored.len()
        );
    }
    Ok(())
}

pub fn search(config: &Config, db: &Db, args: &SearchArgs) -> Result<()> {
    let ctx = AnalysisContext::load(db, &[])?;
    let mut embedder = crate::embed::embedder_from_config(config)?;
    let identity = embedder.identity();
    ensure!(
        *identity == ctx.identity,
        "search queries must be embedded with the same model identity as the \
         indexed code units; the configured embedder differs from the \
         database on: {}",
        identity_diff(&ctx.identity, identity).join(", ")
    );
    let mut vectors = embedder.embed(std::slice::from_ref(&args.text))?;
    let query_vector = &mut vectors[0];
    normalize_in_place(query_vector);

    let filter = WhereFilter::parse(args.r#where.as_deref())?;
    let mut scored: Vec<(usize, f32)> = (0..ctx.units.len())
        .filter(|&i| filter.matches(&ctx.units[i]))
        .filter_map(|i| {
            let row = ctx.vectors.row_for_unit(i)?;
            Some((i, dot(ctx.vectors.vector(row), query_vector)))
        })
        .collect();
    scored.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    let matched = scored.len();
    scored.truncate(args.limit);

    if args.json {
        let items = scored
            .iter()
            .map(|&(i, score)| {
                let mut item = unit_json(&ctx.units[i]);
                item["score"] = json!(score);
                if args.why {
                    item["why"] = json!({
                        "scores": {"cosine": score},
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
            summary_json(matched, scored.len()),
            items,
        ));
    }
    for (i, score) in &scored {
        println!("{score:.4} {}", unit_line(&ctx.units[*i]));
    }
    if scored.len() < matched {
        eprintln!(
            "(top {} of {matched} embedded units; raise --limit for more)",
            scored.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(project: &str, path: &str, name: &str, scope: Option<&str>) -> CodeUnitRef {
        CodeUnitRef {
            id: 7,
            project_label: project.into(),
            relative_path: path.into(),
            language_id: "rust".into(),
            kind: "function".into(),
            name: name.into(),
            scope: scope.map(str::to_string),
            start_byte: 10,
            end_byte: 90,
            start_line: 2,
            end_line: 9,
            body_node_count: 20,
            normalized_body_hash: "hash-a".into(),
            display_source: None,
        }
    }

    #[test]
    fn unit_ids_are_deterministic_and_location_sensitive() {
        let a = unit("main", "src/foo.rs", "parse", Some("Parser"));
        assert_eq!(unit_id(&a), unit_id(&a));
        assert!(unit_id(&a).starts_with("unit:"));
        assert_eq!(unit_id(&a).len(), "unit:".len() + 16);

        let mut moved = a.clone();
        moved.start_byte += 1;
        assert_ne!(unit_id(&a), unit_id(&moved));
        let mut renamed = a.clone();
        renamed.name = "parse2".into();
        assert_ne!(unit_id(&a), unit_id(&renamed));
        let mut rescoped = a.clone();
        rescoped.scope = None;
        assert_ne!(unit_id(&a), unit_id(&rescoped));
    }

    #[test]
    fn where_filter_semantics() {
        let filter =
            WhereFilter::parse(Some("language=rust kind=function path=src/** min_nodes=10"))
                .unwrap();
        assert!(filter.matches(&unit("main", "src/a/b.rs", "f", None)));
        assert!(!filter.matches(&unit("main", "tests/a.rs", "f", None)));
        let mut small = unit("main", "src/a.rs", "f", None);
        small.body_node_count = 5;
        assert!(!filter.matches(&small));

        // Repeated keys OR; scope filter excludes scopeless units.
        let filter = WhereFilter::parse(Some("project=a project=b scope=Parser")).unwrap();
        assert!(filter.matches(&unit("a", "x.rs", "f", Some("Parser"))));
        assert!(filter.matches(&unit("b", "x.rs", "f", Some("Parser"))));
        assert!(!filter.matches(&unit("c", "x.rs", "f", Some("Parser"))));
        assert!(!filter.matches(&unit("a", "x.rs", "f", None)));

        // Empty filter matches everything; bad input errors.
        assert!(
            WhereFilter::parse(None)
                .unwrap()
                .matches(&unit("a", "b", "c", None))
        );
        assert!(WhereFilter::parse(Some("nonsense")).is_err());
        assert!(WhereFilter::parse(Some("color=blue")).is_err());
        assert!(WhereFilter::parse(Some("min_nodes=lots")).is_err());
    }

    #[test]
    fn path_glob_uses_literal_separators() {
        let direct = WhereFilter::parse(Some("path=src/*")).unwrap();
        assert!(direct.matches(&unit("m", "src/a.rs", "f", None)));
        assert!(!direct.matches(&unit("m", "src/deep/a.rs", "f", None)));
        let tree = WhereFilter::parse(Some("path=src/**")).unwrap();
        assert!(tree.matches(&unit("m", "src/deep/a.rs", "f", None)));
    }

    #[test]
    fn summary_reports_bounding_honestly() {
        let bounded = summary_json(10, 5);
        assert_eq!(bounded["exhaustive"], json!(false));
        assert_eq!(bounded["has_more"], json!(true));
        let full = summary_json(5, 5);
        assert_eq!(full["exhaustive"], json!(true));
        assert_eq!(full["has_more"], json!(false));
    }
}
