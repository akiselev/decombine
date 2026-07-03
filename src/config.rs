use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Language ids bundled with this binary. Kept in sync with the language
/// registry assets (Phase 3).
pub const KNOWN_LANGUAGE_IDS: &[&str] = &[
    "c",
    "cpp",
    "csharp",
    "go",
    "java",
    "javascript",
    "kotlin",
    "php",
    "python",
    "ruby",
    "rust",
    "typescript",
];

/// Embedding models supported by the fastembed backend, with their output
/// dimensions and whether a quantized variant exists.
pub const SUPPORTED_MODELS: &[(&str, usize, bool)] = &[
    ("BGESmallENV15", 384, true),
    ("BGEBaseENV15", 768, true),
    ("JinaEmbeddingsV2BaseCode", 768, false),
];

pub const EXECUTION_PROVIDERS: &[&str] = &["cpu", "cuda", "coreml", "directml", "openvino"];

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot parse config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    #[error("config must set either `source_dir` or `projects`")]
    NoSource,
    #[error("config sets both `source_dir` and `projects`; use one or the other")]
    AmbiguousSource,
    #[error("`projects` must contain at least one project")]
    EmptyProjects,
    #[error("project label {0:?} is empty or not a simple identifier")]
    BadProjectLabel(String),
    #[error("duplicate project label {0:?}")]
    DuplicateProjectLabel(String),
    #[error("source directory {0} does not exist or is not a directory")]
    MissingSourceDir(PathBuf),
    #[error("`languages.enabled` must not be empty")]
    NoLanguages,
    #[error("unknown language id {0:?}")]
    UnknownLanguage(String),
    #[error("duplicate language id {0:?}")]
    DuplicateLanguage(String),
    #[error("unsupported embedding backend {0:?} (supported: fastembed)")]
    UnknownBackend(String),
    #[error("unknown embedding model {0:?} (supported: {1})")]
    UnknownModel(String, String),
    #[error("model {0:?} has no quantized variant; set `embedding.quantized: false`")]
    NoQuantizedVariant(String),
    #[error("unknown execution provider {0:?} (supported: {1})")]
    UnknownExecutionProvider(String, String),
    #[error("`embedding.batch_size` must be between 1 and 100000, got {0}")]
    BadBatchSize(usize),
    #[error("`embedding.max_batch_chars` must be at least 1000, got {0}")]
    BadMaxBatchChars(usize),
    #[error("`embedding.max_body_chars` must be at least 100, got {0}")]
    BadMaxBodyChars(usize),
    #[error("`embedding.pending_page_size` must be between 1 and 100000, got {0}")]
    BadPendingPageSize(usize),
    #[error("threshold `{name}` must be in ({min}, {max}], got {value}")]
    ThresholdRange {
        name: &'static str,
        min: f64,
        max: f64,
        value: f64,
    },
    #[error(
        "analysis thresholds must be ordered candidate <= similarity <= rerank, \
         got candidate={candidate} similarity={similarity} rerank={rerank}"
    )]
    ThresholdOrder {
        candidate: f64,
        similarity: f64,
        rerank: f64,
    },
    #[error("`analysis.block_size` must be between 1 and 1000000, got {0}")]
    BadBlockSize(usize),
    #[error(
        "`analysis.min_semantic_body_node_count` must be at least `body_node_count_threshold`, got {semantic} < {indexed}"
    )]
    BadSemanticBodyNodeCount { semantic: usize, indexed: usize },
    #[error("`analysis.max_edges_per_unit` must be between 1 and 1000, got {0}")]
    BadMaxEdgesPerUnit(usize),
    #[error("`analysis.max_cluster_size` must be between 2 and 100000, got {0}")]
    BadMaxClusterSize(usize),
    #[error("concern query name {0:?} is empty or duplicated")]
    BadConcernName(String),
    #[error("concern query {0:?} has empty query text")]
    EmptyConcernQuery(String),
    #[error("`analysis.concerns.top_units_per_concern` must be between 1 and 1000, got {0}")]
    BadConcernTopUnits(usize),
    #[error("comparison label {0:?} does not name a configured project")]
    ComparisonUnknownLabel(String),
    #[error("comparison `left` and `right` must differ, both are {0:?}")]
    ComparisonSameLabel(String),
    #[error(
        "comparison thresholds must satisfy candidate_threshold <= match_threshold, \
         got candidate={candidate} match={match_threshold}"
    )]
    ComparisonThresholdOrder {
        candidate: f64,
        match_threshold: f64,
    },
    #[error("`comparison.top_k_per_unit` must be between 1 and 100, got {0}")]
    BadComparisonTopK(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub source_dir: Option<PathBuf>,
    #[serde(default)]
    pub source_dir_exclude: Vec<String>,
    #[serde(default = "default_db_file")]
    pub db_file: PathBuf,
    #[serde(default = "default_report_dir")]
    pub report_dir: PathBuf,
    #[serde(default = "default_ignore_file")]
    pub ignore_file: PathBuf,
    #[serde(default)]
    pub projects: Option<Vec<ProjectConfig>>,
    #[serde(default)]
    pub languages: LanguagesConfig,
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub analysis: AnalysisConfig,
    #[serde(default)]
    pub comparison: Option<ComparisonConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub label: String,
    pub source_dir: PathBuf,
    #[serde(default)]
    pub source_dir_exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LanguagesConfig {
    #[serde(default = "default_enabled_languages")]
    pub enabled: Vec<String>,
}

impl Default for LanguagesConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled_languages(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_max_batch_chars")]
    pub max_batch_chars: usize,
    #[serde(default = "default_max_body_chars")]
    pub max_body_chars: usize,
    #[serde(default = "default_pending_page_size")]
    pub pending_page_size: usize,
    #[serde(default = "default_true")]
    pub normalize: bool,
    #[serde(default = "default_execution_provider")]
    pub execution_provider: String,
    #[serde(default)]
    pub quantized: bool,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").expect("empty embedding config uses defaults")
    }
}

