from __future__ import annotations

from pathlib import Path
import shutil
import textwrap

ROOT = Path(__file__).resolve().parents[1]


def write(path: str | Path, content: str) -> None:
    target = ROOT / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")


def reset_dir(path: str | Path) -> Path:
    target = ROOT / path
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    return target


# ---------------------------------------------------------------------------
# codeindex-sqlite: physically move the existing persistence implementation.
# ---------------------------------------------------------------------------
sqlite_src = reset_dir("crates/codeindex-sqlite/src")
shutil.copy2(ROOT / "src/db/mod.rs", sqlite_src / "lib.rs")
shutil.copy2(ROOT / "src/db/models.rs", sqlite_src / "models.rs")
shutil.copy2(ROOT / "src/db/migrations.rs", sqlite_src / "migrations.rs")
write(
    "crates/codeindex-sqlite/Cargo.toml",
    r'''
    [package]
    name = "codeindex-sqlite"
    version = "0.1.0"
    edition = "2024"
    description = "SQLite persistence and migrations for code indexes"
    publish = false

    [dependencies]
    anyhow = "1.0.103"
    rusqlite = { version = "0.40.1", features = ["bundled"] }
    serde = { version = "1.0.228", features = ["derive"] }
    ''',
)


# ---------------------------------------------------------------------------
# codeindex-indexer: scanning, incremental indexing, retention, and adapters.
# ---------------------------------------------------------------------------
reset_dir("crates/codeindex-indexer/src")
scanner = (ROOT / "src/index/scanner.rs").read_text(encoding="utf-8")
scanner = scanner.replace(
    "use crate::index::language::LanguageRegistry;",
    "use codeindex_tree_sitter::LanguageRegistry;",
)
scanner = scanner.replace(
    '''fn all_languages() -> HashSet<String> {
        crate::config::KNOWN_LANGUAGE_IDS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }''',
    '''fn all_languages() -> HashSet<String> {
        LanguageRegistry::global()
            .ids()
            .map(str::to_string)
            .collect()
    }''',
)
write("crates/codeindex-indexer/src/scanner.rs", scanner)
write(
    "crates/codeindex-indexer/Cargo.toml",
    r'''
    [package]
    name = "codeindex-indexer"
    version = "0.1.0"
    edition = "2024"
    description = "Incremental filesystem indexing orchestration for codeindex"
    publish = false

    [dependencies]
    anyhow = "1.0.103"
    codeindex-core = { path = "../codeindex-core" }
    codeindex-sqlite = { path = "../codeindex-sqlite" }
    codeindex-tree-sitter = { path = "../codeindex-tree-sitter" }
    ignore = "0.4.27"

    [dev-dependencies]
    tempfile = "3.27.0"
    ''',
)
write(
    "crates/codeindex-indexer/src/lib.rs",
    r'''
    #![forbid(unsafe_code)]

    mod scanner;

    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

    use anyhow::{Context, Result};
    use codeindex_core::{ExtractedEntity, RepresentationKind};
    use codeindex_sqlite::{Db, NewCodeUnit, NewFile};
    use codeindex_tree_sitter::{ExtractOptions, LanguageRegistry, extract_units};

    pub use scanner::{ScannedFile, scan_files};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RetentionMode {
        Full,
        Report,
        Minimal,
    }

    impl RetentionMode {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Full => "full",
                Self::Report => "report",
                Self::Minimal => "minimal",
            }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ProjectSpec {
        pub label: String,
        pub source_dir: PathBuf,
        pub exclude: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct IndexOptions {
        pub projects: Vec<ProjectSpec>,
        pub enabled_languages: Vec<String>,
        pub body_node_count_threshold: usize,
        pub max_body_chars: usize,
        pub retention: RetentionMode,
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    pub struct ProjectStats {
        pub label: String,
        pub indexed: usize,
        pub skipped: usize,
        pub removed: usize,
        pub failed: usize,
        pub units: usize,
        pub total_units: usize,
    }

    pub fn index(
        db: &Db,
        options: &IndexOptions,
        only_label: Option<&str>,
    ) -> Result<Vec<ProjectStats>> {
        db.check_or_set_immutable(
            "index.body_node_count_threshold",
            &options.body_node_count_threshold.to_string(),
        )?;
        db.check_or_set_immutable("index.retention", options.retention.as_str())?;
        db.check_or_set_immutable(
            "embedding.max_body_chars",
            &options.max_body_chars.to_string(),
        )?;

        let mut stats = Vec::new();
        for project in &options.projects {
            if only_label.is_some_and(|label| project.label != label) {
                continue;
            }
            stats.push(index_project(db, options, project)?);
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
        options: &IndexOptions,
        project: &ProjectSpec,
    ) -> Result<ProjectStats> {
        let root = &project.source_dir;
        let project_id = db.upsert_project(&project.label, &root.to_string_lossy())?;
        let enabled: HashSet<String> = options.enabled_languages.iter().cloned().collect();
        let scanned = scan_files(root, &project.exclude, &enabled)?;
        let extraction = ExtractOptions {
            body_node_count_threshold: options.body_node_count_threshold,
            max_body_chars: options.max_body_chars,
        };
        let registry = LanguageRegistry::global();

        let mut stats = ProjectStats {
            label: project.label.clone(),
            ..ProjectStats::default()
        };
        let mut seen = HashSet::with_capacity(scanned.len());
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
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos() as i64)
                .unwrap_or(0);
            let size = metadata.len() as i64;

            if existing
                .as_ref()
                .is_some_and(|record| record.mtime_ns == mtime_ns && record.size == size)
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
            let source_hash = codeindex_tree_sitter::normalizer::sha256_hex(&source);
            if let Some(record) = &existing
                && record.source_hash == source_hash
            {
                db.update_file_meta(record.id, mtime_ns, size)?;
                stats.skipped += 1;
                continue;
            }

            let def = registry
                .get(&file.language_id)
                .context("scanner produced an unregistered language")?;
            let entities = match extract_units(def, &source, &extraction) {
                Ok(entities) => entities,
                Err(error) => {
                    eprintln!("failed to parse {}: {error}", file.absolute_path.display());
                    stats.failed += 1;
                    continue;
                }
            };
            let mut units: Vec<NewCodeUnit> = entities.into_iter().map(to_new_unit).collect();
            apply_retention(&mut units, options.retention);

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

        for record in db.list_files(project_id)? {
            if !seen.contains(&record.relative_path) {
                db.delete_file(record.id)?;
                stats.removed += 1;
            }
        }
        stats.total_units = db.count_units_for_project(project_id)? as usize;
        Ok(stats)
    }

    fn to_new_unit(entity: ExtractedEntity) -> NewCodeUnit {
        let display_source = entity
            .representation_text(&RepresentationKind::FullSource)
            .map(str::to_owned);
        let embedding_text = entity
            .representation_text(&RepresentationKind::Implementation)
            .map(str::to_owned);
        NewCodeUnit {
            language_id: entity.language.into_inner(),
            kind: entity.kind.as_str().to_owned(),
            name: entity.name,
            scope: entity.scope,
            start_byte: entity.span.start_byte,
            end_byte: entity.span.end_byte,
            start_line: entity.span.start_line,
            end_line: entity.span.end_line,
            body_node_count: entity.body_node_count,
            source_hash: entity.source_hash,
            normalized_body_hash: entity.normalized_body_hash,
            display_source,
            embedding_text,
        }
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn indexes_into_existing_sqlite_schema() {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(
                root.path().join("lib.rs"),
                "fn add(a: i32, b: i32) -> i32 { let value = a + b; value }",
            )
            .unwrap();
            let db = codeindex_sqlite::open_in_memory().unwrap();
            let options = IndexOptions {
                projects: vec![ProjectSpec {
                    label: "main".into(),
                    source_dir: root.path().to_path_buf(),
                    exclude: Vec::new(),
                }],
                enabled_languages: vec!["rust".into()],
                body_node_count_threshold: 1,
                max_body_chars: 10_000,
                retention: RetentionMode::Full,
            };
            let stats = index(&db, &options, None).unwrap();
            assert_eq!(stats[0].indexed, 1);
            assert_eq!(stats[0].total_units, 1);
        }
    }
    ''',
)


