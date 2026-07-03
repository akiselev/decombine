//! Embedding pipeline acceptance tests (Phase 5), using the deterministic
//! hash backend so no model download is needed.

use decombine::config::Config;
use decombine::db::{Db, open_or_create};
use decombine::embed::hash::HashEmbedder;
use decombine::embed::{Embedder, embed_pending};
use decombine::index::indexer;

const FN_A: &str = "fn alpha(values: Vec<i64>) -> i64 {\n    let mut total = 0;\n    for value in values {\n        if value > 0 {\n            total += value;\n        }\n    }\n    total\n}\n";
const FN_B: &str = "fn beta(names: Vec<String>) -> usize {\n    let mut count = 0;\n    for name in names {\n        if !name.is_empty() {\n            count += 1;\n        }\n    }\n    count\n}\n";

struct Fixture {
    _dir: tempfile::TempDir,
    config: Config,
    db: Db,
}

fn fixture(retention: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let config_path = dir.path().join("decombine.yaml");
    std::fs::write(
        &config_path,
        format!(
            "source_dir: src\nindex:\n  retention: {retention}\nanalysis:\n  body_node_count_threshold: 5\n"
        ),
    )
    .unwrap();
    let config = Config::load(&config_path).unwrap();
    let db = open_or_create(&config.db_file).unwrap();
    Fixture {
        _dir: dir,
        config,
        db,
    }
}

fn write(f: &Fixture, rel: &str, content: &str) {
    let path = f.config.source_dir.as_deref().unwrap().join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn identical_bodies_embed_once() {
    let f = fixture("full");
    write(&f, "one.rs", FN_A);
    write(&f, "two.rs", FN_A); // exact copy: same normalized hash
    write(&f, "three.rs", FN_B);
    indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(f.db.count_units().unwrap(), 3);

    let mut embedder = HashEmbedder::new(16);
    let stats = embed_pending(&f.db, &mut embedder, &f.config).unwrap();
    assert_eq!(stats.embedded, 2, "two distinct bodies, three units");
    assert_eq!(stats.unresolved, 0);
}

#[test]
fn resume_skips_already_embedded_hashes() {
    let f = fixture("full");
    write(&f, "one.rs", FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();

    let mut embedder = HashEmbedder::new(16);
    assert_eq!(
        embed_pending(&f.db, &mut embedder, &f.config)
            .unwrap()
            .embedded,
        1
    );
    assert_eq!(
        embed_pending(&f.db, &mut embedder, &f.config)
            .unwrap()
            .embedded,
        0
    );

    write(&f, "two.rs", FN_B);
    indexer::index(&f.db, &f.config, None).unwrap();
    assert_eq!(
        embed_pending(&f.db, &mut embedder, &f.config)
            .unwrap()
            .embedded,
        1
    );
}

#[test]
fn model_mismatch_is_a_clear_error() {
    let f = fixture("full");
    write(&f, "one.rs", FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();

    embed_pending(&f.db, &mut HashEmbedder::new(16), &f.config).unwrap();
    let error = embed_pending(&f.db, &mut HashEmbedder::new(32), &f.config).unwrap_err();
    assert!(
        error.to_string().contains("embedding.model"),
        "unexpected error: {error}"
    );
}

#[test]
fn report_retention_rederives_text_from_source() {
    let f = fixture("report");
    write(&f, "one.rs", FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();

    // No stored embedding text in report mode...
    let project = f.db.get_project("main").unwrap().unwrap();
    let file = f.db.get_file(project.id, "one.rs").unwrap().unwrap();
    let units = f.db.list_units_for_file(file.id).unwrap();
    assert!(units[0].embedding_text.is_none());

    // ...but embedding still works by re-reading the source file.
    let mut embedder = HashEmbedder::new(16);
    let stats = embed_pending(&f.db, &mut embedder, &f.config).unwrap();
    assert_eq!(stats.embedded, 1);
    assert_eq!(stats.unresolved, 0);

    let model = f.db.find_or_create_model(embedder.identity()).unwrap();
    let vector =
        f.db.get_embedding(model, &units[0].normalized_body_hash)
            .unwrap()
            .unwrap();
    assert_eq!(vector.len(), 16);
    // Stored vectors are normalized.
    let norm: f32 = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5);
}

#[test]
fn stale_source_in_report_mode_counts_unresolved() {
    let f = fixture("report");
    write(&f, "one.rs", FN_A);
    indexer::index(&f.db, &f.config, None).unwrap();
    // The file changes after indexing but before embedding, without reindex.
    write(&f, "one.rs", FN_B);

    let stats = embed_pending(&f.db, &mut HashEmbedder::new(16), &f.config).unwrap();
    assert_eq!(stats.embedded, 0);
    assert_eq!(stats.unresolved, 1);
}
