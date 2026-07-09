use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Embedding-based detector for non-exact code duplication, concern signals,
/// and project-to-project comparison.
#[derive(Debug, Parser)]
#[command(name = "decombine", version, about)]
pub struct Cli {
    /// Path to the YAML configuration file.
    #[arg(long, global = true, default_value = "decombine.yaml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write a starter configuration file.
    Init(InitArgs),
    /// Print the effective configuration after defaults are applied.
    ShowConfig,
    /// Scan source files and extract code units into the database.
    Index(IndexArgs),
    /// Embed distinct un-embedded code unit bodies with the local model.
    Embed,
    /// Report per-language token-length distributions and over-cap counts for
    /// the configured model, revealing silent truncation. Scans all indexed
    /// units (recovering text from source under report/minimal retention).
    Tokens,
    /// Run analyses over indexed and embedded code units.
    Analyze(AnalyzeArgs),
    /// Compare two indexed projects (left = reference, right = candidate).
    Compare(CompareArgs),
    /// Convenience pipeline: index, embed, then analyze.
    Run(AnalyzeArgs),
    /// Language support commands.
    Languages(LanguagesArgs),
    /// Embedding model commands.
    Models(ModelsArgs),
    /// Diagnose parser, model, and storage health.
    Doctor(DoctorArgs),
    /// Compare embeddings between two databases (e.g. a CPU baseline vs an
    /// accelerator build) and gate on drift. Exits non-zero if the gate fails.
    Drift(DriftArgs),
    /// Machine-friendly queries over the indexed database (stable unit IDs,
    /// metadata filters, vector neighbors, semantic search).
    Query(QueryArgs),
}

#[derive(Debug, Args)]
pub struct DriftArgs {
    /// Reference database (e.g. the CPU baseline).
    #[arg(long)]
    pub baseline: PathBuf,
    /// Candidate database to check against the baseline.
    #[arg(long)]
    pub candidate: PathBuf,
    /// Neighbourhood size for the top-k recall metric.
    #[arg(long, default_value_t = 10)]
    pub top_k: usize,
    /// Cap on query units scanned for recall (0 = all shared units).
    #[arg(long, default_value_t = 2000)]
    pub sample: usize,
    /// Gate: minimum per-body cosine allowed.
    #[arg(long, default_value_t = 0.9999)]
    pub min_cosine: f32,
    /// Gate: minimum mean top-k neighbour recall allowed.
    #[arg(long, default_value_t = 0.99)]
    pub min_recall: f64,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Run a live embedding smoke test with this execution provider,
    /// overriding `embedding.execution_provider` for the check. Downloads the
    /// configured model on first use and reports the provider actually used.
    #[arg(long)]
    pub provider: Option<String>,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Overwrite an existing configuration file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct IndexArgs {
    /// Only index the project with this label.
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    #[command(subcommand)]
    pub analysis: Option<AnalysisCommand>,
    /// Emit a machine-readable JSON report on stdout instead of writing the
    /// markdown report directory. Progress stays on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    /// Cap top-level result items in --json output (the summary always
    /// reports the full matched count and whether more results exist).
    #[arg(long, global = true)]
    pub limit: Option<usize>,
}

#[derive(Debug, Subcommand)]
pub enum AnalysisCommand {
    /// Detect non-exact duplicate code clusters (the default analysis).
    Duplicates,
    /// Project code units onto configured concern queries.
    Concerns,
}

#[derive(Debug, Args)]
pub struct CompareArgs {
    /// Label of the reference project (defaults to comparison.left in config).
    #[arg(long)]
    pub left: Option<String>,
    /// Label of the candidate project (defaults to comparison.right in config).
    #[arg(long)]
    pub right: Option<String>,
    /// Emit a machine-readable JSON report on stdout instead of writing the
    /// markdown report directory. Progress stays on stderr.
    #[arg(long)]
    pub json: bool,
    /// Cap match records in --json output (the summary always reports the
    /// full matched count and whether more results exist).
    #[arg(long)]
    pub limit: Option<usize>,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    #[command(subcommand)]
    pub command: QueryCommand,
}

#[derive(Debug, Subcommand)]
pub enum QueryCommand {
    /// Report what this config/database can answer (projects, languages,
    /// model, embedding freshness, available analyzers).
    Capabilities(CapabilitiesArgs),
    /// Resolve a `unit:<id>` selector to its metadata (and optionally source).
    Inspect(InspectArgs),
    /// List indexed code units, filtered by metadata.
    Units(UnitsArgs),
    /// Vector neighbors of an indexed unit, best first.
    Similar(SimilarArgs),
    /// Semantic search over indexed units from a natural-language query
    /// (embeds the query with the configured model).
    Search(SearchArgs),
    /// Query by example: nearest neighbors of a known unit (same engine as
    /// `similar`, the workflow name agents know it by).
    Qbe(SimilarArgs),
}

#[derive(Debug, Args)]
pub struct CapabilitiesArgs {
    /// Emit JSON on stdout instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct InspectArgs {
    /// Unit selector, e.g. `unit:0123456789abcdef`.
    pub selector: String,
    /// Include the unit's source text (stored display source, or recovered
    /// from the project source tree by byte range).
    #[arg(long)]
    pub source: bool,
    /// Emit JSON on stdout instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UnitsArgs {
    /// Metadata filter, e.g. 'language=rust kind=function path=src/**'.
    /// Keys AND together, repeating a key ORs its values; `path`, `name`,
    /// and `scope` accept globs; `min_nodes=N` gates body size.
    #[arg(long)]
    pub r#where: Option<String>,
    /// Maximum units to return (default: all).
    #[arg(long)]
    pub limit: Option<usize>,
    /// Emit JSON on stdout instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SimilarArgs {
    /// Query unit selector, e.g. `unit:0123456789abcdef`.
    #[arg(long)]
    pub unit: String,
    /// Maximum neighbors to return.
    #[arg(long, default_value_t = 20, visible_alias = "neighbors")]
    pub limit: usize,
    /// Only return neighbors with cosine >= this raw score. Raw cosines do
    /// not port across models; omit to rank without a cutoff.
    #[arg(long)]
    pub threshold: Option<f32>,
    /// Metadata filter applied to candidate neighbors (see `query units`).
    #[arg(long)]
    pub r#where: Option<String>,
    /// Attach inline score/decision explanations to each result.
    #[arg(long)]
    pub why: bool,
    /// Emit JSON on stdout instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Natural-language query text.
    #[arg(long)]
    pub text: String,
    /// Maximum results to return.
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Metadata filter applied to candidate units (see `query units`).
    #[arg(long)]
    pub r#where: Option<String>,
    /// Attach inline score/evidence explanations to each result.
    #[arg(long)]
    pub why: bool,
    /// Emit JSON on stdout instead of human-readable text.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LanguagesArgs {
    #[command(subcommand)]
    pub command: LanguagesCommand,
}

#[derive(Debug, Subcommand)]
pub enum LanguagesCommand {
    /// List bundled languages and whether they are enabled.
    List,
}

#[derive(Debug, Args)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub command: ModelsCommand,
}

#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// List supported embedding models.
    List,
    /// Download the configured model into the local cache.
    Download,
}