# ---------------------------------------------------------------------------
# codeindex-embedding: model execution and resumable embedding projections.
# Existing mature implementation is moved verbatim and compiled behind a
# reusable, application-neutral configuration adapter.
# ---------------------------------------------------------------------------
embedding_src = reset_dir("crates/codeindex-embedding/src")
shutil.copytree(ROOT / "src/embed", embedding_src / "embed")
write(
    "crates/codeindex-embedding/Cargo.toml",
    r'''
    [package]
    name = "codeindex-embedding"
    version = "0.1.0"
    edition = "2024"
    description = "Embedding models and resumable projections for code indexes"
    publish = false

    [dependencies]
    anyhow = "1.0.103"
    codeindex-core = { path = "../codeindex-core" }
    codeindex-sqlite = { path = "../codeindex-sqlite" }
    codeindex-tree-sitter = { path = "../codeindex-tree-sitter" }
    fastembed = { version = "5.17.2", optional = true }
    hex = "0.4.3"
    ort = { version = "=2.0.0-rc.12", optional = true, default-features = false }
    sha2 = "0.11.0"
    ureq = { version = "3.3.0", optional = true }

    [features]
    default = []
    fastembed = ["dep:fastembed", "dep:ureq"]
    accel = ["fastembed", "dep:ort"]
    cuda = ["accel", "ort/cuda"]
    directml = ["accel", "ort/directml", "fastembed/directml"]
    coreml = ["accel", "ort/coreml"]
    openvino = ["accel", "ort/openvino"]
    load-dynamic = ["accel", "ort/load-dynamic", "fastembed/ort-load-dynamic"]

    [dev-dependencies]
    tempfile = "3.27.0"
    ''',
)
write(
    "crates/codeindex-embedding/build.rs",
    r'''
    use std::path::Path;

    fn main() {
        println!("cargo:rerun-if-changed=../../Cargo.lock");
        let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let text = std::fs::read_to_string(lock).unwrap_or_default();
        for (package, var) in [
            ("fastembed", "DECOMBINE_FASTEMBED_VERSION"),
            ("ort", "DECOMBINE_ORT_VERSION"),
        ] {
            let version = locked_version(&text, package).unwrap_or_else(|| "unknown".to_string());
            println!("cargo:rustc-env={var}={version}");
        }
    }

    fn locked_version(lock: &str, package: &str) -> Option<String> {
        let mut in_package = false;
        for line in lock.lines() {
            if line == "[[package]]" {
                in_package = false;
            } else if line == format!("name = \"{package}\"") {
                in_package = true;
            } else if in_package && let Some(version) = line.strip_prefix("version = \"") {
                return Some(version.trim_end_matches('"').to_string());
            }
        }
        None
    }
    ''',
)
write(
    "crates/codeindex-embedding/src/config.rs",
    r'''
    use std::path::PathBuf;

    pub const SUPPORTED_MODELS: &[(&str, usize, bool)] = &[
        ("BGESmallENV15", 384, true),
        ("BGEBaseENV15", 768, true),
        ("JinaEmbeddingsV2BaseCode", 768, false),
        ("AllMiniLML6V2", 384, true),
        ("GTEBaseENV15", 768, true),
        ("SnowflakeArcticEmbedM", 768, true),
        ("SnowflakeArcticEmbedMLong", 768, true),
        ("NomicEmbedTextV15", 768, true),
    ];

    #[derive(Debug, Clone, Copy)]
    pub struct ManagedModel {
        pub name: &'static str,
        pub cache_id: &'static str,
        pub repo: &'static str,
        pub revision: &'static str,
        pub onnx_file: &'static str,
        pub dimensions: usize,
        pub pooling: &'static str,
        pub max_length: usize,
        pub files: &'static [ManagedFile],
    }

    #[derive(Debug, Clone, Copy)]
    pub struct ManagedFile {
        pub path: &'static str,
        pub sha256: &'static str,
        pub size: u64,
    }

    pub const MANAGED_MODELS: &[ManagedModel] = &[ManagedModel {
        name: "CodeRankEmbed",
        cache_id: "coderankembed",
        repo: "Zenabius/CodeRankEmbed-onnx",
        revision: "main",
        onnx_file: "onnx/model.onnx",
        dimensions: 768,
        pooling: "mean",
        max_length: 2048,
        files: &[
            ManagedFile {
                path: "onnx/model.onnx",
                sha256: "87edaf9f6d544e9d46ed81e1e13610ac01b1c1904e3b26fcf1ce6744a0319ffa",
                size: 548_260_181,
            },
            ManagedFile {
                path: "tokenizer.json",
                sha256: "91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854",
                size: 711_649,
            },
            ManagedFile {
                path: "config.json",
                sha256: "5ff856a41d0f53ef2d74520627d464bd75c2efd8f26f381bd528654895c29b6c",
                size: 1_525,
            },
            ManagedFile {
                path: "special_tokens_map.json",
                sha256: "5d5b662e421ea9fac075174bb0688ee0d9431699900b90662acd44b2a350503a",
                size: 695,
            },
            ManagedFile {
                path: "tokenizer_config.json",
                sha256: "7809f768ee3614618b3f1b91dcbfab4f6a9d4b79fb1ad5d17feb65a7c1bb5b7a",
                size: 1_417,
            },
        ],
    }];

    pub fn managed_model(name: &str) -> Option<&'static ManagedModel> {
        MANAGED_MODELS.iter().find(|model| model.name == name)
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ProviderMode {
        Require,
        Auto,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CustomModelConfig {
        pub dir: PathBuf,
        pub onnx_file: PathBuf,
        pub dimensions: usize,
        pub pooling: String,
        pub max_length: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct EmbeddingConfig {
        pub backend: String,
        pub model: String,
        pub cache_dir: Option<PathBuf>,
        pub batch_size: usize,
        pub max_batch_chars: usize,
        pub max_batch_token_area: usize,
        pub max_body_chars: usize,
        pub pending_page_size: usize,
        pub normalize: bool,
        pub execution_provider: String,
        pub provider_mode: ProviderMode,
        pub quantized: bool,
        pub custom: Option<CustomModelConfig>,
    }

    impl Default for EmbeddingConfig {
        fn default() -> Self {
            Self {
                backend: "fastembed".into(),
                model: "CodeRankEmbed".into(),
                cache_dir: None,
                batch_size: 256,
                max_batch_chars: 200_000,
                max_batch_token_area: 16_000_000,
                max_body_chars: 10_000,
                pending_page_size: 512,
                normalize: true,
                execution_provider: "cpu".into(),
                provider_mode: ProviderMode::Require,
                quantized: false,
                custom: None,
            }
        }
    }

    impl EmbeddingConfig {
        pub fn dimensions(&self) -> usize {
            if let Some(custom) = &self.custom {
                return custom.dimensions;
            }
            if let Some(managed) = managed_model(&self.model) {
                return managed.dimensions;
            }
            SUPPORTED_MODELS
                .iter()
                .find(|(name, _, _)| *name == self.model)
                .map(|(_, dimensions, _)| *dimensions)
                .unwrap_or(0)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AnalysisConfig {
        pub body_node_count_threshold: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Config {
        pub embedding: EmbeddingConfig,
        pub analysis: AnalysisConfig,
    }
    ''',
)
write(
    "crates/codeindex-embedding/src/lib.rs",
    r'''
    #![forbid(unsafe_code)]

    pub mod config;

    pub mod db {
        pub use codeindex_sqlite::*;
    }

    pub mod index {
        pub mod language {
            pub use codeindex_tree_sitter::{LanguageDef, LanguageRegistry};
        }

        pub mod extractor {
            use anyhow::Result;
            use codeindex_core::{ExtractedEntity, RepresentationKind};
            use codeindex_sqlite::NewCodeUnit;
            use codeindex_tree_sitter::LanguageDef;

            pub use codeindex_tree_sitter::ExtractOptions;

            pub fn extract_units(
                def: &LanguageDef,
                source: &str,
                options: &ExtractOptions,
            ) -> Result<Vec<NewCodeUnit>> {
                codeindex_tree_sitter::extract_units(def, source, options).map(|entities| {
                    entities.into_iter().map(to_new_unit).collect()
                })
            }

            fn to_new_unit(entity: ExtractedEntity) -> NewCodeUnit {
                let display_source = entity
                    .representation_text(&RepresentationKind::FullSource)
                    .map(str::to_owned);
                let embedding_text = entity
                    .representation_text(&RepresentationKind::Implementation)
                    .map(str::to_owned);
                NewCodeUnit {
                    language_id: entity.language.into_inner(),
                    kind: entity.kind.as_str().to_owned(),
                    name: entity.name,
                    scope: entity.scope,
                    start_byte: entity.span.start_byte,
                    end_byte: entity.span.end_byte,
                    start_line: entity.span.start_line,
                    end_line: entity.span.end_line,
                    body_node_count: entity.body_node_count,
                    source_hash: entity.source_hash,
                    normalized_body_hash: entity.normalized_body_hash,
                    display_source,
                    embedding_text,
                }
            }
        }
    }

    pub mod embed;
    pub use embed::*;
    ''',
)


