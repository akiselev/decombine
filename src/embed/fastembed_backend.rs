use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

use crate::config::EmbeddingConfig;
use crate::db::ModelIdentity;
use crate::embed::Embedder;

/// Local ONNX inference through the `fastembed` crate. Downloads the model
/// into the cache directory on first use; no API credentials involved.
pub struct FastembedBackend {
    identity: ModelIdentity,
    model: TextEmbedding,
    batch_size: usize,
}

/// Map a config model name (+ quantized flag) to the fastembed variant.
fn resolve_model(name: &str, quantized: bool) -> Result<EmbeddingModel> {
    Ok(match (name, quantized) {
        ("BGESmallENV15", false) => EmbeddingModel::BGESmallENV15,
        ("BGESmallENV15", true) => EmbeddingModel::BGESmallENV15Q,
        ("BGEBaseENV15", false) => EmbeddingModel::BGEBaseENV15,
        ("BGEBaseENV15", true) => EmbeddingModel::BGEBaseENV15Q,
        ("JinaEmbeddingsV2BaseCode", false) => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        _ => bail!("unsupported fastembed model {name:?} (quantized={quantized})"),
    })
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
            batch_size: config.batch_size,
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

    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        self.model
            .embed(inputs, Some(self.batch_size))
            .context("fastembed inference failed")
    }
}
