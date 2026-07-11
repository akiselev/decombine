use anyhow::Result;

use crate::config::{Config, RetentionMode};
use crate::db::Db;

pub use codeindex_indexer::ProjectStats;

pub fn index(db: &Db, config: &Config, only_label: Option<&str>) -> Result<Vec<ProjectStats>> {
    let retention = match config.index.retention {
        RetentionMode::Full => codeindex_indexer::RetentionMode::Full,
        RetentionMode::Report => codeindex_indexer::RetentionMode::Report,
        RetentionMode::Minimal => codeindex_indexer::RetentionMode::Minimal,
    };
    let options = codeindex_indexer::IndexOptions {
        projects: config
            .resolved_projects()
            .into_iter()
            .map(|project| codeindex_indexer::ProjectSpec {
                label: project.label,
                source_dir: project.source_dir,
                exclude: project.exclude,
            })
            .collect(),
        enabled_languages: config.languages.enabled.clone(),
        body_node_count_threshold: config.analysis.body_node_count_threshold,
        max_body_chars: config.embedding.max_body_chars,
        retention,
    };
    codeindex_indexer::index(db, &options, only_label)
}