# ---------------------------------------------------------------------------
# codeindex-query: reusable metadata filters, stable selectors, and vector rank.
# ---------------------------------------------------------------------------
reset_dir("crates/codeindex-query/src")
write(
    "crates/codeindex-query/Cargo.toml",
    r'''
    [package]
    name = "codeindex-query"
    version = "0.1.0"
    edition = "2024"
    description = "Reusable filtering, stable selectors, and vector ranking for code indexes"
    publish = false

    [dependencies]
    anyhow = "1.0.103"
    codeindex-sqlite = { path = "../codeindex-sqlite" }
    globset = "0.4.18"
    hex = "0.4.3"
    sha2 = "0.11.0"
    ''',
)
write(
    "crates/codeindex-query/src/lib.rs",
    r'''
    #![forbid(unsafe_code)]

    use anyhow::{Context, Result, bail};
    use codeindex_sqlite::ModelIdentity;
    use globset::{GlobBuilder, GlobMatcher};
    use sha2::{Digest, Sha256};

    pub trait UnitView {
        fn project_label(&self) -> &str;
        fn relative_path(&self) -> &str;
        fn language_id(&self) -> &str;
        fn kind(&self) -> &str;
        fn name(&self) -> &str;
        fn scope(&self) -> Option<&str>;
        fn start_byte(&self) -> usize;
        fn end_byte(&self) -> usize;
        fn start_line(&self) -> usize;
        fn end_line(&self) -> usize;
        fn body_node_count(&self) -> usize;
        fn normalized_body_hash(&self) -> &str;
    }

    pub fn unit_id(unit: &impl UnitView) -> String {
        let ingredients = [
            unit.project_label(),
            unit.relative_path(),
            &unit.start_byte().to_string(),
            &unit.end_byte().to_string(),
            unit.normalized_body_hash(),
            unit.name(),
            unit.scope().unwrap_or(""),
            unit.language_id(),
        ]
        .join("\0");
        let digest = Sha256::digest(ingredients.as_bytes());
        format!("unit:{}", &hex::encode(digest)[..16])
    }

    pub fn unit_line(unit: &impl UnitView) -> String {
        let scope = unit.scope().map(|scope| format!(" ({scope})")).unwrap_or_default();
        format!(
            "{} {}:{}:{}-{} {} {}{}",
            unit_id(unit),
            unit.project_label(),
            unit.relative_path(),
            unit.start_line(),
            unit.end_line(),
            unit.kind(),
            unit.name(),
            scope,
        )
    }

    #[derive(Default)]
    pub struct WhereFilter {
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
        pub fn parse(expression: Option<&str>) -> Result<Self> {
            let mut filter = Self::default();
            let Some(expression) = expression else {
                return Ok(filter);
            };
            for clause in expression.split_whitespace() {
                let Some((key, value)) = clause.split_once('=') else {
                    bail!("malformed --where clause {clause:?}: expected key=value");
                };
                match key {
                    "project" => filter.project.push(value.to_owned()),
                    "language" => filter.language.push(value.to_owned()),
                    "kind" => filter.kind.push(value.to_owned()),
                    "name" => filter.name.push(glob(value, false)?),
                    "scope" => filter.scope.push(glob(value, false)?),
                    "path" => filter.path.push(glob(value, true)?),
                    "min_nodes" => {
                        filter.min_nodes = Some(value.parse().with_context(|| {
                            format!("min_nodes wants an integer, got {value:?}")
                        })?);
                    }
                    _ => bail!(
                        "unknown --where key {key:?} (supported: project, language, kind, name, scope, path, min_nodes)"
                    ),
                }
                filter.clauses.push(clause.to_owned());
            }
            Ok(filter)
        }

        pub fn matches(&self, unit: &impl UnitView) -> bool {
            let any_eq = |values: &[String], actual: &str| {
                values.is_empty() || values.iter().any(|value| value == actual)
            };
            let any_glob = |values: &[GlobMatcher], actual: &str| {
                values.is_empty() || values.iter().any(|value| value.is_match(actual))
            };
            any_eq(&self.project, unit.project_label())
                && any_eq(&self.language, unit.language_id())
                && any_eq(&self.kind, unit.kind())
                && any_glob(&self.name, unit.name())
                && (self.scope.is_empty()
                    || unit.scope().is_some_and(|scope| {
                        self.scope.iter().any(|matcher| matcher.is_match(scope))
                    }))
                && any_glob(&self.path, unit.relative_path())
                && self
                    .min_nodes
                    .is_none_or(|minimum| unit.body_node_count() >= minimum)
        }

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

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ScoredIndex {
        pub index: usize,
        pub score: f32,
    }

    pub fn dot(left: &[f32], right: &[f32]) -> f32 {
        left.iter().zip(right).map(|(a, b)| a * b).sum()
    }

    pub fn rank_candidates<'a>(
        query: &[f32],
        candidates: impl IntoIterator<Item = (usize, &'a [f32])>,
        threshold: f32,
    ) -> Vec<ScoredIndex> {
        let mut scored: Vec<ScoredIndex> = candidates
            .into_iter()
            .filter_map(|(index, vector)| {
                let score = dot(query, vector);
                (score >= threshold).then_some(ScoredIndex { index, score })
            })
            .collect();
        scored.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then(left.index.cmp(&right.index))
        });
        scored
    }

    pub fn identity_diff(stored: &ModelIdentity, current: &ModelIdentity) -> Vec<String> {
        let optional = |value: &Option<String>| value.clone().unwrap_or_else(|| "none".into());
        let mut differences = Vec::new();
        let mut field = |name: &str, left: String, right: String| {
            if left != right {
                differences.push(format!("{name} ({left:?} -> {right:?})"));
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
            optional(&stored.runtime_version),
            optional(&current.runtime_version),
        );
        field("model", stored.model.clone(), current.model.clone());
        field("revision", optional(&stored.revision), optional(&current.revision));
        field(
            "dimensions",
            stored.dimensions.to_string(),
            current.dimensions.to_string(),
        );
        field(
            "tokenizer_hash",
            optional(&stored.tokenizer_hash),
            optional(&current.tokenizer_hash),
        );
        field(
            "model_hash",
            optional(&stored.model_hash),
            optional(&current.model_hash),
        );
        field("normalize", stored.normalize.to_string(), current.normalize.to_string());
        field(
            "execution_provider",
            stored.execution_provider.clone(),
            current.execution_provider.clone(),
        );
        field(
            "quantization",
            optional(&stored.quantization),
            optional(&current.quantization),
        );
        field(
            "cache_path",
            optional(&stored.cache_path),
            optional(&current.cache_path),
        );
        differences
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[derive(Clone)]
        struct Unit {
            project: String,
            path: String,
            name: String,
            scope: Option<String>,
            nodes: usize,
        }

        impl UnitView for Unit {
            fn project_label(&self) -> &str { &self.project }
            fn relative_path(&self) -> &str { &self.path }
            fn language_id(&self) -> &str { "rust" }
            fn kind(&self) -> &str { "function" }
            fn name(&self) -> &str { &self.name }
            fn scope(&self) -> Option<&str> { self.scope.as_deref() }
            fn start_byte(&self) -> usize { 10 }
            fn end_byte(&self) -> usize { 90 }
            fn start_line(&self) -> usize { 2 }
            fn end_line(&self) -> usize { 9 }
            fn body_node_count(&self) -> usize { self.nodes }
            fn normalized_body_hash(&self) -> &str { "body" }
        }

        fn unit(path: &str) -> Unit {
            Unit {
                project: "main".into(),
                path: path.into(),
                name: "parse".into(),
                scope: Some("Parser".into()),
                nodes: 20,
            }
        }

        #[test]
        fn selectors_and_filters_are_deterministic() {
            let value = unit("src/parser.rs");
            assert_eq!(unit_id(&value), unit_id(&value));
            assert!(WhereFilter::parse(Some("path=src/** scope=Parser min_nodes=10"))
                .unwrap()
                .matches(&value));
            assert!(!WhereFilter::parse(Some("path=tests/**"))
                .unwrap()
                .matches(&value));
        }

        #[test]
        fn candidate_ranking_is_stable() {
            let first = [1.0, 0.0];
            let second = [0.5, 0.5];
            let ranked = rank_candidates(&first, [(1, second.as_slice()), (0, first.as_slice())], -1.0);
            assert_eq!(ranked[0].index, 0);
            assert_eq!(ranked[1].index, 1);
        }
    }
    ''',
)


