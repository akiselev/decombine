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