impl EmbeddingConfig {
    /// Output dimensions of the configured model.
    pub fn dimensions(&self) -> usize {
        SUPPORTED_MODELS
            .iter()
            .find(|(name, _, _)| *name == self.model)
            .map(|(_, dims, _)| *dims)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RetentionMode {
    /// Store display source and embedding text.
    Full,
    /// Store only the report/display source plus hashes and ranges.
    Report,
    /// Store hashes and ranges only; reports reread source files.
    Minimal,
}

impl RetentionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            RetentionMode::Full => "full",
            RetentionMode::Report => "report",
            RetentionMode::Minimal => "minimal",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexConfig {
    #[serde(default = "default_retention")]
    pub retention: RetentionMode,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            retention: default_retention(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnalysisConfig {
    #[serde(default = "default_candidate_threshold")]
    pub candidate_threshold: f64,
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: f64,
    #[serde(default = "default_rerank_threshold")]
    pub rerank_threshold: f64,
    #[serde(default = "default_block_size")]
    pub block_size: usize,
    #[serde(default = "default_body_node_count_threshold")]
    pub body_node_count_threshold: usize,
    #[serde(default = "default_min_semantic_body_node_count")]
    pub min_semantic_body_node_count: usize,
    #[serde(default = "default_max_edges_per_unit")]
    pub max_edges_per_unit: usize,
    #[serde(default = "default_max_cluster_size")]
    pub max_cluster_size: usize,
    #[serde(default)]
    pub concerns: ConcernsConfig,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").expect("empty analysis config uses defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConcernsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_min_projection")]
    pub min_projection: f64,
    #[serde(default = "default_top_units_per_concern")]
    pub top_units_per_concern: usize,
    #[serde(default)]
    pub queries: Vec<ConcernQuery>,
}

impl Default for ConcernsConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").expect("empty concerns config uses defaults")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConcernQuery {
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComparisonConfig {
    #[serde(default)]
    pub left: Option<String>,
    #[serde(default)]
    pub right: Option<String>,
    #[serde(default = "default_comparison_candidate_threshold")]
    pub candidate_threshold: f64,
    #[serde(default = "default_match_threshold")]
    pub match_threshold: f64,
    #[serde(default = "default_top_k_per_unit")]
    pub top_k_per_unit: usize,
    #[serde(default = "default_true")]
    pub use_name_hints: bool,
    #[serde(default = "default_true")]
    pub use_path_hints: bool,
}

impl Default for ComparisonConfig {
    fn default() -> Self {
        serde_yaml::from_str("{}").expect("empty comparison config uses defaults")
    }
}

fn default_db_file() -> PathBuf {
    PathBuf::from("decombine.db")
}
fn default_report_dir() -> PathBuf {
    PathBuf::from("decombine-report")
}
fn default_ignore_file() -> PathBuf {
    PathBuf::from(".decombineignore")
}
fn default_enabled_languages() -> Vec<String> {
    KNOWN_LANGUAGE_IDS.iter().map(|s| s.to_string()).collect()
}
fn default_backend() -> String {
    "fastembed".to_string()
}
fn default_model() -> String {
    "BGESmallENV15".to_string()
}
fn default_batch_size() -> usize {
    256
}
fn default_max_batch_chars() -> usize {
    200_000
}
fn default_max_body_chars() -> usize {
    10_000
}
fn default_pending_page_size() -> usize {
    512
}
fn default_execution_provider() -> String {
    "cpu".to_string()
}
fn default_true() -> bool {
    true
}
fn default_retention() -> RetentionMode {
    RetentionMode::Report
}
fn default_candidate_threshold() -> f64 {
    0.88
}
fn default_similarity_threshold() -> f64 {
    0.92
}
fn default_rerank_threshold() -> f64 {
    0.94
}
fn default_block_size() -> usize {
    1000
}
fn default_body_node_count_threshold() -> usize {
    10
}
fn default_min_semantic_body_node_count() -> usize {
    20
}
fn default_max_edges_per_unit() -> usize {
    5
}
fn default_max_cluster_size() -> usize {
    100
}
fn default_min_projection() -> f64 {
    0.45
}
fn default_top_units_per_concern() -> usize {
    50
}
fn default_comparison_candidate_threshold() -> f64 {
    0.78
}
fn default_match_threshold() -> f64 {
    0.86
}
fn default_top_k_per_unit() -> usize {
    5
}

/// A project root after single/multi-project resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProject {
    pub label: String,
    pub source_dir: PathBuf,
    pub exclude: Vec<String>,
}

impl Config {
    /// Load a config file, apply defaults, resolve paths relative to the
    /// config file's directory, and validate.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let mut config: Config =
            serde_yaml::from_str(&text).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        config.resolve_paths(base);
        config.validate()?;
        Ok(config)
    }

    /// Make all configured paths absolute relative to `base`.
    fn resolve_paths(&mut self, base: &Path) {
        let join = |p: &PathBuf| {
            if p.is_absolute() {
                p.clone()
            } else {
                base.join(p)
            }
        };
        if let Some(dir) = &self.source_dir {
            self.source_dir = Some(join(dir));
        }
        if let Some(projects) = &mut self.projects {
            for project in projects {
                project.source_dir = join(&project.source_dir);
            }
        }
        self.db_file = join(&self.db_file);
        self.report_dir = join(&self.report_dir);
        self.ignore_file = join(&self.ignore_file);
        if let Some(dir) = &self.embedding.cache_dir {
            self.embedding.cache_dir = Some(join(dir));
        }
    }

    /// The effective project list: `source_dir` becomes one implicit project
    /// labeled `main`.
    pub fn resolved_projects(&self) -> Vec<ResolvedProject> {
        match (&self.source_dir, &self.projects) {
            (Some(dir), None) => vec![ResolvedProject {
                label: "main".to_string(),
                source_dir: dir.clone(),
                exclude: self.source_dir_exclude.clone(),
            }],
            (None, Some(projects)) => projects
                .iter()
                .map(|p| ResolvedProject {
                    label: p.label.clone(),
                    source_dir: p.source_dir.clone(),
                    exclude: p.source_dir_exclude.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_sources()?;
        self.validate_languages()?;
        self.validate_embedding()?;
        self.validate_analysis()?;
        self.validate_concerns()?;
        self.validate_comparison()?;
        Ok(())
    }

    fn validate_sources(&self) -> Result<(), ConfigError> {
        match (&self.source_dir, &self.projects) {
            (None, None) => return Err(ConfigError::NoSource),
            (Some(_), Some(_)) => return Err(ConfigError::AmbiguousSource),
            _ => {}
        }
        let projects = self.resolved_projects();
        if projects.is_empty() {
            return Err(ConfigError::EmptyProjects);
        }
        let mut labels = HashSet::new();
        for project in &projects {
            let label = project.label.trim();
            if label.is_empty()
                || !label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(ConfigError::BadProjectLabel(project.label.clone()));
            }
            if !labels.insert(project.label.clone()) {
                return Err(ConfigError::DuplicateProjectLabel(project.label.clone()));
            }
            if !project.source_dir.is_dir() {
                return Err(ConfigError::MissingSourceDir(project.source_dir.clone()));
            }
        }
        Ok(())
    }

    fn validate_languages(&self) -> Result<(), ConfigError> {
        if self.languages.enabled.is_empty() {
            return Err(ConfigError::NoLanguages);
        }
        let mut seen = HashSet::new();
        for id in &self.languages.enabled {
            if !KNOWN_LANGUAGE_IDS.contains(&id.as_str()) {
                return Err(ConfigError::UnknownLanguage(id.clone()));
            }
            if !seen.insert(id.clone()) {
                return Err(ConfigError::DuplicateLanguage(id.clone()));
            }
        }
        Ok(())
    }

    fn validate_embedding(&self) -> Result<(), ConfigError> {
        let e = &self.embedding;
        if e.backend != "fastembed" {
            return Err(ConfigError::UnknownBackend(e.backend.clone()));
        }
        let Some((_, _, has_quantized)) = SUPPORTED_MODELS
            .iter()
            .find(|(name, _, _)| *name == e.model)
        else {
            let supported = SUPPORTED_MODELS
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ConfigError::UnknownModel(e.model.clone(), supported));
        };
        if e.quantized && !has_quantized {
            return Err(ConfigError::NoQuantizedVariant(e.model.clone()));
        }
        if !EXECUTION_PROVIDERS.contains(&e.execution_provider.as_str()) {
            return Err(ConfigError::UnknownExecutionProvider(
                e.execution_provider.clone(),
                EXECUTION_PROVIDERS.join(", "),
            ));
        }
        if e.batch_size == 0 || e.batch_size > 100_000 {
            return Err(ConfigError::BadBatchSize(e.batch_size));
        }
        if e.max_batch_chars < 1000 {
            return Err(ConfigError::BadMaxBatchChars(e.max_batch_chars));
        }
        if e.max_body_chars < 100 {
            return Err(ConfigError::BadMaxBodyChars(e.max_body_chars));
        }
        if e.pending_page_size == 0 || e.pending_page_size > 100_000 {
            return Err(ConfigError::BadPendingPageSize(e.pending_page_size));
        }
        Ok(())
    }

    fn validate_analysis(&self) -> Result<(), ConfigError> {
        let a = &self.analysis;
        check_threshold("analysis.candidate_threshold", a.candidate_threshold)?;
        check_threshold("analysis.similarity_threshold", a.similarity_threshold)?;
        // Rerank compares boosted scores, which may exceed raw cosine range.
        check_threshold_max("analysis.rerank_threshold", a.rerank_threshold, 1.5)?;
        if !(a.candidate_threshold <= a.similarity_threshold
            && a.similarity_threshold <= a.rerank_threshold)
        {
            return Err(ConfigError::ThresholdOrder {
                candidate: a.candidate_threshold,
                similarity: a.similarity_threshold,
                rerank: a.rerank_threshold,
            });
        }
        if a.block_size == 0 || a.block_size > 1_000_000 {
            return Err(ConfigError::BadBlockSize(a.block_size));
        }
        if a.min_semantic_body_node_count < a.body_node_count_threshold {
            return Err(ConfigError::BadSemanticBodyNodeCount {
                semantic: a.min_semantic_body_node_count,
                indexed: a.body_node_count_threshold,
            });
        }
        if a.max_edges_per_unit == 0 || a.max_edges_per_unit > 1000 {
            return Err(ConfigError::BadMaxEdgesPerUnit(a.max_edges_per_unit));
        }
        if a.max_cluster_size < 2 || a.max_cluster_size > 100_000 {
            return Err(ConfigError::BadMaxClusterSize(a.max_cluster_size));
        }
        Ok(())
    }

    fn validate_concerns(&self) -> Result<(), ConfigError> {
        let c = &self.analysis.concerns;
        check_threshold("analysis.concerns.min_projection", c.min_projection)?;
        if c.top_units_per_concern == 0 || c.top_units_per_concern > 1000 {
            return Err(ConfigError::BadConcernTopUnits(c.top_units_per_concern));
        }
        let mut names = HashSet::new();
        for query in &c.queries {
            if query.name.trim().is_empty() || !names.insert(query.name.clone()) {
                return Err(ConfigError::BadConcernName(query.name.clone()));
            }
            if query.query.trim().is_empty() {
                return Err(ConfigError::EmptyConcernQuery(query.name.clone()));
            }
        }
        Ok(())
    }

    fn validate_comparison(&self) -> Result<(), ConfigError> {
        let Some(c) = &self.comparison else {
            return Ok(());
        };
        check_threshold("comparison.candidate_threshold", c.candidate_threshold)?;
        check_threshold("comparison.match_threshold", c.match_threshold)?;
        if c.candidate_threshold > c.match_threshold {
            return Err(ConfigError::ComparisonThresholdOrder {
                candidate: c.candidate_threshold,
                match_threshold: c.match_threshold,
            });
        }
        if c.top_k_per_unit == 0 || c.top_k_per_unit > 100 {
            return Err(ConfigError::BadComparisonTopK(c.top_k_per_unit));
        }
        // Labels are optional in the file (they can come from the command
        // line), but when set they must name distinct configured projects.
        let labels: HashSet<String> = self
            .resolved_projects()
            .into_iter()
            .map(|p| p.label)
            .collect();
        for label in [&c.left, &c.right].into_iter().flatten() {
            if !labels.contains(label) {
                return Err(ConfigError::ComparisonUnknownLabel(label.clone()));
            }
        }
        if let (Some(left), Some(right)) = (&c.left, &c.right)
            && left == right
        {
            return Err(ConfigError::ComparisonSameLabel(left.clone()));
        }
        Ok(())
    }
}

fn check_threshold(name: &'static str, value: f64) -> Result<(), ConfigError> {
    check_threshold_max(name, value, 1.0)
}

fn check_threshold_max(name: &'static str, value: f64, max: f64) -> Result<(), ConfigError> {
    if value <= 0.0 || value > max || !value.is_finite() {
        return Err(ConfigError::ThresholdRange {
            name,
            min: 0.0,
            max,
            value,
        });
    }
    Ok(())
}

/// Starter configuration written by `decombine init`.
pub const CONFIG_TEMPLATE: &str = r#"# decombine configuration
source_dir: .
source_dir_exclude: []
db_file: decombine.db
report_dir: decombine-report
ignore_file: .decombineignore

# Optional multi-project form. If present, remove `source_dir` above.
# projects:
#   - label: v1
#     source_dir: ../old-project
#   - label: v2
#     source_dir: .

languages:
  enabled: ["c", "cpp", "csharp", "go", "java", "javascript", "kotlin", "php", "python", "ruby", "rust", "typescript"]

embedding:
  backend: fastembed
  model: BGESmallENV15
  # cache_dir: /absolute/path/to/decombine/models
  batch_size: 256
  max_batch_chars: 200000
  max_body_chars: 10000
  pending_page_size: 512
  normalize: true
  execution_provider: cpu
  quantized: false

index:
  retention: report # full | report | minimal

analysis:
  candidate_threshold: 0.88
  similarity_threshold: 0.92
  rerank_threshold: 0.94
  block_size: 1000
  body_node_count_threshold: 10
  min_semantic_body_node_count: 20
  max_edges_per_unit: 5
  max_cluster_size: 100
  concerns:
    enabled: false
    min_projection: 0.45
    top_units_per_concern: 50
    queries: []
    # queries:
    #   - name: error-handling
    #     query: "error handling, retries, and failure reporting"

# comparison:
#   left: v1
#   right: v2
#   candidate_threshold: 0.78
#   match_threshold: 0.86
#   top_k_per_unit: 5
#   use_name_hints: true
#   use_path_hints: true
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(dir: &Path) -> Config {
        let mut config: Config = serde_yaml::from_str("source_dir: .").unwrap();
        config.resolve_paths(dir);
        config
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn golden_template_parses_and_validates() {
        let dir = tempdir();
        let path = dir.path().join("decombine.yaml");
        std::fs::write(&path, CONFIG_TEMPLATE).unwrap();
        let config = Config::load(&path).unwrap();
        // The template must spell out the defaults exactly.
        assert_eq!(config.embedding, EmbeddingConfig::default());
        assert_eq!(config.analysis, AnalysisConfig::default());
        assert_eq!(config.index, IndexConfig::default());
        assert_eq!(config.languages, LanguagesConfig::default());
        assert_eq!(config.comparison, None);
        let projects = config.resolved_projects();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].label, "main");
    }

    #[test]
    fn missing_source_dir_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.source_dir = Some(dir.path().join("does-not-exist"));
        assert!(matches!(
            config.validate(),
            Err(ConfigError::MissingSourceDir(_))
        ));
    }