# ---------------------------------------------------------------------------
# Umbrella crate: one dependency for future code-intelligence applications.
# ---------------------------------------------------------------------------
reset_dir("crates/codeindex/src")
write(
    "crates/codeindex/Cargo.toml",
    r'''
    [package]
    name = "codeindex"
    version = "0.1.0"
    edition = "2024"
    description = "Facade over the reusable codeindex crates"
    publish = false

    [dependencies]
    codeindex-core = { path = "../codeindex-core" }
    codeindex-embedding = { path = "../codeindex-embedding", default-features = false }
    codeindex-indexer = { path = "../codeindex-indexer" }
    codeindex-query = { path = "../codeindex-query" }
    codeindex-sqlite = { path = "../codeindex-sqlite" }
    codeindex-tree-sitter = { path = "../codeindex-tree-sitter" }

    [features]
    default = []
    fastembed = ["codeindex-embedding/fastembed"]
    accel = ["fastembed", "codeindex-embedding/accel"]
    cuda = ["accel", "codeindex-embedding/cuda"]
    directml = ["accel", "codeindex-embedding/directml"]
    coreml = ["accel", "codeindex-embedding/coreml"]
    openvino = ["accel", "codeindex-embedding/openvino"]
    load-dynamic = ["accel", "codeindex-embedding/load-dynamic"]
    ''',
)
write(
    "crates/codeindex/src/lib.rs",
    r'''
    #![forbid(unsafe_code)]

    pub use codeindex_core as core;
    pub use codeindex_embedding as embedding;
    pub use codeindex_indexer as indexer;
    pub use codeindex_query as query;
    pub use codeindex_sqlite as sqlite;
    pub use codeindex_tree_sitter as tree_sitter;
    ''',
)


