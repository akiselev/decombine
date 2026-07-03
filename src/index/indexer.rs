use std::collections::HashSet;
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};

use crate::config::{Config, RetentionMode};
use crate::db::{Db, NewCodeUnit, NewFile};
use crate::index::extractor::{ExtractOptions, extract_units};
use crate::index::language::LanguageRegistry;
use crate::index::normalizer::sha256_hex;
use crate::index::scanner::scan_files;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectStats {
    pub label: String,
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
    pub failed: usize,
    pub units: usize,
}

/// Index every configured project (or just `only_label`), incrementally.
pub fn index(db: &Db, config: &Config, only_label: Option<&str>) -> Result<Vec<ProjectStats>> {
    // Settings that must not change once units exist in this database.
    db.check_or_set_immutable(
        "index.body_node_count_threshold",
        &config.analysis.body_node_count_threshold.to_string(),
    )?;
    db.check_or_set_immutable("index.retention", config.index.retention.as_str())?;
    db.check_or_set_immutable(
        "embedding.max_body_chars",
        &config.embedding.max_body_chars.to_string(),
    )?;

    let mut stats = Vec::new();
    for project in config.resolved_projects() {
        if let Some(label) = only_label
            && project.label != label
        {
            continue;
        }
        stats.push(index_project(db, config, &project)?);
    }
    if let Some(label) = only_label
        && stats.is_empty()
    {
        anyhow::bail!("no configured project labeled {label:?}");
    }
    let pruned = db.prune_orphan_embeddings()?;
    if pruned > 0 {
        eprintln!("pruned {pruned} orphaned embeddings");
    }
    Ok(stats)
}

fn index_project(
    db: &Db,
    config: &Config,
    project: &crate::config::ResolvedProject,
) -> Result<ProjectStats> {
    let root = &project.source_dir;
    let project_id = db.upsert_project(&project.label, &root.to_string_lossy())?;
    let enabled: HashSet<String> = config.languages.enabled.iter().cloned().collect();
    let scanned = scan_files(root, &project.exclude, &enabled)?;

    let options = ExtractOptions {
        body_node_count_threshold: config.analysis.body_node_count_threshold,
        max_body_chars: config.embedding.max_body_chars,
    };
    let registry = LanguageRegistry::global();

    let mut stats = ProjectStats {
        label: project.label.clone(),
        ..ProjectStats::default()
    };
    let mut seen: HashSet<String> = HashSet::with_capacity(scanned.len());
    for file in &scanned {
        seen.insert(file.relative_path.clone());
        let existing = db.get_file(project_id, &file.relative_path)?;

        let metadata = match std::fs::metadata(&file.absolute_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                eprintln!("failed to stat {}: {error}", file.absolute_path.display());
                stats.failed += 1;
                continue;
            }
        };
        let mtime_ns = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        let size = metadata.len() as i64;

        if let Some(record) = &existing
            && record.mtime_ns == mtime_ns
            && record.size == size
        {
            stats.skipped += 1;
            continue;
        }

        let source = match std::fs::read_to_string(&file.absolute_path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("failed to read {}: {error}", file.absolute_path.display());
                stats.failed += 1;
                continue;
            }
        };
        let source_hash = sha256_hex(&source);
        if let Some(record) = &existing
            && record.source_hash == source_hash
        {
            // Touched but unchanged: refresh metadata, keep units.
            db.update_file_meta(record.id, mtime_ns, size)?;
            stats.skipped += 1;
            continue;
        }

        let def = registry
            .get(&file.language_id)
            .context("scanner produced an unregistered language")?;
        let mut units = match extract_units(def, &source, &options) {
            Ok(units) => units,
            Err(error) => {
                eprintln!("failed to parse {}: {error}", file.absolute_path.display());
                stats.failed += 1;
                continue;
            }
        };
        apply_retention(&mut units, config.index.retention);

        let file_id = db.upsert_file(&NewFile {
            project_id,
            relative_path: file.relative_path.clone(),
            language_id: file.language_id.clone(),
            mtime_ns,
            size,
            source_hash,
        })?;
        db.insert_units(file_id, &units)?;
        stats.indexed += 1;
        stats.units += units.len();
    }

    // Remove records for files that vanished from disk (or became excluded).
    for record in db.list_files(project_id)? {
        if !seen.contains(&record.relative_path) {
            db.delete_file(record.id)?;
            stats.removed += 1;
        }
    }
    Ok(stats)
}

fn apply_retention(units: &mut [NewCodeUnit], retention: RetentionMode) {
    for unit in units {
        match retention {
            RetentionMode::Full => {}
            RetentionMode::Report => unit.embedding_text = None,
            RetentionMode::Minimal => {
                unit.embedding_text = None;
                unit.display_source = None;
            }
        }
    }
}
