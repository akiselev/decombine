//! The analysis context and the shared analyzer trait.
//!
//! The loaded corpus — projects, code units, embedding spaces, and vectors —
//! plus the end-to-end search operations live in `codeindex-search` as
//! [`SearchIndex`]. codeindex stores any number of independently modelled
//! embedding spaces; decombine analyzes exactly one of them
//! ([`DECOMBINE_SPACE`], the implementation channel), so `AnalysisContext`
//! wraps a `SearchIndex` and pins that space. It derefs to the index, so
//! `ctx.units` / `ctx.projects` keep working, and owns the `Analyzer` trait (a
//! decombine concept: focused analyzers over a shared context).

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

use anyhow::{Result, bail};
use codeindex_core::{EmbeddingSpaceId, EmbeddingSpaceIdentity, RepresentationKind};
use codeindex_search::storage::ProjectRecord;
use codeindex_search::{SearchIndex, SearchSpace};

pub use codeindex_search::CodeUnitRef;

use crate::analyze::vector_store::VectorStore;
use crate::db::{Db, ModelId, ModelIdentity};

/// The one embedding space decombine embeds and analyzes. It matches the id
/// codeindex's default `embed_pending` gives the implementation channel, so
/// databases interoperate with the codeindex CLI.
pub const DECOMBINE_SPACE: &str = "default/implementation";

pub fn decombine_space_id() -> EmbeddingSpaceId {
    EmbeddingSpaceId::new(DECOMBINE_SPACE)
}

/// The representation channel decombine embeds: declaration plus body,
/// documentation stripped, hashed by the normalized body.
pub fn decombine_channel() -> RepresentationKind {
    RepresentationKind::Implementation
}

pub struct AnalysisContext {
    pub index: SearchIndex,
    pub space_id: EmbeddingSpaceId,
    pub model_id: ModelId,
    pub identity: ModelIdentity,
}

impl Deref for AnalysisContext {
    type Target = SearchIndex;

    fn deref(&self) -> &SearchIndex {
        &self.index
    }
}

impl DerefMut for AnalysisContext {
    fn deref_mut(&mut self) -> &mut SearchIndex {
        &mut self.index
    }
}

impl AnalysisContext {
    /// Load the selected projects (empty = all) with decombine's embedding
    /// space and its vectors.
    pub fn load(db: &Db, project_labels: &[String]) -> Result<AnalysisContext> {
        let space_id = decombine_space_id();
        let Some(space) = db.get_space(&space_id)? else {
            bail!("no embeddings found; embed the corpus first");
        };
        let snapshot = db.snapshot_with_spaces(project_labels, std::slice::from_ref(&space_id))?;
        if snapshot.projects.is_empty() {
            bail!("no indexed projects; index a project first");
        }
        let index = SearchIndex::from_snapshot(snapshot)?;
        let execution = db.model_executions(space.model_id)?.pop();
        Ok(AnalysisContext {
            identity: ModelIdentity::from_parts(&space.identity, execution.as_ref()),
            model_id: space.model_id,
            space_id,
            index,
        })
    }

    /// Build a context from in-memory parts (tests, external snapshots): one
    /// space holding `vectors`, aligned to `units`.
    pub fn from_parts(
        identity: ModelIdentity,
        projects: Vec<ProjectRecord>,
        units: Vec<CodeUnitRef>,
        vectors: VectorStore,
    ) -> AnalysisContext {
        let space_id = decombine_space_id();
        let space =
            EmbeddingSpaceIdentity::new(space_id.clone(), decombine_channel(), identity.contract());
        let mut spaces = BTreeMap::new();
        spaces.insert(
            space_id.clone(),
            SearchSpace {
                identity: space,
                vectors,
            },
        );
        AnalysisContext {
            index: SearchIndex {
                projects,
                units,
                spaces,
                relations: Vec::new(),
            },
            space_id,
            model_id: 1,
            identity,
        }
    }

    pub fn space(&self) -> &SearchSpace {
        &self.index.spaces[&self.space_id]
    }

    pub fn vectors(&self) -> &VectorStore {
        &self.space().vectors
    }
}

/// Load selected projects (empty = all) and their code units without any
/// embeddings. Used by metadata-only consumers (unit listing, inspection).
pub fn load_projects_and_units(
    db: &Db,
    project_labels: &[String],
) -> Result<(Vec<ProjectRecord>, Vec<CodeUnitRef>)> {
    let mut snapshot = match db.get_space(&decombine_space_id())? {
        Some(_) => db.snapshot_with_spaces(project_labels, &[decombine_space_id()])?,
        None => db.snapshot(project_labels)?,
    };
    if snapshot.projects.is_empty() {
        bail!("no indexed projects; index a project first");
    }
    snapshot.spaces.clear();
    let index = SearchIndex::from_snapshot(snapshot)?;
    Ok((index.projects, index.units))
}

/// Trait for focused analyzers operating over a shared context.
pub trait Analyzer {
    type Config;
    type Output;

    fn run(&self, ctx: &AnalysisContext, config: &Self::Config) -> Result<Self::Output>;
}