# ---------------------------------------------------------------------------
# Decombine compatibility adapters. The CLI/analyzers keep their public module
# paths while all reusable implementation now lives under crates/.
# ---------------------------------------------------------------------------
write(
    "src/db/mod.rs",
    r'''
    pub use codeindex_sqlite::*;
    pub use codeindex_sqlite::{migrations, models};
    ''',
)
for stale in [ROOT / "src/db/models.rs", ROOT / "src/db/migrations.rs"]:
    stale.unlink(missing_ok=True)

write(
    "src/index/scanner.rs",
    r'''
    pub use codeindex_indexer::{ScannedFile, scan_files};
    ''',
)
write(
    "src/index/indexer.rs",
    r'''
    use anyhow::Result;

    use crate::config::{Config, RetentionMode};
    use crate::db::Db;

    pub use codeindex_indexer::ProjectStats;

    pub fn index(db: &Db, config: &Config, only_label: Option<&str>) -> Result<Vec<ProjectStats>> {
        let retention = match config.index.retention {
            RetentionMode::Full => codeindex_indexer::RetentionMode::Full,
            RetentionMode::Report => codeindex_indexer::RetentionMode::Report,
            RetentionMode::Minimal => codeindex_indexer::RetentionMode::Minimal,
        };
        let options = codeindex_indexer::IndexOptions {
            projects: config
                .resolved_projects()
                .into_iter()
                .map(|project| codeindex_indexer::ProjectSpec {
                    label: project.label,
                    source_dir: project.source_dir,
                    exclude: project.exclude,
                })
                .collect(),
            enabled_languages: config.languages.enabled.clone(),
            body_node_count_threshold: config.analysis.body_node_count_threshold,
            max_body_chars: config.embedding.max_body_chars,
            retention,
        };
        codeindex_indexer::index(db, &options, only_label)
    }
    ''',
)

