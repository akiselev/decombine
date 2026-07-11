use serde::{Deserialize, Serialize};

pub type ProjectId = i64;
pub type FileId = i64;
pub type UnitId = i64;
pub type ModelId = i64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: ProjectId,
    pub label: String,
    pub source_dir: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub id: FileId,
    pub project_id: ProjectId,
    pub relative_path: String,
    pub language_id: String,
    pub mtime_ns: i64,
    pub size: i64,
    pub source_hash: String,
}

/// A new file row before it has an id.
#[derive(Debug, Clone)]
pub struct NewFile {
    pub project_id: ProjectId,
    pub relative_path: String,
    pub language_id: String,
    pub mtime_ns: i64,
    pub size: i64,
    pub source_hash: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeUnit {
    pub id: UnitId,
    pub file_id: FileId,
    pub language_id: String,
    pub kind: String,
    pub name: String,
    pub scope: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub body_node_count: usize,
    pub source_hash: String,
    pub normalized_body_hash: String,
    pub display_source: Option<String>,
    pub embedding_text: Option<String>,
}

/// A new code unit before it has an id.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCodeUnit {
    pub language_id: String,
    pub kind: String,
    pub name: String,
    pub scope: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub body_node_count: usize,
    pub source_hash: String,
    pub normalized_body_hash: String,
    pub display_source: Option<String>,
    pub embedding_text: Option<String>,
}

/// Everything that identifies an embedding model for reproducible runs.
/// Two runs with the same identity produce comparable vectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelIdentity {
    pub backend: String,
    pub backend_version: String,
    /// ONNX Runtime / `ort` version when applicable.
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

#[derive(Debug, Clone)]
pub struct EmbeddingModelRecord {
    pub id: ModelId,
    pub identity: ModelIdentity,
}

/// Encode a vector as a little-endian f32 blob.
pub fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        blob.extend_from_slice(&value.to_le_bytes());
    }
    blob
}

/// Decode a little-endian f32 blob back into a vector.
pub fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_blob_roundtrip() {
        let vector = vec![0.25_f32, -1.5, 3.75, f32::MIN_POSITIVE];
        assert_eq!(blob_to_vector(&vector_to_blob(&vector)), vector);
    }
}
