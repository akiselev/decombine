//! Loading a reusable `AnalysisContext` from the database: selected
//! projects, their code units, model identity, and normalized vectors.

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use rusqlite::params_from_iter;

use crate::analyze::vector_store::VectorStore;
use crate::db::{Db, ModelIdentity, Project, UnitId, blob_to_vector};

/// A code unit joined with its file and project, as used by analyzers.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeUnitRef {
    pub id: UnitId,
    pub project_label: String,
    pub relative_path: String,
    pub language_id: String,
    pub kind: String,
    pub name: String,
    pub scope: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub body_node_count: usize,
    pub normalized_body_hash: String,
    pub display_source: Option<String>,
}

impl CodeUnitRef {
    /// `label:path` display location.
    pub fn location(&self) -> String {
        format!("{}:{}", self.project_label, self.relative_path)
    }
}

pub struct AnalysisContext {
    pub model_id: i64,
    pub identity: ModelIdentity,
    pub projects: Vec<Project>,
    /// Sorted by (project label, path, start byte) for determinism.
    pub units: Vec<CodeUnitRef>,
    pub vectors: VectorStore,
}

/// Trait for focused analyzers operating over a shared context.
pub trait Analyzer {
    type Config;
    type Output;

    fn run(&self, ctx: &AnalysisContext, config: &Self::Config) -> Result<Self::Output>;
}

/// Load selected projects (empty = all) and their code units without
/// requiring embeddings. Used by metadata-only query commands; the full
/// `AnalysisContext::load` builds on it.
pub fn load_projects_and_units(
    db: &Db,
    project_labels: &[String],
) -> Result<(Vec<Project>, Vec<CodeUnitRef>)> {
    let all_projects = db.list_projects()?;
    let projects: Vec<Project> = if project_labels.is_empty() {
        all_projects
    } else {
        let mut selected = Vec::new();
        for label in project_labels {
            let project = all_projects
                .iter()
                .find(|p| &p.label == label)
                .with_context(|| format!("project {label:?} is not indexed"))?;
            selected.push(project.clone());
        }
        selected
    };
    if projects.is_empty() {
        bail!("no indexed projects; run `decombine index` first");
    }

    // Load units for the selected projects.
    let placeholders = projects.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT u.id, p.label, f.relative_path, u.language_id, u.kind, u.name, u.scope,
                    u.start_byte, u.end_byte, u.start_line, u.end_line,
                    u.body_node_count, u.normalized_body_hash, u.display_source
             FROM code_units u
             JOIN files f ON f.id = u.file_id
             JOIN projects p ON p.id = f.project_id
             WHERE p.id IN ({placeholders})
             ORDER BY p.label, f.relative_path, u.start_byte, u.end_byte"
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let units: Vec<CodeUnitRef> = stmt
        .query_map(params_from_iter(projects.iter().map(|p| p.id)), |row| {
            Ok(CodeUnitRef {
                id: row.get(0)?,
                project_label: row.get(1)?,
                relative_path: row.get(2)?,
                language_id: row.get(3)?,
                kind: row.get(4)?,
                name: row.get(5)?,
                scope: row.get(6)?,
                start_byte: row.get::<_, i64>(7)? as usize,
                end_byte: row.get::<_, i64>(8)? as usize,
                start_line: row.get::<_, i64>(9)? as usize,
                end_line: row.get::<_, i64>(10)? as usize,
                body_node_count: row.get::<_, i64>(11)? as usize,
                normalized_body_hash: row.get(12)?,
                display_source: row.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok((projects, units))
}

impl AnalysisContext {
    /// Load the context for the given project labels (empty = all).
    pub fn load(db: &Db, project_labels: &[String]) -> Result<AnalysisContext> {
        let (projects, units) = load_projects_and_units(db, project_labels)?;

        // The analysis model: exactly one embedding model may exist per
        // database (enforced by the embed pipeline's immutable settings).
        let models = db.list_models()?;
        let model = match models.as_slice() {
            [] => bail!("no embeddings found; run `decombine embed` first"),
            [model] => model.clone(),
            _ => bail!("database contains multiple embedding models; this is unsupported"),
        };

        // Load this model's embeddings once, then assign per unit by hash.
        let mut by_hash: HashMap<String, Vec<f32>> = HashMap::new();
        let mut stmt = db.conn().prepare(
            "SELECT normalized_body_hash, vector_blob FROM embeddings WHERE model_id = ?1",
        )?;
        let rows = stmt.query_map([model.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (hash, blob) = row?;
            by_hash.insert(hash, blob_to_vector(&blob));
        }

        let vectors = units
            .iter()
            .map(|unit| by_hash.get(&unit.normalized_body_hash).cloned())
            .collect();
        let vectors = VectorStore::from_unit_vectors(model.identity.dimensions, vectors);

        Ok(AnalysisContext {
            model_id: model.id,
            identity: model.identity,
            projects,
            units,
            vectors,
        })
    }

    /// Unit indices belonging to one project label.
    pub fn unit_indices_for_project(&self, label: &str) -> Vec<usize> {
        self.units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.project_label == label)
            .map(|(index, _)| index)
            .collect()
    }
}