write(
    "src/embed/mod.rs",
    r'''
    use anyhow::Result;

    use crate::config::{
        Config, CustomModelConfig as AppCustomModelConfig,
        EmbeddingConfig as AppEmbeddingConfig, ProviderMode as AppProviderMode,
    };
    use crate::db::{Db, ModelId, ModelIdentity};

    #[cfg(feature = "fastembed")]
    pub use codeindex_embedding::embed::fastembed_backend;
    pub use codeindex_embedding::embed::hash;
    pub use codeindex_embedding::{
        ACCELERATOR_PROVIDERS, EmbedProgress, EmbedStats, Embedder, LanguageTokens,
        ProviderDiag, TokenStats, accelerator_diagnostics, existing_model_id,
        normalize_in_place,
    };

    fn custom(config: &AppCustomModelConfig) -> codeindex_embedding::config::CustomModelConfig {
        codeindex_embedding::config::CustomModelConfig {
            dir: config.dir.clone(),
            onnx_file: config.onnx_file.clone(),
            dimensions: config.dimensions,
            pooling: config.pooling.clone(),
            max_length: config.max_length,
        }
    }

    fn embedding(config: &AppEmbeddingConfig) -> codeindex_embedding::config::EmbeddingConfig {
        codeindex_embedding::config::EmbeddingConfig {
            backend: config.backend.clone(),
            model: config.model.clone(),
            cache_dir: config.cache_dir.clone(),
            batch_size: config.batch_size,
            max_batch_chars: config.max_batch_chars,
            max_batch_token_area: config.max_batch_token_area,
            max_body_chars: config.max_body_chars,
            pending_page_size: config.pending_page_size,
            normalize: config.normalize,
            execution_provider: config.execution_provider.clone(),
            provider_mode: match config.provider_mode {
                AppProviderMode::Require => codeindex_embedding::config::ProviderMode::Require,
                AppProviderMode::Auto => codeindex_embedding::config::ProviderMode::Auto,
            },
            quantized: config.quantized,
            custom: config.custom.as_ref().map(custom),
        }
    }

    fn run_config(config: &Config) -> codeindex_embedding::config::Config {
        codeindex_embedding::config::Config {
            embedding: embedding(&config.embedding),
            analysis: codeindex_embedding::config::AnalysisConfig {
                body_node_count_threshold: config.analysis.body_node_count_threshold,
            },
        }
    }

    pub fn embedder_from_config(config: &Config) -> Result<Box<dyn Embedder>> {
        codeindex_embedding::embedder_from_config(&run_config(config))
    }

    pub fn embed_pending(
        db: &Db,
        embedder: &mut dyn Embedder,
        config: &Config,
    ) -> Result<EmbedStats> {
        codeindex_embedding::embed_pending(db, embedder, &run_config(config))
    }

    pub fn embed_pending_with_progress(
        db: &Db,
        embedder: &mut dyn Embedder,
        config: &Config,
        progress: impl FnMut(EmbedProgress),
    ) -> Result<EmbedStats> {
        codeindex_embedding::embed_pending_with_progress(
            db,
            embedder,
            &run_config(config),
            progress,
        )
    }

    pub fn token_report(
        db: &Db,
        config: &Config,
        embedder: &dyn Embedder,
    ) -> Result<Vec<LanguageTokens>> {
        codeindex_embedding::token_report(db, &run_config(config), embedder)
    }

    #[allow(dead_code)]
    fn _model_types_are_compatible(_: ModelId, _: &ModelIdentity) {}
    ''',
)

