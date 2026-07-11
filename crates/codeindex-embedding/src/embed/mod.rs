#[cfg(feature = "fastembed")]
pub mod fastembed_backend;
pub mod hash;

use std::collections::{BTreeMap, HashMap};

use anyhow::{Context, Result};

use crate::config::{EmbeddingConfig, EmbeddingRunConfig};
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
    /// Tokens per input the model actually attends to (inputs are truncated
    /// beyond this). Bounds the padded-cost estimate in batch packing.
    fn max_sequence_length(&self) -> usize {
        512
    }
    /// Exact token count for one input using the model's own tokenizer,
    /// when the backend can provide it. Measured on the OSS eval corpora,
    /// the chars/4 fallback underestimates real token counts by 1.3–2.3x
    /// in padded area (Go worst), which is exactly the margin by which
    /// embed runs overshot the token-area memory budget.
    fn count_tokens(&self, _text: &str) -> Option<usize> {
        None
    }
    /// True token count with truncation disabled. `count_tokens` clamps at
    /// the model's truncation length (fastembed configures truncation on the
    /// inference tokenizer), which hides how far over the cap an input runs;
    /// this reveals the severity a capped model would silently drop.
    fn count_tokens_untruncated(&self, _text: &str) -> Option<usize> {
        None
    }
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// Build the configured embedder. Only the fastembed backend is selectable
/// from config; the hash backend is for tests.
pub fn embedder_from_config(config: &EmbeddingConfig) -> Result<Box<dyn Embedder>> {
    match config.backend.as_str() {
        #[cfg(feature = "fastembed")]
        "fastembed" => Ok(Box::new(fastembed_backend::FastembedBackend::new(config)?)),
        #[cfg(not(feature = "fastembed"))]
        "fastembed" => anyhow::bail!(
            "this binary was built without the `fastembed` feature; \
             rebuild with `cargo build --features fastembed`"
        ),
        other => anyhow::bail!("unsupported embedding backend {other:?}"),
    }
}

/// Accelerator execution providers the local backend can request, in priority order.
pub const ACCELERATOR_PROVIDERS: &[&str] = &["cuda", "directml", "coreml", "openvino"];

/// One provider's readiness, as reported by `doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDiag {
    pub name: &'static str,
    /// This binary was built with the provider's cargo feature.
    pub compiled: bool,
    /// ONNX Runtime was built with support for the provider (`None` when not
    /// compiled in, so it cannot be queried).
    pub available: Option<bool>,
    /// The provider can run on this OS/arch at all.
    pub platform_supported: Option<bool>,
}

