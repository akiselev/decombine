//! Re-export of the parser-neutral tree-sitter frontend. Extraction returns
//! `ExtractedEntity` values (spans plus representation channels); the indexer
//! persists them, so decombine no longer projects them into a legacy row.

pub use codeindex_core::ExtractedEntity;
pub use codeindex_tree_sitter::{ExtractOptions, extract_units};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::language::LanguageRegistry;
    use codeindex_core::RepresentationKind;

    #[test]
    fn extraction_exposes_implementation_channel() {
        let def = LanguageRegistry::global().get("rust").unwrap();
        let units = extract_units(
            def,
            "fn add(a: i32, b: i32) -> i32 { a + b }",
            &ExtractOptions {
                body_node_count_threshold: 1,
                max_body_chars: 10_000,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "add");
        assert!(
            units[0]
                .representation_text(&RepresentationKind::Implementation)
                .unwrap()
                .contains("a + b")
        );
    }
}