write(
    "src/query/mod.rs",
    r'''
    //! Agent-facing query command orchestration over reusable codeindex-query primitives.

    use std::path::Path;

    use anyhow::{Context as _, Result, ensure};
    use codeindex_query::{
        UnitView, WhereFilter, identity_diff, rank_candidates, unit_id, unit_line,
    };
    use serde_json::{Value, json};

    use crate::analyze::context::{AnalysisContext, CodeUnitRef, load_projects_and_units};
    use crate::cli::{CapabilitiesArgs, InspectArgs, SearchArgs, SimilarArgs, UnitsArgs};
    use crate::config::Config;
    use crate::db::{Db, ModelIdentity, Project};
    use crate::embed::normalize_in_place;

    pub const QUERY_SCHEMA_VERSION: &str = "decombine.query.v1";
    pub const CAPABILITIES_SCHEMA_VERSION: &str = "decombine.capabilities.v1";

    impl UnitView for CodeUnitRef {
        fn project_label(&self) -> &str { &self.project_label }
        fn relative_path(&self) -> &str { &self.relative_path }
        fn language_id(&self) -> &str { &self.language_id }
        fn kind(&self) -> &str { &self.kind }
        fn name(&self) -> &str { &self.name }
        fn scope(&self) -> Option<&str> { self.scope.as_deref() }
        fn start_byte(&self) -> usize { self.start_byte }
        fn end_byte(&self) -> usize { self.end_byte }
        fn start_line(&self) -> usize { self.start_line }
        fn end_line(&self) -> usize { self.end_line }
        fn body_node_count(&self) -> usize { self.body_node_count }
        fn normalized_body_hash(&self) -> &str { &self.normalized_body_hash }
    }

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
        ensure!(
            selector.starts_with("unit:"),
            "selector {selector:?} is not a unit selector (expected `unit:<id>` as printed by query/report output)"
        );
        units
            .iter()
            .position(|unit| unit_id(unit) == selector)
            .with_context(|| {
                format!(
                    "{selector} not found in the current index. Unit IDs are deterministic per index generation and change when code is re-indexed; re-run the query that produced the ID, or list units with `decombine query units`."
                )
            })
    }

    fn unit_source(projects: &[Project], unit: &CodeUnitRef) -> Option<String> {
        if let Some(source) = &unit.display_source {
            return Some(source.clone());
        }
        let project = projects.iter().find(|project| project.label == unit.project_label)?;
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
                Ok((db.count_embeddings(model.id)?, db.count_unembedded_hashes(model.id)?))
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
                model.identity.model,
                model.identity.dimensions,
                model.identity.execution_provider
            ),
            None => println!("model: none (run `decombine embed`)"),
        }
        if let Some((count, pending)) = embeddings {
            println!("embeddings: {count} bodies ({pending} pending)");
        }
        println!(
            "concerns: {} ({} queries)",
            if config.analysis.concerns.enabled { "enabled" } else { "disabled" },
            config.analysis.concerns.queries.len()
        );
        Ok(())
    }

    pub fn units(db: &Db, args: &UnitsArgs) -> Result<()> {
        let (_, all_units) = load_projects_and_units(db, &[])?;
        let filter = WhereFilter::parse(args.r#where.as_deref())?;
        let matching: Vec<&CodeUnitRef> = all_units.iter().filter(|unit| filter.matches(*unit)).collect();
        let matched = matching.len();
        let returned = args.limit.unwrap_or(matched).min(matched);
        if args.json {
            let items = matching[..returned].iter().map(|unit| unit_json(unit)).collect();
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
        let query_row = ctx.vectors.row_for_unit(query_index).expect("checked above");
        let query_vector = ctx.vectors.vector(query_row).to_vec();
        let candidates = (0..ctx.units.len()).filter_map(|index| {
            if index == query_index || !filter.matches(&ctx.units[index]) {
                return None;
            }
            let row = ctx.vectors.row_for_unit(index)?;
            Some((index, ctx.vectors.vector(row)))
        });
        let mut scored = rank_candidates(&query_vector, candidates, threshold);
        let matched = scored.len();
        scored.truncate(args.limit);

        if args.json {
            let items = scored
                .iter()
                .map(|scored| {
                    let mut item = unit_json(&ctx.units[scored.index]);
                    item["score"] = json!(scored.score);
                    if args.why {
                        item["why"] = json!({
                            "scores": {"cosine": scored.score},
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
        for scored in &scored {
            println!("{:.4} {}", scored.score, unit_line(&ctx.units[scored.index]));
        }
        if scored.len() < matched {
            eprintln!("(top {} of {matched} candidates; raise --limit for more)", scored.len());
        }
        Ok(())
    }

    pub fn search(config: &Config, db: &Db, args: &SearchArgs) -> Result<()> {
        let ctx = AnalysisContext::load(db, &[])?;
        let mut embedder = crate::embed::embedder_from_config(config)?;
        let identity = embedder.identity();
        ensure!(
            *identity == ctx.identity,
            "search queries must be embedded with the same model identity as the indexed code units; the configured embedder differs from the database on: {}",
            identity_diff(&ctx.identity, identity).join(", ")
        );
        let mut vectors = embedder.embed(std::slice::from_ref(&args.text))?;
        let query_vector = &mut vectors[0];
        normalize_in_place(query_vector);
        let filter = WhereFilter::parse(args.r#where.as_deref())?;
        let candidates = (0..ctx.units.len()).filter_map(|index| {
            if !filter.matches(&ctx.units[index]) {
                return None;
            }
            let row = ctx.vectors.row_for_unit(index)?;
            Some((index, ctx.vectors.vector(row)))
        });
        let mut scored = rank_candidates(query_vector, candidates, -1.0);
        let matched = scored.len();
        scored.truncate(args.limit);

        if args.json {
            let items = scored
                .iter()
                .map(|scored| {
                    let mut item = unit_json(&ctx.units[scored.index]);
                    item["score"] = json!(scored.score);
                    if args.why {
                        item["why"] = json!({
                            "scores": {"cosine": scored.score},
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
        for scored in &scored {
            println!("{:.4} {}", scored.score, unit_line(&ctx.units[scored.index]));
        }
        if scored.len() < matched {
            eprintln!("(top {} of {matched} embedded units; raise --limit for more)", scored.len());
        }
        Ok(())
    }
    ''',
)