/// Compile- and runtime status of each accelerator provider. On a CPU-only
/// build every provider reports `compiled: false`; with `accel` features the
/// ONNX Runtime availability and platform support are queried live.
pub fn accelerator_diagnostics() -> Vec<ProviderDiag> {
    #[cfg(feature = "accel")]
    {
        use ort::ep::{CUDA, CoreML, DirectML, ExecutionProvider, OpenVINO};
        fn diag(name: &'static str, compiled: bool, ep: &dyn ExecutionProvider) -> ProviderDiag {
            ProviderDiag {
                name,
                compiled,
                available: Some(ep.is_available().unwrap_or(false)),
                platform_supported: Some(ep.supported_by_platform()),
            }
        }
        vec![
            diag("cuda", cfg!(feature = "cuda"), &CUDA::default()),
            diag("directml", cfg!(feature = "directml"), &DirectML::default()),
            diag("coreml", cfg!(feature = "coreml"), &CoreML::default()),
            diag("openvino", cfg!(feature = "openvino"), &OpenVINO::default()),
        ]
    }
    #[cfg(not(feature = "accel"))]
    {
        ACCELERATOR_PROVIDERS
            .iter()
            .map(|name| ProviderDiag {
                name,
                compiled: false,
                available: None,
                platform_supported: None,
            })
            .collect()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmbedStats {
    /// Distinct body hashes pending at the start of this run.
    pub pending_total: usize,
    /// Distinct body hashes embedded in this run.
    pub embedded: usize,
    /// Pending hashes whose text could not be recovered (stale files in
    /// `minimal`/`report` retention).
    pub unresolved: usize,
    pub batches: usize,
    /// Token-length instrumentation over embedded inputs (post-truncation
    /// counts from the packer).
    pub tokens: TokenStats,
}

/// Distribution of input token lengths plus padding/truncation costs, using
/// the same counts the batch packer budgets with.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TokenStats {
    /// Sorted at read time via `percentile`; append-only during the run.
    lengths: Vec<usize>,
    /// Inputs at the model's truncation length (content beyond it is lost).
    pub truncated: usize,
    /// Sum of real token positions across inputs.
    pub token_positions: u64,
    /// Sum of padded positions actually materialized per batch
    /// (`batch_len * longest_in_batch`).
    pub padded_positions: u64,
}

impl TokenStats {
    fn record_batch(&mut self, tokens: impl Iterator<Item = usize> + Clone, max_len: usize) {
        let longest = tokens.clone().max().unwrap_or(0);
        let mut count = 0_u64;
        for t in tokens {
            self.lengths.push(t);
            self.token_positions += t as u64;
            if t >= max_len {
                self.truncated += 1;
            }
            count += 1;
        }
        self.padded_positions += count * longest as u64;
    }

    /// Record one input's true (untruncated) token length for distribution
    /// reporting. Unlike `record_batch` this tracks no padding — the input is
    /// being measured, not batched — so `padding_waste`/`truncated` stay 0 and
    /// over-cap counts come from `over` instead.
    pub fn record_length(&mut self, tokens: usize) {
        self.lengths.push(tokens);
        self.token_positions += tokens as u64;
    }

    /// Fold another distribution into this one (for an all-languages total).
    pub fn merge(&mut self, other: &TokenStats) {
        self.lengths.extend_from_slice(&other.lengths);
        self.truncated += other.truncated;
        self.token_positions += other.token_positions;
        self.padded_positions += other.padded_positions;
    }

    /// Inputs strictly longer than `threshold` tokens — the content a model
    /// capped at `threshold` would silently drop.
    pub fn over(&self, threshold: usize) -> usize {
        self.lengths.iter().filter(|&&t| t > threshold).count()
    }

    pub fn percentile(&self, q: f64) -> usize {
        if self.lengths.is_empty() {
            return 0;
        }
        let mut sorted = self.lengths.clone();
        sorted.sort_unstable();
        sorted[((q * (sorted.len() - 1) as f64).round() as usize).min(sorted.len() - 1)]
    }

    pub fn max(&self) -> usize {
        self.lengths.iter().copied().max().unwrap_or(0)
    }

    pub fn count(&self) -> usize {
        self.lengths.len()
    }

    /// Fraction of materialized positions that are padding.
    pub fn padding_waste(&self) -> f64 {
        if self.padded_positions == 0 {
            return 0.0;
        }
        1.0 - self.token_positions as f64 / self.padded_positions as f64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedProgress {
    pub pending_total: usize,
    pub embedded: usize,
    pub unresolved: usize,
    pub batches: usize,
    pub current_batch: usize,
}

/// Embed every distinct un-embedded body hash with this embedder, enforcing
/// model-identity immutability and resuming where a prior run stopped.
pub fn embed_pending(
    db: &Db,
    embedder: &mut dyn Embedder,
    config: &EmbeddingRunConfig,
) -> Result<EmbedStats> {
    embed_pending_with_progress(db, embedder, config, |_| {})
}

pub fn embed_pending_with_progress(
    db: &Db,
    embedder: &mut dyn Embedder,
    config: &EmbeddingRunConfig,
    mut progress: impl FnMut(EmbedProgress),
) -> Result<EmbedStats> {
    let identity = embedder.identity().clone();
    db.check_or_set_immutable("embedding.backend", &identity.backend)?;
    db.check_or_set_immutable("embedding.model", &identity.model)?;
    db.check_or_set_immutable("embedding.dimensions", &identity.dimensions.to_string())?;
    db.check_or_set_immutable("embedding.normalize", &identity.normalize.to_string())?;
    let model_id = db.find_or_create_model(&identity)?;

    let pending_total = db.count_unembedded_hashes(model_id)? as usize;
    let mut stats = EmbedStats {
        pending_total,
        ..EmbedStats::default()
    };
    let mut after_hash: Option<String> = None;
    loop {
        let pending = db.unembedded_hashes_page(
            model_id,
            after_hash.as_deref(),
            config.embedding.pending_page_size,
        )?;
        if pending.is_empty() {
            break;
        }
        after_hash = pending.last().map(|(hash, _)| hash.clone());

        let mut resolved: Vec<(String, String)> = Vec::with_capacity(pending.len());
        let mut missing: Vec<String> = Vec::new();
        for (hash, text) in pending {
            match text {
                Some(text) => resolved.push((hash, text)),
                None => missing.push(hash),
            }
        }
        if !missing.is_empty() {
            let recovered = recover_texts_from_source(db, config, &missing)?;
            for hash in missing {
                match recovered.get(&hash) {
                    Some(text) => resolved.push((hash, text.clone())),
                    None => stats.unresolved += 1,
                }
            }
        }
        // Pack length-sorted items so every batch's padded token area
        // (count x longest-item-tokens^2, the term ONNX attention memory
        // scales with) stays under budget. Token counts come from the
        // model's own tokenizer when available — the chars/4 estimate
        // undershoots real counts enough to blow the budget. Hash order
        // only matters for the page cursor above, not within a page.
        let max_sequence_length = embedder.max_sequence_length();
        let mut sized: Vec<(String, String, usize)> = resolved
            .into_iter()
            .map(|(hash, text)| {
                let tokens = embedder
                    .count_tokens(&text)
                    .unwrap_or_else(|| estimated_tokens(&text, max_sequence_length))
                    .min(max_sequence_length.max(1));
                (hash, text, tokens)
            })
            .collect();
        sized.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

        for batch in pack_batches(
            &sized,
            config.embedding.batch_size,
            config.embedding.max_batch_chars,
            config.embedding.max_batch_token_area,
        ) {
            stats
                .tokens
                .record_batch(batch.iter().map(|(_, _, t)| *t), max_sequence_length);
            let texts: Vec<String> = batch.iter().map(|(_, text, _)| text.clone()).collect();
            let vectors = embedder.embed(&texts)?;
            anyhow::ensure!(
                vectors.len() == batch.len(),
                "embedder returned {} vectors for {} inputs",
                vectors.len(),
                batch.len()
            );
            for ((hash, _, _), mut vector) in batch.iter().zip(vectors) {
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
            progress(EmbedProgress {
                pending_total: stats.pending_total,
                embedded: stats.embedded,
                unresolved: stats.unresolved,
                batches: stats.batches,
                current_batch: batch.len(),
            });
        }
    }
    Ok(stats)
}

/// The model row this config maps to, if embeddings exist already.
pub fn existing_model_id(db: &Db, identity: &ModelIdentity) -> Result<ModelId> {
    db.find_or_create_model(identity)
}

/// One language's untruncated token-length distribution.
#[derive(Debug, Clone)]
pub struct LanguageTokens {
    pub language: String,
    pub stats: TokenStats,
}

/// Measure the true (untruncated) token-length distribution of every indexed
/// unit body, bucketed by language, using the model's own tokenizer. Unlike
/// the embed-time stats this scans all units regardless of embedding state
/// (so it works on already-embedded DBs) and recovers text from source under
/// report/minimal retention. Units whose text cannot be recovered, or whose
/// backend cannot count tokens, are skipped.
pub fn token_report(
    db: &Db,
    config: &EmbeddingRunConfig,
    embedder: &dyn Embedder,
) -> Result<Vec<LanguageTokens>> {
    let rows = db.all_unit_texts()?;
    let missing: Vec<String> = rows
        .iter()
        .filter(|(_, _, text)| text.is_none())
        .map(|(hash, _, _)| hash.clone())
        .collect();
    let recovered = if missing.is_empty() {
        HashMap::new()
    } else {
        recover_texts_from_source(db, config, &missing)?
    };

    let mut by_language: BTreeMap<String, TokenStats> = BTreeMap::new();
    for (hash, language, text) in rows {
        let text = match text {
            Some(text) => text,
            None => match recovered.get(&hash) {
                Some(text) => text.clone(),
                None => continue,
            },
        };
        let Some(tokens) = embedder.count_tokens_untruncated(&text) else {
            continue;
        };
        by_language
            .entry(language)
            .or_default()
            .record_length(tokens);
    }
    Ok(by_language
        .into_iter()
        .map(|(language, stats)| LanguageTokens { language, stats })
        .collect())
}

/// Rough tokens for a code body: ~4 chars/token, clamped to what the model
/// will actually attend to after truncation.
fn estimated_tokens(text: &str, max_sequence_length: usize) -> usize {
    (text.chars().count() / 4 + 1).min(max_sequence_length.max(1))
}

/// Split (hash, text, tokens) items into batches bounded by item count,
/// total characters, and padded token area. ONNX runtime pads every input
/// to the longest in the batch and materializes attention over it, so peak
/// memory scales with `count * longest_tokens^2`; that product is what
/// `max_token_area` caps. Items must be sorted longest-first so the running
/// maximum is the first item and long bodies batch together instead of
/// inflating short ones.
fn pack_batches(
    items: &[(String, String, usize)],
    max_items: usize,
    max_chars: usize,
    max_token_area: usize,
) -> Vec<&[(String, String, usize)]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    let mut longest_tokens = 0;
    for (index, (_, text, tokens)) in items.iter().enumerate() {
        let len = text.chars().count();
        let longest = longest_tokens.max(*tokens);
        let at_capacity = index > start
            && (index - start >= max_items
                || chars + len > max_chars
                || (index - start + 1) * longest * longest > max_token_area);
        if at_capacity {
            batches.push(&items[start..index]);
            start = index;
            chars = 0;
            longest_tokens = 0;
        }
        chars += len;
        longest_tokens = longest_tokens.max(*tokens);
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
    config: &EmbeddingRunConfig,
    hashes: &[String],
) -> Result<HashMap<String, String>> {
    let wanted: std::collections::HashSet<&str> = hashes.iter().map(|s| s.as_str()).collect();
    let locations = db.locations_for_hashes(hashes)?;
    let options = ExtractOptions {
        body_node_count_threshold: config.source_recovery.body_node_count_threshold,
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

    /// Items sized in chars, tokens derived with the chars/4 estimate
    /// clamped at 500 — mirrors the fallback path.
    fn pairs(sizes: &[usize]) -> Vec<(String, String, usize)> {
        sizes
            .iter()
            .enumerate()
            .map(|(i, len)| {
                let text = "x".repeat(*len);
                let tokens = estimated_tokens(&text, 500);
                (format!("h{i}"), text, tokens)
            })
            .collect()
    }

    #[test]
    fn batches_respect_item_and_char_limits() {
        let items = pairs(&[10, 10, 10, 10, 10]);
        let by_count = pack_batches(&items, 2, 1000, usize::MAX);
        assert_eq!(by_count.len(), 3);
        assert_eq!(by_count[0].len(), 2);
        assert_eq!(by_count[2].len(), 1);

        let by_chars = pack_batches(&items, 100, 25, usize::MAX);
        assert_eq!(by_chars.len(), 3, "10+10 fits, third overflows 25");

        // A single oversized item still forms its own batch.
        let big = pairs(&[500]);
        assert_eq!(pack_batches(&big, 10, 25, usize::MAX).len(), 1);
    }

    #[test]
    fn batches_respect_token_area_budget() {
        // 2000 chars ~ 501 tokens, clamped to 500 -> area 250_000 per item.
        // Budget 1_000_000 fits four such items per batch.
        let items = pairs(&[2000, 2000, 2000, 2000, 2000, 2000]);
        let batches = pack_batches(&items, 100, usize::MAX, 1_000_000);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 4);
        assert_eq!(batches[1].len(), 2);

        // Longest-first input: one long item (500 tokens after clamping)
        // caps its batch at 4, then the short tail (26 tokens) packs densely.
        let mut sizes = vec![2000usize];
        sizes.extend(std::iter::repeat_n(100, 20));
        let mixed = pairs(&sizes);
        let batches = pack_batches(&mixed, 100, usize::MAX, 1_000_000);
        assert_eq!(batches[0].len(), 4, "long head limits the first batch");
        assert_eq!(batches.len(), 2, "short tail packs into one batch");

        // A single item over budget still embeds alone.
        let big = pairs(&[4000]);
        assert_eq!(pack_batches(&big, 10, usize::MAX, 1000).len(), 1);

        // Exact token counts override any char-based intuition: 100-char
        // items reported as 500 tokens each pack like long items.
        let dense: Vec<(String, String, usize)> = (0..6)
            .map(|i| (format!("d{i}"), "y".repeat(100), 500))
            .collect();
        let batches = pack_batches(&dense, 100, usize::MAX, 1_000_000);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 4);
    }

    #[test]
    fn token_stats_percentiles_truncation_and_waste() {
        let mut stats = TokenStats::default();
        // One batch padded to 500: 3 items -> 1500 padded, 1000 real.
        stats.record_batch([500usize, 300, 200].into_iter(), 500);
        // A dense batch with no padding.
        stats.record_batch([100usize, 100].into_iter(), 500);
        assert_eq!(stats.count(), 5);
        assert_eq!(stats.truncated, 1);
        assert_eq!(stats.max(), 500);
        assert_eq!(stats.percentile(0.5), 200);
        assert_eq!(stats.token_positions, 1200);
        assert_eq!(stats.padded_positions, 1700);
        assert!((stats.padding_waste() - (1.0 - 1200.0 / 1700.0)).abs() < 1e-9);
    }

    #[test]
    fn record_length_over_and_merge() {
        let mut a = TokenStats::default();
        for len in [100usize, 600, 2100, 300] {
            a.record_length(len);
        }
        assert_eq!(a.count(), 4);
        assert_eq!(a.max(), 2100);
        assert_eq!(a.over(512), 2, "600 and 2100 exceed 512");
        assert_eq!(a.over(2048), 1, "only 2100 exceeds 2048");
        // record_length tracks no padding, so waste stays zero.
        assert_eq!(a.padding_waste(), 0.0);

        let mut b = TokenStats::default();
        b.record_length(700);
        a.merge(&b);
        assert_eq!(a.count(), 5);
        assert_eq!(a.over(512), 3);
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
