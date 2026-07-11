#![forbid(unsafe_code)]

pub use codeindex_core;

// Keep the mature bundled-language implementation byte-for-byte shared during
// the extraction. The decombine package no longer compiles this module itself;
// this crate is now its owner. A later cleanup can physically relocate the file
// without changing the public API.
#[path = "../../../src/index/language.rs"]
pub mod language;

mod extractor;
pub mod normalizer;

pub use extractor::{ExtractOptions, extract_file, extract_units};
pub use language::{LanguageDef, LanguageRegistry, LanguageSpec, ScopeRule};

// The existing language module's self-test checks the known bundled IDs through
// `crate::config`. Keep that assertion local to this frontend crate.
#[cfg(test)]
pub(crate) mod config {
    pub const KNOWN_LANGUAGE_IDS: &[&str] = &[
        "c", "cpp", "csharp", "go", "java", "javascript", "kotlin", "php",
        "python", "ruby", "rust", "typescript",
    ];
}
