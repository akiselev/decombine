use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use fastembed::{
    EmbeddingModel, InitOptionsUserDefined, Pooling, TextEmbedding, TextInitOptions,
    TokenizerFiles, UserDefinedEmbeddingModel,
};

use crate::config::{CustomModelConfig, EmbeddingConfig};
use crate::db::ModelIdentity;
use crate::embed::Embedder;

/// Local ONNX inference through the `fastembed` crate. Downloads the model
/// into the cache directory on first use; no API credentials involved.
pub struct FastembedBackend {
    identity: ModelIdentity,
    model: TextEmbedding,
    max_sequence_length: usize,
}

/// Tokenizer truncation length fastembed applies to catalog models (its
/// `DEFAULT_MAX_LENGTH`); custom models use their configured `max_length`.
const CATALOG_MAX_LENGTH: usize = 512;

/// Map a config model name (+ quantized flag) to the fastembed variant.
fn resolve_model(name: &str, quantized: bool) -> Result<EmbeddingModel> {
    Ok(match (name, quantized) {
        ("BGESmallENV15", false) => EmbeddingModel::BGESmallENV15,
        ("BGESmallENV15", true) => EmbeddingModel::BGESmallENV15Q,
        ("BGEBaseENV15", false) => EmbeddingModel::BGEBaseENV15,
        ("BGEBaseENV15", true) => EmbeddingModel::BGEBaseENV15Q,
        ("JinaEmbeddingsV2BaseCode", false) => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        ("AllMiniLML6V2", false) => EmbeddingModel::AllMiniLML6V2,
        ("AllMiniLML6V2", true) => EmbeddingModel::AllMiniLML6V2Q,
        ("GTEBaseENV15", false) => EmbeddingModel::GTEBaseENV15,
        ("GTEBaseENV15", true) => EmbeddingModel::GTEBaseENV15Q,
        ("SnowflakeArcticEmbedM", false) => EmbeddingModel::SnowflakeArcticEmbedM,
        ("SnowflakeArcticEmbedM", true) => EmbeddingModel::SnowflakeArcticEmbedMQ,
        ("SnowflakeArcticEmbedMLong", false) => EmbeddingModel::SnowflakeArcticEmbedMLong,
        ("SnowflakeArcticEmbedMLong", true) => EmbeddingModel::SnowflakeArcticEmbedMLongQ,
        ("NomicEmbedTextV15", false) => EmbeddingModel::NomicEmbedTextV15,
        ("NomicEmbedTextV15", true) => EmbeddingModel::NomicEmbedTextV15Q,
        _ => bail!("unsupported fastembed model {name:?} (quantized={quantized})"),
    })
}

/// Load a locally exported ONNX model through fastembed's user-defined
/// model path. No download or cache: the files must already exist.
fn load_custom_model(custom: &CustomModelConfig) -> Result<TextEmbedding> {
    let read = |name: &std::path::Path| {
        std::fs::read(custom.dir.join(name)).with_context(|| {
            format!(
                "reading custom model file {}",
                custom.dir.join(name).display()
            )
        })
    };
    let tokenizer_files = TokenizerFiles {
        tokenizer_file: read(std::path::Path::new("tokenizer.json"))?,
        config_file: read(std::path::Path::new("config.json"))?,
        special_tokens_map_file: read(std::path::Path::new("special_tokens_map.json"))?,
        tokenizer_config_file: read(std::path::Path::new("tokenizer_config.json"))?,
    };
    let pooling = match custom.pooling.as_str() {
        "cls" => Pooling::Cls,
        _ => Pooling::Mean,
    };
    let model = UserDefinedEmbeddingModel::new(read(&custom.onnx_file)?, tokenizer_files)
        .with_pooling(pooling);
    let options = InitOptionsUserDefined::new().with_max_length(custom.max_length);
    TextEmbedding::try_new_from_user_defined(model, options)
        .with_context(|| format!("loading custom ONNX model from {}", custom.dir.display()))
}

impl FastembedBackend {
    pub fn new(config: &EmbeddingConfig) -> Result<Self> {
        if config.execution_provider != "cpu" {
            bail!(
                "execution provider {:?} is not compiled into this binary; only `cpu` is \
                 currently supported by the fastembed backend",
                config.execution_provider
            );
        }
        if let Some(custom) = &config.custom {
            let model = load_custom_model(custom)?;
            return Ok(Self {
                identity: ModelIdentity {
                    backend: "fastembed".into(),
                    backend_version: env!("DECOMBINE_FASTEMBED_VERSION").into(),
                    runtime_version: Some(format!("ort {}", env!("DECOMBINE_ORT_VERSION"))),
                    model: config.model.clone(),
                    revision: Some(format!(
                        "custom:{} pooling={} max_length={}",
                        custom.dir.join(&custom.onnx_file).display(),
                        custom.pooling,
                        custom.max_length
                    )),
                    dimensions: custom.dimensions,
                    tokenizer_hash: None,
                    model_hash: None,
                    normalize: config.normalize,
                    execution_provider: config.execution_provider.clone(),
                    quantization: None,
                    cache_path: Some(custom.dir.to_string_lossy().into_owned()),
                },
                model,
                max_sequence_length: custom.max_length,
            });
        }
        let model_name = resolve_model(&config.model, config.quantized)?;
        let info = TextEmbedding::get_model_info(&model_name)
            .context("fastembed has no metadata for the selected model")?;
        let dimensions = info.dim;
        let model_code = info.model_code.clone();

        let cache_dir = match &config.cache_dir {
            Some(dir) => dir.clone(),
            None => std::env::var_os("FASTEMBED_CACHE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(default_cache_dir),
        };
        let options = TextInitOptions::new(model_name)
            .with_show_download_progress(true)
            .with_cache_dir(cache_dir.clone());
        let model = TextEmbedding::try_new(options)
            .with_context(|| format!("loading fastembed model {}", config.model))?;

        Ok(Self {
            identity: ModelIdentity {
                backend: "fastembed".into(),
                backend_version: env!("DECOMBINE_FASTEMBED_VERSION").into(),
                runtime_version: Some(format!("ort {}", env!("DECOMBINE_ORT_VERSION"))),
                model: config.model.clone(),
                revision: Some(model_code),
                dimensions,
                tokenizer_hash: None,
                model_hash: None,
                normalize: config.normalize,
                execution_provider: config.execution_provider.clone(),
                quantization: config.quantized.then(|| "quantized".to_string()),
                cache_path: Some(cache_dir.to_string_lossy().into_owned()),
            },
            model,
            max_sequence_length: CATALOG_MAX_LENGTH,
        })
    }
}

fn default_cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("decombine").join("models");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("decombine")
            .join("models");
    }
    PathBuf::from(".decombine-models")
}

impl Embedder for FastembedBackend {
    fn identity(&self) -> &ModelIdentity {
        &self.identity
    }

    fn max_sequence_length(&self) -> usize {
        self.max_sequence_length
    }

    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        // The caller packs batches to a padded-memory budget; pass each
        // through as a single fastembed batch so that budget is authoritative.
        self.model
            .embed(inputs, Some(inputs.len().max(1)))
            .context("fastembed inference failed")
    }
}
