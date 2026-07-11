#![forbid(unsafe_code)]

pub mod config;

pub mod db {
    pub use codeindex_sqlite::*;
}

pub mod index {
    pub mod language {
        pub use codeindex_tree_sitter::{LanguageDef, LanguageRegistry};
    }

    pub mod extractor {
        use anyhow::Result;
        use codeindex_core::{ExtractedEntity, RepresentationKind};
        use codeindex_sqlite::NewCodeUnit;
        use codeindex_tree_sitter::LanguageDef;

        pub use codeindex_tree_sitter::ExtractOptions;

        pub fn extract_units(
            def: &LanguageDef,
            source: &str,
            options: &ExtractOptions,
        ) -> Result<Vec<NewCodeUnit>> {
            codeindex_tree_sitter::extract_units(def, source, options)
                .map(|entities| entities.into_iter().map(to_new_unit).collect())
        }

        fn to_new_unit(entity: ExtractedEntity) -> NewCodeUnit {
            let display_source = entity
                .representation_text(&RepresentationKind::FullSource)
                .map(str::to_owned);
            let embedding_text = entity
                .representation_text(&RepresentationKind::Implementation)
                .map(str::to_owned);
            NewCodeUnit {
                language_id: entity.language.into_inner(),
                kind: entity.kind.as_str().to_owned(),
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
        }
    }
}

pub mod embed;
pub use embed::*;
