use anyhow::Result;

use crate::config::{
    Config, CustomModelConfig as AppCustomModelConfig, EmbeddingConfig as AppEmbeddingConfig,
    ProviderMode as AppProviderMode,
};
use crate::db::{Db, ModelId, ModelIdentity};

#[cfg(feature = "fastembed")]
pub use codeindex_embedding::embed::fastembed_backend;
pub use codeindex_embedding::embed::hash;
pub use codeindex_embedding::{
    ACCELERATOR_PROVIDERS, EmbedProgress, EmbedStats, Embedder, LanguageTokens, ProviderDiag,
    TokenStats, accelerator_diagnostics, existing_model_id, normalize_in_place,
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

pub fn embed_pending(db: &Db, embedder: &mut dyn Embedder, config: &Config) -> Result<EmbedStats> {
    codeindex_embedding::embed_pending(db, embedder, &run_config(config))
}

pub fn embed_pending_with_progress(
    db: &Db,
    embedder: &mut dyn Embedder,
    config: &Config,
    progress: impl FnMut(EmbedProgress),
) -> Result<EmbedStats> {
    codeindex_embedding::embed_pending_with_progress(db, embedder, &run_config(config), progress)
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
