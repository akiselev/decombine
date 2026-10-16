pub mod supervisor;

use anyhow::Result;
use codeindex_core::EmbeddingSpaceIdentity;

use crate::analyze::context::{decombine_channel, decombine_space_id};
use crate::config::{
    Config, CustomModelConfig as AppCustomModelConfig, EmbeddingConfig as AppEmbeddingConfig,
    ProviderMode as AppProviderMode, RemoteConfig as AppRemoteConfig,
};
use crate::db::Db;

#[cfg(feature = "fastembed")]
pub use codeindex_embedding::embed::fastembed_backend;
pub use codeindex_embedding::embed::hash;
pub use codeindex_embedding::{
    ACCELERATOR_PROVIDERS, EmbedRequest, EmbeddingBackend, EmbeddingBackend as Embedder,
    ProviderDiag, TokenStats, accelerator_diagnostics, apply_output_dimensions, normalize_in_place,
};
pub use codeindex_indexer::{EmbedProgress, EmbedStats, LanguageTokens, find_or_create_model_id};

fn custom(config: &AppCustomModelConfig) -> codeindex_embedding::config::CustomModelConfig {
    codeindex_embedding::config::CustomModelConfig {
        dir: config.dir.clone(),
        onnx_file: config.onnx_file.clone(),
        dimensions: config.dimensions,
        pooling: config.pooling.clone(),
        max_length: config.max_length,
    }
}

fn remote(config: &AppRemoteConfig) -> codeindex_embedding::config::RemoteModelConfig {
    use crate::config::DataPolicy as App;
    use codeindex_embedding::config::DataPolicy;
    codeindex_embedding::config::RemoteModelConfig {
        data_policy: Some(match config.data_policy {
            App::Zdr => DataPolicy::Zdr,
            App::NoTraining => DataPolicy::NoTraining,
            App::ProviderDefault => DataPolicy::ProviderDefault,
        }),
        base_url: config.base_url.clone(),
        api_key_env: config.api_key_env.clone(),
        dimensions: config.dimensions,
        request_dimensions: config.request_dimensions,
        max_input_tokens: config.max_input_tokens,
        providers: config.providers.clone(),
        quantizations: config.quantizations.clone(),
        document_input_type: config.document_input_type.clone(),
        query_input_type: config.query_input_type.clone(),
        max_inputs_per_request: config.max_inputs_per_request,
        concurrency: config.concurrency,
        query_prefix: config.query_prefix.clone(),
        document_prefix: config.document_prefix.clone(),
        timeout_secs: config.timeout_secs,
        max_retries: config.max_retries,
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
        remote: config.remote.as_ref().map(remote),
        // A supervised child is already guarded by its parent's timer.
        memory_guard: config
            .memory_guard
            .filter(|_| !supervisor::is_supervised_child())
            .map(|guard| {
                codeindex_embedding::memory::MemoryGuard::new(
                    guard.pause_below_mb,
                    guard.resume_above_mb(),
                )
            }),
    }
}

fn run_config(config: &Config) -> codeindex_embedding::config::EmbeddingRunConfig {
    codeindex_embedding::config::EmbeddingRunConfig {
        embedding: embedding(&config.embedding),
        source_recovery: codeindex_embedding::config::SourceRecoveryConfig {
            body_node_count_threshold: config.analysis.body_node_count_threshold,
        },
    }
}

pub fn embedder_from_config(config: &Config) -> Result<Box<dyn Embedder>> {
    codeindex_embedding::embedder_from_config(&embedding(&config.embedding))
}

/// The decombine space for the loaded model: the implementation channel only.
/// codeindex's own `embed_pending` embeds every representation channel, which
/// decombine never reads.
fn space(embedder: &dyn Embedder) -> EmbeddingSpaceIdentity {
    EmbeddingSpaceIdentity::new(
        decombine_space_id(),
        decombine_channel(),
        embedder.contract().clone(),
    )
}

pub fn embed_pending(db: &Db, embedder: &mut dyn Embedder, config: &Config) -> Result<EmbedStats> {
    embed_pending_with_progress(db, embedder, config, |_| {})
}

pub fn embed_pending_with_progress(
    db: &Db,
    embedder: &mut dyn Embedder,
    config: &Config,
    mut progress: impl FnMut(EmbedProgress),
) -> Result<EmbedStats> {
    let space = space(embedder);
    if let Some(existing) = db.get_space(&space.id)?
        && existing.identity.model != space.model
    {
        anyhow::bail!(
            "this database was embedded with model {:?} but embedding.model is {:?} \
             (differing: {}); use one database per model",
            existing.identity.model.model,
            space.model.model,
            codeindex_query::contract_diff(&existing.identity.model, &space.model).join(", ")
        );
    }
    codeindex_indexer::embed_space_pending_with_progress(
        db,
        embedder,
        &run_config(config),
        &space,
        None,
        &mut progress,
    )
}

pub fn token_report(
    db: &Db,
    config: &Config,
    embedder: &dyn Embedder,
) -> Result<Vec<LanguageTokens>> {
    codeindex_indexer::token_report_channel(
        db,
        &run_config(config),
        embedder,
        &decombine_channel(),
        None,
    )
}

/// The timer-based guard's poll interval, when the config asks for
/// supervision and this process is not already the supervised child.
pub fn supervision(
    config: &Config,
) -> Option<(crate::config::MemoryGuardConfig, std::time::Duration)> {
    let guard = config.embedding.memory_guard?;
    let interval = guard.watch_interval_ms?;
    (!supervisor::is_supervised_child())
        .then(|| (guard, std::time::Duration::from_millis(interval.max(100))))
}
