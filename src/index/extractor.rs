use anyhow::Result;
use codeindex_core::RepresentationKind;

use crate::db::NewCodeUnit;
pub use codeindex_tree_sitter::ExtractOptions;
use codeindex_tree_sitter::{LanguageDef, extract_units as extract_entities};

/// Compatibility adapter from the parser-neutral frontend IR into decombine's
/// existing persistence row. Phase 2 deliberately leaves the database schema
/// unchanged while moving parsing and language ownership into reusable crates.
pub fn extract_units(
    def: &LanguageDef,
    source: &str,
    options: &ExtractOptions,
) -> Result<Vec<NewCodeUnit>> {
    extract_entities(def, source, options).map(|entities| {
        entities
            .into_iter()
            .map(|entity| {
                let display_source = entity
                    .representation_text(&RepresentationKind::FullSource)
                    .map(ToOwned::to_owned);
                let embedding_text = entity
                    .representation_text(&RepresentationKind::Implementation)
                    .map(ToOwned::to_owned);
                NewCodeUnit {
                    language_id: entity.language.into_inner(),
                    kind: entity.kind.as_str().to_string(),
                    name: entity.name,
                    scope: entity.scope,
                    start_byte: entity.span.start_byte,
                    end_byte: entity.span.end_byte,
                    start_line: entity.span.start_line,
                    end_line: entity.span.end_line,
                    body_node_count: entity.body_node_count,
                    source_hash: entity.source_hash,
                    normalized_body_hash: entity.normalized_body_hash,
                    display_source,
                    embedding_text,
                }
            })
            .collect()
    })
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
            &ExtractOptions { body_node_count_threshold: 1, max_body_chars: 10_000 },
        )
        .unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].language_id, "rust");
        assert_eq!(units[0].name, "add");
        assert!(units[0].embedding_text.as_deref().unwrap().contains("a + b"));
    }
}