    #[test]
    fn no_source_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.source_dir = None;
        assert!(matches!(config.validate(), Err(ConfigError::NoSource)));
    }

    #[test]
    fn conflicting_source_dir_and_projects_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.projects = Some(vec![ProjectConfig {
            label: "a".into(),
            source_dir: dir.path().to_path_buf(),
            source_dir_exclude: vec![],
        }]);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::AmbiguousSource)
        ));
    }

    #[test]
    fn duplicate_project_labels_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.source_dir = None;
        let project = ProjectConfig {
            label: "same".into(),
            source_dir: dir.path().to_path_buf(),
            source_dir_exclude: vec![],
        };
        config.projects = Some(vec![project.clone(), project]);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::DuplicateProjectLabel(_))
        ));
    }

    #[test]
    fn bad_project_label_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.source_dir = None;
        config.projects = Some(vec![ProjectConfig {
            label: "has spaces".into(),
            source_dir: dir.path().to_path_buf(),
            source_dir_exclude: vec![],
        }]);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::BadProjectLabel(_))
        ));
    }

    #[test]
    fn bad_thresholds_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.analysis.candidate_threshold = 1.2;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ThresholdRange { .. })
        ));

        let mut config = base_config(dir.path());
        config.analysis.candidate_threshold = 0.95;
        config.analysis.similarity_threshold = 0.90;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ThresholdOrder { .. })
        ));
    }

    #[test]
    fn unknown_language_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.languages.enabled = vec!["cobol".into()];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownLanguage(_))
        ));
    }

    #[test]
    fn bad_retention_mode_rejected() {
        let err = serde_yaml::from_str::<Config>("source_dir: .\nindex:\n  retention: archive\n")
            .unwrap_err();
        assert!(err.to_string().contains("retention"));
    }

    #[test]
    fn bad_comparison_labels_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.comparison = Some(ComparisonConfig {
            left: Some("nope".into()),
            right: Some("main".into()),
            ..ComparisonConfig::default()
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ComparisonUnknownLabel(_))
        ));

        let mut config = base_config(dir.path());
        config.comparison = Some(ComparisonConfig {
            left: Some("main".into()),
            right: Some("main".into()),
            ..ComparisonConfig::default()
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ComparisonSameLabel(_))
        ));
    }

    #[test]
    fn comparison_threshold_order_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.comparison = Some(ComparisonConfig {
            candidate_threshold: 0.9,
            match_threshold: 0.8,
            ..ComparisonConfig::default()
        });
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ComparisonThresholdOrder { .. })
        ));
    }

    #[test]
    fn incompatible_embedding_config_rejected() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.embedding.backend = "openai".into();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownBackend(_))
        ));

        let mut config = base_config(dir.path());
        config.embedding.model = "NotAModel".into();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownModel(_, _))
        ));

        let mut config = base_config(dir.path());
        config.embedding.model = "JinaEmbeddingsV2BaseCode".into();
        config.embedding.quantized = true;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::NoQuantizedVariant(_))
        ));

        let mut config = base_config(dir.path());
        config.embedding.execution_provider = "tpu".into();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::UnknownExecutionProvider(_, _))
        ));

        let mut config = base_config(dir.path());
        config.embedding.batch_size = 0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::BadBatchSize(0))
        ));
    }

    #[test]
    fn concern_validation() {
        let dir = tempdir();
        let mut config = base_config(dir.path());
        config.analysis.concerns.queries = vec![ConcernQuery {
            name: "auth".into(),
            query: "  ".into(),
        }];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::EmptyConcernQuery(_))
        ));

        let mut config = base_config(dir.path());
        config.analysis.concerns.queries = vec![
            ConcernQuery {
                name: "auth".into(),
                query: "authentication".into(),
            },
            ConcernQuery {
                name: "auth".into(),
                query: "authorization".into(),
            },
        ];
        assert!(matches!(
            config.validate(),
            Err(ConfigError::BadConcernName(_))
        ));

        let mut config = base_config(dir.path());
        config.analysis.concerns.min_projection = 0.0;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::ThresholdRange { .. })
        ));
    }

    #[test]
    fn two_projects_share_relative_paths() {
        let dir = tempdir();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut config = base_config(dir.path());
        config.source_dir = None;
        config.projects = Some(vec![
            ProjectConfig {
                label: "v1".into(),
                source_dir: a,
                source_dir_exclude: vec![],
            },
            ProjectConfig {
                label: "v2".into(),
                source_dir: b,
                source_dir_exclude: vec![],
            },
        ]);
        config.validate().unwrap();
        assert_eq!(config.resolved_projects().len(), 2);
    }
}