# ---------------------------------------------------------------------------
# Workspace and feature forwarding.
# ---------------------------------------------------------------------------
write(
    "Cargo.toml",
    r'''
    [workspace]
    members = [
        ".",
        "crates/codeindex",
        "crates/codeindex-core",
        "crates/codeindex-embedding",
        "crates/codeindex-indexer",
        "crates/codeindex-query",
        "crates/codeindex-sqlite",
        "crates/codeindex-tree-sitter",
    ]
    resolver = "3"

    [package]
    name = "decombine"
    version = "0.1.0"
    edition = "2024"
    description = "Embedding-based detector for non-exact code duplication, concern signals, and project comparison"
    publish = false

    [dependencies]
    anyhow = "1.0.103"
    clap = { version = "4.6.1", features = ["derive"] }
    codeindex-core = { path = "crates/codeindex-core" }
    codeindex-embedding = { path = "crates/codeindex-embedding", default-features = false }
    codeindex-indexer = { path = "crates/codeindex-indexer" }
    codeindex-query = { path = "crates/codeindex-query" }
    codeindex-sqlite = { path = "crates/codeindex-sqlite" }
    codeindex-tree-sitter = { path = "crates/codeindex-tree-sitter" }
    globset = "0.4.18"
    hex = "0.4.3"
    ndarray = "0.17.2"
    rayon = "1.12.0"
    rusqlite = { version = "0.40.1", features = ["bundled"] }
    serde = { version = "1.0.228", features = ["derive"] }
    serde_json = "1.0.150"
    serde_yaml = "0.9.34"
    sha2 = "0.11.0"
    thiserror = "2.0.18"

    [features]
    default = ["fastembed"]
    fastembed = ["codeindex-embedding/fastembed"]
    accel = ["fastembed", "codeindex-embedding/accel"]
    cuda = ["accel", "codeindex-embedding/cuda"]
    directml = ["accel", "codeindex-embedding/directml"]
    coreml = ["accel", "codeindex-embedding/coreml"]
    openvino = ["accel", "codeindex-embedding/openvino"]
    load-dynamic = ["accel", "codeindex-embedding/load-dynamic"]

    [dev-dependencies]
    assert_cmd = "2.2.2"
    predicates = "3.1.4"
    tempfile = "3.27.0"

    [profile.release]
    lto = "thin"
    codegen-units = 1
    strip = "symbols"
    ''',
)

write(
    "docs/codeindex-architecture.md",
    r'''
    # Codeindex workspace architecture

    The reusable code-intelligence substrate is split by dependency and change
    boundary rather than by command:

    - `codeindex-core`: parser- and storage-neutral entities, spans, and textual
      representation channels.
    - `codeindex-tree-sitter`: bundled grammars, language adapters,
      normalization, and parser-neutral extraction.
    - `codeindex-sqlite`: the current incremental SQLite schema, migrations,
      model identities, vectors, and persistence API.
    - `codeindex-indexer`: filesystem scanning, change detection, extraction,
      retention, and transactional updates into `codeindex-sqlite`.
    - `codeindex-embedding`: local model execution, provider diagnostics,
      batching, source-text recovery, and resumable embedding projection.
    - `codeindex-query`: stable selectors, metadata filtering, identity
      diagnostics, and deterministic vector ranking.
    - `codeindex`: a thin facade for applications that prefer one dependency.

    `decombine` keeps configuration, CLI commands, duplicate/concern/comparison
    analyzers, and report rendering. Its `db`, `index`, `embed`, and `query`
    modules are compatibility adapters over the reusable crates, preserving the
    existing CLI and database behavior while allowing future binaries to consume
    the substrate directly.

    The current SQLite schema remains intentionally compatible in this
    extraction. Entity-version and multi-representation persistence are a
    separate schema migration, not hidden inside the crate move.
    ''',
)

print("full codeindex extraction generated")
