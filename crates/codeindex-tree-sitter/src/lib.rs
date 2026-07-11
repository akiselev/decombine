#![forbid(unsafe_code)]

pub use codeindex_core;

pub mod language;
mod extractor;
pub mod normalizer;

pub use extractor::{ExtractOptions, extract_file, extract_units};
pub use language::{LanguageDef, LanguageRegistry, LanguageSpec, ScopeRule};

#[cfg(test)]
pub(crate) mod config {
    pub const KNOWN_LANGUAGE_IDS: &[&str] = &[
        "c", "cpp", "csharp", "go", "java", "javascript", "kotlin", "php",
        "python", "ruby", "rust", "typescript",
    ];
}