//! The analysis context and the shared analyzer trait.
//!
//! The loaded corpus itself — projects, code units, model identity, and
//! vectors — plus the end-to-end search operations now live in
//! `codeindex-search` as [`SearchIndex`]. decombine keeps `AnalysisContext` as
//! an alias so its analyzers and reports keep their existing paths, and owns
//! the `Analyzer` trait (a decombine concept: focused analyzers over a shared
//! context), which is not part of the reusable search service.

use anyhow::Result;

pub use codeindex_search::{
    CodeUnitRef, SearchIndex as AnalysisContext, load_metadata, load_projects_and_units,
};

/// Trait for focused analyzers operating over a shared context.
pub trait Analyzer {
    type Config;
    type Output;

    fn run(&self, ctx: &AnalysisContext, config: &Self::Config) -> Result<Self::Output>;
}
