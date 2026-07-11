//! Compatibility shim: the dense vector store and similarity kernels now live
//! in `codeindex-search` (`codeindex_search::vector_store`). Re-exported here
//! so decombine's analyzers keep their `crate::analyze::vector_store::…` paths.

pub use codeindex_search::vector_store::{ScoredPair, VectorStore, dot};
