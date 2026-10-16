pub use codeindex_sqlite::*;
pub use codeindex_sqlite::{migrations, models};

use codeindex_core::{EmbeddingSpaceIdentity, Pooling, PromptContract};
use serde::{Deserialize, Serialize};

/// The model provenance decombine prints in reports and query envelopes: the
/// semantic contract of the analyzed space flattened together with the most
/// recent execution record (codeindex persists the two separately).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub backend: String,
    pub backend_version: String,
    pub runtime_version: Option<String>,
    pub model: String,
    pub revision: Option<String>,
    pub dimensions: usize,
    pub tokenizer_hash: Option<String>,
    pub model_hash: Option<String>,
    pub normalize: bool,
    pub execution_provider: String,
    pub quantization: Option<String>,
    pub cache_path: Option<String>,
}

impl ModelIdentity {
    pub fn from_parts(space: &EmbeddingSpaceIdentity, execution: Option<&ExecutionInfo>) -> Self {
        Self::from_contract(&space.model, space.effective_dimensions(), execution)
    }

    /// The identity of a loaded embedder (its contract plus live execution).
    pub fn of_embedder(embedder: &dyn codeindex_embedding::EmbeddingBackend) -> Self {
        let contract = embedder.contract();
        Self::from_contract(
            contract,
            contract.native_dimensions,
            Some(embedder.execution()),
        )
    }

    pub fn from_contract(
        contract: &ModelContract,
        dimensions: usize,
        execution: Option<&ExecutionInfo>,
    ) -> Self {
        ModelIdentity {
            backend: execution.map_or_else(|| "unknown".into(), |e| e.backend.clone()),
            backend_version: execution.map_or_else(String::new, |e| e.backend_version.clone()),
            runtime_version: execution.and_then(|e| e.runtime_version.clone()),
            model: contract.model.clone(),
            revision: contract.revision.clone(),
            dimensions,
            tokenizer_hash: contract.tokenizer_hash.clone(),
            model_hash: contract.model_hash.clone(),
            normalize: contract.normalize,
            execution_provider: execution
                .map_or_else(|| "unknown".into(), |e| e.execution_provider.clone()),
            quantization: contract.quantization.clone(),
            cache_path: execution.and_then(|e| e.cache_path.clone()),
        }
    }

    /// The execution half, for in-memory embedders built from an identity.
    pub fn execution(&self) -> ExecutionInfo {
        ExecutionInfo {
            backend: self.backend.clone(),
            backend_version: self.backend_version.clone(),
            runtime_version: self.runtime_version.clone(),
            execution_provider: self.execution_provider.clone(),
            cache_path: self.cache_path.clone(),
        }
    }

    /// A best-effort contract for in-memory contexts that have no stored space.
    pub fn contract(&self) -> ModelContract {
        ModelContract {
            model: self.model.clone(),
            revision: self.revision.clone(),
            model_hash: self.model_hash.clone(),
            tokenizer_hash: self.tokenizer_hash.clone(),
            pooling: Pooling::ModelDefined,
            normalize: self.normalize,
            native_dimensions: self.dimensions,
            max_sequence_length: 512,
            prompts: PromptContract::Symmetric,
            quantization: self.quantization.clone(),
        }
    }
}
