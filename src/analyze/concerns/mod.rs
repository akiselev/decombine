//! Concern-signal analysis by explicit query projection: embed named
//! concern queries with the same model used for code units, score every
//! unit by normalized dot product, and measure structural spread. Results
//! are *candidate* concerns — evidence for human review, not proof.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::analyze::context::AnalysisContext;
use crate::analyze::duplicate::cross_directory::nearest_neighbor_dispersion;
use crate::analyze::paths::{directory_entropy, directory_of, top_level_module};
use crate::config::ConcernsConfig;
use crate::embed::{EmbedRequest, Embedder, apply_output_dimensions, normalize_in_place};

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredUnit {
    /// Index into `AnalysisContext::units`.
    pub unit: usize,
    pub projection: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpreadMetrics {
    pub files: usize,
    pub directories: usize,
    pub top_level_modules: usize,
    pub directory_entropy: f64,
    /// Mean nearest-neighbor path distance among the kept units.
    pub dispersion: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConcernFinding {
    pub name: String,
    pub query: String,
    /// Top units above `min_projection`, best first.
    pub units: Vec<ScoredUnit>,
    pub spread: SpreadMetrics,
}

#[derive(Debug, Default)]
pub struct ConcernReport {
    /// Sorted by concern name for deterministic output.
    pub findings: Vec<ConcernFinding>,
}

/// Runs projection scoring with an embedder that must match the context's
/// model identity (query vectors and unit vectors must share a space).
pub struct ConcernAnalyzer<'e> {
    pub embedder: &'e mut dyn Embedder,
}

impl ConcernAnalyzer<'_> {
    /// Not an `Analyzer` impl: embedding the queries needs `&mut self`.
    pub fn run_mut(
        &mut self,
        ctx: &AnalysisContext,
        config: &ConcernsConfig,
    ) -> Result<ConcernReport> {
        let contract = self.embedder.contract();
        let space = &ctx.space().identity;
        ensure!(
            *contract == space.model,
            "concern queries must be embedded with the same model as code units \
             (context: {}, embedder: {}; differing: {})",
            space.model.model,
            contract.model,
            codeindex_query::contract_diff(&space.model, contract).join(", ")
        );

        let mut queries = config.queries.clone();
        queries.sort_by(|a, b| a.name.cmp(&b.name));
        let texts: Vec<String> = queries.iter().map(|q| q.query.clone()).collect();
        let inputs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let mut vectors = self.embedder.embed(&EmbedRequest::queries(&inputs, None))?;
        for vector in &mut vectors {
            normalize_in_place(vector);
            apply_output_dimensions(
                vector,
                space.document_side.output_dimensions,
                space.model.normalize,
            )?;
        }

        let mut findings = Vec::with_capacity(queries.len());
        for (query, query_vector) in queries.iter().zip(&vectors) {
            let mut scored: Vec<ScoredUnit> = (0..ctx.units.len())
                .filter_map(|unit| {
                    let row = ctx.vectors().row_for_unit(unit)?;
                    let projection =
                        crate::analyze::vector_store::dot(ctx.vectors().vector(row), query_vector);
                    (projection >= config.min_projection as f32)
                        .then_some(ScoredUnit { unit, projection })
                })
                .collect();
            scored.sort_by(|x, y| {
                y.projection
                    .total_cmp(&x.projection)
                    .then(x.unit.cmp(&y.unit))
            });
            scored.truncate(config.top_units_per_concern);

            findings.push(ConcernFinding {
                name: query.name.clone(),
                query: query.query.clone(),
                spread: spread_metrics(ctx, &scored),
                units: scored,
            });
        }
        Ok(ConcernReport { findings })
    }
}

fn spread_metrics(ctx: &AnalysisContext, scored: &[ScoredUnit]) -> SpreadMetrics {
    let units: Vec<&crate::analyze::context::CodeUnitRef> =
        scored.iter().map(|s| &ctx.units[s.unit]).collect();
    let files: BTreeSet<String> = units.iter().map(|u| u.location()).collect();
    let directories: BTreeSet<&str> = units
        .iter()
        .map(|u| directory_of(&u.relative_path))
        .collect();
    let modules: BTreeSet<&str> = units
        .iter()
        .map(|u| top_level_module(&u.relative_path))
        .collect();
    SpreadMetrics {
        files: files.len(),
        directories: directories.len(),
        top_level_modules: modules.len(),
        directory_entropy: directory_entropy(units.iter().map(|u| u.relative_path.as_str())),
        dispersion: nearest_neighbor_dispersion(&units),
    }
}
