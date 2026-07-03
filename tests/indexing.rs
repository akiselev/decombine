//! Incremental indexing acceptance tests (Phase 4).

use std::path::Path;

use decombine::config::Config;
use decombine::db::{Db, ModelIdentity, open_or_create};
use decombine::index::indexer;

const BIG_FN_A: &str = "fn alpha(values: Vec<i64>) -> i64 {\n    let mut total = 0;\n    for value in values {\n        if value > 0 {\n            total += value;\n        }\n    }\n    total\n}\n";
const BIG_FN_B: &str = "fn beta(names: Vec<String>) -> usize {\n    let mut count = 0;\n    for name in names {\n        if !name.is_empty() {\n            count += 1;\n        }\n    }\n    count\n}\n";

struct Fixture {
    _dir: tempfile::TempDir,
    config: Config,
    db: Db,
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Build a config + database over one or two temp source trees.
fn fixture(config_yaml: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src-a")).unwrap();
    std::fs::create_dir_all(dir.path().join("src-b")).unwrap();
    let config_path = dir.path().join("decombine.yaml");
    std::fs::write(&config_path, config_yaml).unwrap();
    let config = Config::load(&config_path).unwrap();
    let db = open_or_create(&config.db_file).unwrap();
    Fixture {
        _dir: dir,
        config,
        db,
    }
}

fn single_project() -> Fixture {
    fixture("source_dir: src-a\nanalysis:\n  body_node_count_threshold: 5\n")
}

fn root(fixture: &Fixture) -> &Path {
    fixture.config.source_dir.as_deref().unwrap()
}

fn test_identity() -> ModelIdentity {
    ModelIdentity {
        backend: "test".into(),
        backend_version: "0".into(),
        runtime_version: None,
        model: "test".into(),
        revision: None,
        dimensions: 4,
        tokenizer_hash: None,
        model_hash: None,
        normalize: true,
        execution_provider: "cpu".into(),
        quantization: None,
        cache_path: None,
    }
}

#[test]
fn reindex_unchanged_tree_skips_files() {
    let f = single_project();
    write(root(&f), "a.rs", BIG_FN_A);
    write(root(&f), "b.rs", BIG_FN_B);

    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats[0].indexed, 2);
    assert_eq!(stats[0].skipped, 0);

    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats[0].indexed, 0);
    assert_eq!(stats[0].skipped, 2);
    assert_eq!(stats[0].removed, 0);
}

#[test]
fn modified_file_replaces_units() {
    let f = single_project();
    write(root(&f), "a.rs", BIG_FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();

    let project = f.db.get_project("main").unwrap().unwrap();
    let file = f.db.get_file(project.id, "a.rs").unwrap().unwrap();
    let before = f.db.list_units_for_file(file.id).unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].name, "alpha");

    // Ensure a different mtime even on coarse filesystem clocks.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(root(&f), "a.rs", BIG_FN_B);
    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats[0].indexed, 1);

    let after = f.db.list_units_for_file(file.id).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].name, "beta");
    assert_eq!(f.db.count_units().unwrap(), 1);
}

#[test]
fn deleted_file_removes_units_and_orphan_embeddings() {
    let f = single_project();
    write(root(&f), "a.rs", BIG_FN_A);
    write(root(&f), "b.rs", BIG_FN_B);
    indexer::index(&f.db, &f.config, None).unwrap();

    // Embed both units' hashes with a fake model.
    let model = f.db.find_or_create_model(&test_identity()).unwrap();
    let project = f.db.get_project("main").unwrap().unwrap();
    for file in f.db.list_files(project.id).unwrap() {
        for unit in f.db.list_units_for_file(file.id).unwrap() {
            f.db.insert_embedding(model, &unit.normalized_body_hash, &[1.0, 0.0, 0.0, 0.0])
                .unwrap();
        }
    }
    assert_eq!(f.db.count_embeddings(model).unwrap(), 2);

    std::fs::remove_file(root(&f).join("b.rs")).unwrap();
    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats[0].removed, 1);
    assert_eq!(f.db.count_units().unwrap(), 1);
    assert_eq!(f.db.count_embeddings(model).unwrap(), 1);
}

#[test]
fn two_projects_can_share_relative_paths() {
    let f = fixture(
        "projects:\n  - label: v1\n    source_dir: src-a\n  - label: v2\n    source_dir: src-b\nanalysis:\n  body_node_count_threshold: 5\n",
    );
    let projects = f.config.resolved_projects();
    write(&projects[0].source_dir, "same/mod.rs", BIG_FN_A);
    write(&projects[1].source_dir, "same/mod.rs", BIG_FN_B);

    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats.len(), 2);
    assert_eq!(stats[0].indexed, 1);
    assert_eq!(stats[1].indexed, 1);
    assert_eq!(f.db.count_units().unwrap(), 2);

    // Only reindex one labeled project.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write(&projects[0].source_dir, "extra.rs", BIG_FN_B);
    let stats = indexer::index(&f.db, &f.config, Some("v1")).unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].label, "v1");
    assert_eq!(stats[0].indexed, 1);

    assert!(indexer::index(&f.db, &f.config, Some("nope")).is_err());
}

#[test]
fn exclude_patterns_apply() {
    let f = fixture(
        "source_dir: src-a\nsource_dir_exclude: [\"vendor/**\"]\nanalysis:\n  body_node_count_threshold: 5\n",
    );
    write(root(&f), "keep.rs", BIG_FN_A);
    write(root(&f), "vendor/skip.rs", BIG_FN_B);
    let stats = indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(stats[0].indexed, 1);
    let project = f.db.get_project("main").unwrap().unwrap();
    let files = f.db.list_files(project.id).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].relative_path, "keep.rs");
}

#[test]
fn immutable_settings_enforced_across_runs() {
    let f = single_project();
    write(root(&f), "a.rs", BIG_FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();

    let mut changed = f.config.clone();
    changed.analysis.body_node_count_threshold = 3;
    let error = indexer::index(&f.db, &changed, None).unwrap_err();
    assert!(error.to_string().contains("body_node_count_threshold"));
}

#[test]
fn retention_modes_control_stored_text() {
    for (retention, want_display, want_embed) in [
        ("full", true, true),
        ("report", true, false),
        ("minimal", false, false),
    ] {
        let f = fixture(&format!(
            "source_dir: src-a\nindex:\n  retention: {retention}\nanalysis:\n  body_node_count_threshold: 5\n"
        ));
        write(root(&f), "a.rs", BIG_FN_A);
        indexer::index(&f.db, &f.config, None).unwrap();
        let project = f.db.get_project("main").unwrap().unwrap();
        let file = f.db.get_file(project.id, "a.rs").unwrap().unwrap();
        let units = f.db.list_units_for_file(file.id).unwrap();
        assert_eq!(
            units[0].display_source.is_some(),
            want_display,
            "{retention}"
        );
        assert_eq!(units[0].embedding_text.is_some(), want_embed, "{retention}");
    }
}
