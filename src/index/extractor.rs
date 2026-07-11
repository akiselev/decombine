use anyhow::Result;

use crate::db::NewCodeUnit;
pub use codeindex_tree_sitter::ExtractOptions;
use codeindex_tree_sitter::{LanguageDef, extract_units as extract_entities};

/// Compatibility adapter from the parser-neutral frontend IR into decombine's
/// existing persistence row. The channel→column projection is shared with the
/// reusable crates via `NewCodeUnit::from`; this adapter only keeps the
/// legacy `extract_units` signature the analyzers were written against.
pub fn extract_units(
    def: &LanguageDef,
    source: &str,
    options: &ExtractOptions,
) -> Result<Vec<NewCodeUnit>> {
    extract_entities(def, source, options)
        .map(|entities| entities.into_iter().map(NewCodeUnit::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::language::LanguageRegistry;

    #[test]
    fn compatibility_adapter_preserves_legacy_fields() {
        let def = LanguageRegistry::global().get("rust").unwrap();
        let units = extract_units(
            def,
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            &ExtractOptions {
                body_node_count_threshold: 1,
                max_body_chars: 10_000,
            },
        )
        .unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].language_id, "rust");
        assert_eq!(units[0].name, "add");
        assert!(
            units[0]
                .embedding_text
                .as_deref()
                .unwrap()
                .contains("a + b")
        );
    }
}
