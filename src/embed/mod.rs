#[cfg(feature = "fastembed")]
pub mod fastembed_backend;
pub mod hash;

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::db::{Db, ModelId, ModelIdentity};
use crate::index::extractor::{ExtractOptions, extract_units};
use crate::index::language::LanguageRegistry;

/// A local embedding backend. Implementations must be deterministic for a
/// fixed `ModelIdentity`.
pub trait Embedder {
    fn identity(&self) -> &ModelIdentity;
    fn dimensions(&self) -> usize {
        self.identity().dimensions
    }
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Build the configured embedder. Only the fastembed backend is selectable
/// from config; the hash backend is for tests.
pub fn embedder_from_config(config: &Config) -> Result<Box<dyn Embedder>> {
    match config.embedding.backend.as_str() {
        #[cfg(feature = "fastembed")]
        "fastembed" => Ok(Box::new(fastembed_backend::FastembedBackend::new(
            &config.embedding,
        )?)),
        #[cfg(not(feature = "fastembed"))]
        "fastembed" => anyhow::bail!(
            "this binary was built without the `fastembed` feature; \
             rebuild with `cargo build --features fastembed`"
        ),
        other => anyhow::bail!("unsupported embedding backend {other:?}"),
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmbedStats {
    /// Distinct body hashes embedded in this run.
    pub embedded: usize,
    /// Pending hashes whose text could not be recovered (stale files in
    /// `minimal`/`report` retention).
    pub unresolved: usize,
    pub batches: usize,
}

/// Embed every distinct un-embedded body hash with this embedder, enforcing
/// model-identity immutability and resuming where a prior run stopped.
pub fn embed_pending(db: &Db, embedder: &mut dyn Embedder, config: &Config) -> Result<EmbedStats> {
    let identity = embedder.identity().clone();
    db.check_or_set_immutable("embedding.backend", &identity.backend)?;
    db.check_or_set_immutable("embedding.model", &identity.model)?;
    db.check_or_set_immutable("embedding.dimensions", &identity.dimensions.to_string())?;
    db.check_or_set_immutable("embedding.normalize", &identity.normalize.to_string())?;
    let model_id = db.find_or_create_model(&identity)?;

    let pending = db.unembedded_hashes(model_id)?;
    let mut resolved: Vec<(String, String)> = Vec::with_capacity(pending.len());
    let mut missing: Vec<String> = Vec::new();
    for (hash, text) in pending {
        match text {
            Some(text) => resolved.push((hash, text)),
            None => missing.push(hash),
        }
    }
    let mut stats = EmbedStats::default();
    if !missing.is_empty() {
        let recovered = recover_texts_from_source(db, config, &missing)?;
        for hash in missing {
            match recovered.get(&hash) {
                Some(text) => resolved.push((hash, text.clone())),
                None => stats.unresolved += 1,
            }
        }
    }
    resolved.sort_by(|a, b| a.0.cmp(&b.0));

    for batch in batch_by_size(
        &resolved,
        config.embedding.batch_size,
        config.embedding.max_batch_chars,
    ) {
        let texts: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();
        let vectors = embedder.embed(&texts)?;
        anyhow::ensure!(
            vectors.len() == batch.len(),
            "embedder returned {} vectors for {} inputs",
            vectors.len(),
            batch.len()
        );
        for ((hash, _), mut vector) in batch.iter().zip(vectors) {
            anyhow::ensure!(
                vector.len() == identity.dimensions,
                "model {} returned {} dimensions, expected {}",
                identity.model,
                vector.len(),
                identity.dimensions
            );
            if identity.normalize {
                normalize_in_place(&mut vector);
            }
            db.insert_embedding(model_id, hash, &vector)?;
            stats.embedded += 1;
        }
        stats.batches += 1;
    }
    Ok(stats)
}

/// The model row this config maps to, if embeddings exist already.
pub fn existing_model_id(db: &Db, identity: &ModelIdentity) -> Result<ModelId> {
    db.find_or_create_model(identity)
}

/// Split items into batches bounded by item count and total characters.
fn batch_by_size(
    items: &[(String, String)],
    max_items: usize,
    max_chars: usize,
) -> Vec<&[(String, String)]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    for (index, (_, text)) in items.iter().enumerate() {
        let len = text.chars().count();
        let at_capacity = index > start && (index - start >= max_items || chars + len > max_chars);
        if at_capacity {
            batches.push(&items[start..index]);
            start = index;
            chars = 0;
        }
        chars += len;
    }
    if start < items.len() {
        batches.push(&items[start..]);
    }
    batches
}

pub fn normalize_in_place(vector: &mut [f32]) {
    let norm = vector
        .iter()
        .map(|v| (*v as f64) * (*v as f64))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value = (*value as f64 / norm) as f32;
        }
    }
}

/// In `report`/`minimal` retention the embedding text is not stored;
/// recover it by re-extracting the source files that contain the pending
/// hashes. Files that changed since indexing simply fail to recover their
/// hash and are picked up on the next index+embed cycle.
fn recover_texts_from_source(
    db: &Db,
    config: &Config,
    hashes: &[String],
) -> Result<HashMap<String, String>> {
    let wanted: std::collections::HashSet<&str> = hashes.iter().map(|s| s.as_str()).collect();
    let locations = db.locations_for_hashes(hashes)?;
    let options = ExtractOptions {
        body_node_count_threshold: config.analysis.body_node_count_threshold,
        max_body_chars: config.embedding.max_body_chars,
    };
    let registry = LanguageRegistry::global();
    let mut recovered = HashMap::new();
    for location in locations {
        let path = std::path::Path::new(&location.source_dir).join(&location.relative_path);
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let def = registry
            .get(&location.language_id)
            .with_context(|| format!("unknown language {}", location.language_id))?;
        for unit in extract_units(def, &source, &options)? {
            if wanted.contains(unit.normalized_body_hash.as_str())
                && let Some(text) = unit.embedding_text
            {
                recovered.insert(unit.normalized_body_hash, text);
            }
        }
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(sizes: &[usize]) -> Vec<(String, String)> {
        sizes
            .iter()
            .enumerate()
            .map(|(i, len)| (format!("h{i}"), "x".repeat(*len)))
            .collect()
    }

    #[test]
    fn batches_respect_item_and_char_limits() {
        let items = pairs(&[10, 10, 10, 10, 10]);
        let by_count = batch_by_size(&items, 2, 1000);
        assert_eq!(by_count.len(), 3);
        assert_eq!(by_count[0].len(), 2);
        assert_eq!(by_count[2].len(), 1);

        let by_chars = batch_by_size(&items, 100, 25);
        assert_eq!(by_chars.len(), 3, "10+10 fits, third overflows 25");

        // A single oversized item still forms its own batch.
        let big = pairs(&[500]);
        assert_eq!(batch_by_size(&big, 10, 25).len(), 1);
    }

    #[test]
    fn normalization() {
        let mut vector = vec![3.0, 4.0];
        normalize_in_place(&mut vector);
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
        let mut zero = vec![0.0, 0.0];
        normalize_in_place(&mut zero);
        assert_eq!(zero, vec![0.0, 0.0]);
    }
}
