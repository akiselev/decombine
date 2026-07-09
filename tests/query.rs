//! Agent query interface acceptance tests: stable IDs and JSON envelopes for
//! the analyzer serializers, plus the `decombine query` CLI surface run
//! end-to-end over a real indexed database (with fixture embeddings; no
//! model download).

use anyhow::Result;
use assert_cmd::Command;
use decombine::analyze::compare::CompareAnalyzer;
use decombine::analyze::duplicate::DuplicateAnalyzer;
use decombine::analyze::vector_store::VectorStore;
use decombine::analyze::{AnalysisContext, Analyzer, CodeUnitRef};
use decombine::config::{AnalysisConfig, ComparisonConfig, RetentionMode};
use decombine::db::{ModelIdentity, Project};
use decombine::query::unit_id;
use decombine::report::markdown::ReportMeta;
use serde_json::Value;

fn identity() -> ModelIdentity {
    ModelIdentity {
        backend: "test".into(),
        backend_version: "1.0".into(),
        runtime_version: None,
        model: "fixture".into(),
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

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    v
}

fn unit(project: &str, path: &str, name: &str, hash: &str, start_line: usize) -> CodeUnitRef {
    CodeUnitRef {
        id: 0,
        project_label: project.into(),
        relative_path: path.into(),
        language_id: "rust".into(),
        kind: "function".into(),
        name: name.into(),
        scope: None,
        start_byte: start_line * 100,
        end_byte: start_line * 100 + 80,
        start_line,
        end_line: start_line + 8,
        body_node_count: 20,
        normalized_body_hash: hash.into(),
        display_source: Some(format!("fn {name}() {{ /* fixture */ }}")),
    }
}

fn ctx(
    units: Vec<CodeUnitRef>,
    vectors: Vec<Option<Vec<f32>>>,
    labels: &[&str],
) -> AnalysisContext {
    let projects = labels
        .iter()
        .enumerate()
        .map(|(i, label)| Project {
            id: i as i64 + 1,
            label: label.to_string(),
            source_dir: format!("/{label}"),
            role: None,
        })
        .collect();
    AnalysisContext {
        model_id: 1,
        identity: identity(),
        projects,
        vectors: VectorStore::from_unit_vectors(4, vectors),
        units,
    }
}

fn meta(projects: &[(&str, &str)]) -> ReportMeta {
    ReportMeta {
        identity: identity(),
        analysis: AnalysisConfig::default(),
        retention: RetentionMode::Report,
        timestamp: "2026-01-01 00:00:00Z".into(),
        projects: projects
            .iter()
            .map(|(l, r)| (l.to_string(), r.to_string()))
            .collect(),
        ignore_file: ".decombine-ignore".into(),
    }
}

// ---------- analyzer JSON serializers ----------

#[test]
fn duplicate_json_envelope_ids_and_limit() {
    let units = vec![
        unit("main", "src/a.rs", "alpha", "h1", 1),
        unit("main", "src/b.rs", "alpha_copy", "h1", 1),
        unit("main", "src/c.rs", "beta", "h2", 20),
        unit("main", "src/d.rs", "beta_near", "h3", 40),
    ];
    let e1 = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    let e2 = normalize(vec![0.0, 1.0, 0.05, 0.0]);
    let e3 = normalize(vec![0.0, 1.0, 0.0, 0.05]);
    let context = ctx(
        units,
        vec![Some(e1.clone()), Some(e1), Some(e2), Some(e3)],
        &["main"],
    );
    let analyzer = DuplicateAnalyzer {
        ignored_hashes: Default::default(),
    };
    let config: AnalysisConfig = serde_yaml::from_str(
        "candidate_threshold: 0.88\nsimilarity_threshold: 0.92\nrerank_threshold: 0.94\n",
    )
    .unwrap();
    let report = analyzer.run(&context, &config).unwrap();
    assert_eq!(report.clusters.len(), 2);

    let value = decombine::report::json::duplicate_json(
        &meta(&[("main", "/main")]),
        &context,
        &report,
        None,
    );
    assert_eq!(value["schema_version"], "decombine.duplicates.v1");
    assert_eq!(value["kind"], "duplicate_report");
    assert_eq!(value["summary"]["matched"], 2);
    assert_eq!(value["summary"]["exhaustive"], true);
    let first = &value["items"][0];
    assert!(
        first["cluster_id"]
            .as_str()
            .unwrap()
            .starts_with("cluster:")
    );
    assert!(
        first["members"][0]["unit_id"]
            .as_str()
            .unwrap()
            .starts_with("unit:")
    );

    // Same analysis twice → identical JSON (deterministic contract).
    let again = decombine::report::json::duplicate_json(
        &meta(&[("main", "/main")]),
        &context,
        &report,
        None,
    );
    assert_eq!(value, again);

    let limited = decombine::report::json::duplicate_json(
        &meta(&[("main", "/main")]),
        &context,
        &report,
        Some(1),
    );
    assert_eq!(limited["summary"]["returned"], 1);
    assert_eq!(limited["summary"]["has_more"], true);
    assert_eq!(limited["items"].as_array().unwrap().len(), 1);
}

#[test]
fn comparison_json_envelope_and_match_ids() {
    let units = vec![
        unit("old", "src/a.rs", "alpha", "h1", 1),
        unit("new", "src/a.rs", "alpha", "h1", 1),
        unit("old", "src/b.rs", "beta", "h2", 20),
        unit("new", "src/b.rs", "beta_rewrite", "h3", 20),
    ];
    let e1 = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    let e2 = normalize(vec![0.0, 1.0, 0.0, 0.0]);
    let e3 = normalize(vec![0.0, 1.0, 0.1, 0.0]);
    let context = ctx(
        units,
        vec![Some(e1.clone()), Some(e1), Some(e2), Some(e3)],
        &["new", "old"],
    );
    let analyzer = CompareAnalyzer {
        left_label: "old".into(),
        right_label: "new".into(),
    };
    let config: ComparisonConfig =
        serde_yaml::from_str("left: old\nright: new\ncalibration: none\n").unwrap();
    let report = analyzer.run(&context, &config).unwrap();

    let value = decombine::report::json::comparison_json(
        &meta(&[("old", "/old"), ("new", "/new")]),
        &context,
        &report,
        None,
    );
    assert_eq!(value["schema_version"], "decombine.compare.v1");
    assert_eq!(value["left"], "old");
    assert_eq!(value["right"], "new");
    assert_eq!(value["summary"]["exact_copy"], 1);
    let items = value["items"].as_array().unwrap();
    assert_eq!(items.len(), report.matches.len());
    for item in items {
        assert!(item["match_id"].as_str().unwrap().starts_with("match:"));
    }
    assert_eq!(items[0]["class"], "exact_copy");
    assert_eq!(items[0]["left"][0]["project"], "old");
}

// ---------- CLI end-to-end ----------

const FN_ALPHA: &str = "fn alpha(values: Vec<i64>) -> i64 {\n    let mut total = 0;\n    for value in values {\n        if value > 0 {\n            total += value;\n        }\n    }\n    total\n}\n";
const FN_BETA: &str = "fn beta(names: Vec<String>) -> usize {\n    let mut count = 0;\n    for name in names {\n        if !name.is_empty() {\n            count += 1;\n        }\n    }\n    count\n}\n";

fn cli(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("decombine").unwrap();
    cmd.current_dir(dir);
    cmd
}

fn json_stdout(cmd: &mut Command) -> Value {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is valid JSON")
}

/// Temp project with two Rust functions, indexed via the real CLI.
fn indexed_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src-a/src")).unwrap();
    std::fs::write(
        dir.path().join("src-a/src/lib.rs"),
        format!("{FN_ALPHA}\n{FN_BETA}"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("decombine.yaml"),
        "source_dir: src-a\nanalysis:\n  body_node_count_threshold: 5\n",
    )
    .unwrap();
    cli(dir.path()).arg("index").assert().success();
    dir
}

/// Attach fixture embeddings (one distinct vector per body hash) so vector
/// queries work without a model download.
fn embed_fixture(dir: &std::path::Path) -> Result<()> {
    let config = decombine::config::Config::load(&dir.join("decombine.yaml"))?;
    let db = decombine::db::open_or_create(&config.db_file)?;
    let model = db.find_or_create_model(&identity())?;
    let mut stmt = db
        .conn()
        .prepare("SELECT DISTINCT normalized_body_hash FROM code_units ORDER BY 1")?;
    let hashes: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .map(|row| row.unwrap())
        .collect();
    for (i, hash) in hashes.iter().enumerate() {
        let mut v = vec![0.05_f32; 4];
        v[i % 4] = 1.0;
        db.insert_embedding(model, hash, &normalize(v))?;
    }
    Ok(())
}

#[test]
fn query_cli_units_inspect_capabilities_similar() -> Result<()> {
    let dir = indexed_fixture();

    // units --json: envelope with stable IDs.
    let value = json_stdout(cli(dir.path()).args(["query", "units", "--json"]));
    assert_eq!(value["schema_version"], "decombine.query.v1");
    let items = value["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let alpha_id = items
        .iter()
        .find(|i| i["name"] == "alpha")
        .and_then(|i| i["unit_id"].as_str())
        .unwrap()
        .to_string();
    assert!(alpha_id.starts_with("unit:"));

    // The printed ID matches the library's ID for the same unit fields.
    let config = decombine::config::Config::load(&dir.path().join("decombine.yaml"))?;
    let db = decombine::db::open_or_create(&config.db_file)?;
    let (_, all_units) = decombine::analyze::context::load_projects_and_units(&db, &[])?;
    let alpha = all_units.iter().find(|u| u.name == "alpha").unwrap();
    assert_eq!(unit_id(alpha), alpha_id);

    // --where filters and --limit bounding are honest.
    let filtered = json_stdout(cli(dir.path()).args([
        "query",
        "units",
        "--where",
        "name=alpha path=src/**",
        "--json",
    ]));
    assert_eq!(filtered["summary"]["matched"], 1);
    let limited = json_stdout(cli(dir.path()).args(["query", "units", "--limit", "1", "--json"]));
    assert_eq!(limited["summary"]["has_more"], true);
    assert_eq!(limited["summary"]["exhaustive"], false);

    // inspect --source recovers text (report retention keeps display source).
    let inspected =
        json_stdout(cli(dir.path()).args(["query", "inspect", &alpha_id, "--source", "--json"]));
    assert_eq!(inspected["items"][0]["unit_id"], alpha_id.as_str());
    assert!(
        inspected["items"][0]["source"]
            .as_str()
            .unwrap()
            .contains("fn alpha")
    );

    // Unknown selectors fail with re-resolution guidance.
    cli(dir.path())
        .args(["query", "inspect", "unit:0000000000000000"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not found in the current index"));

    // capabilities before embeddings: vector queries reported unavailable.
    let caps = json_stdout(cli(dir.path()).args(["query", "capabilities", "--json"]));
    assert_eq!(caps["schema_version"], "decombine.capabilities.v1");
    assert_eq!(caps["projects"][0]["units"], 2);
    assert_eq!(caps["model"], Value::Null);
    assert_eq!(caps["query_commands"]["similar"], false);

    // similar without embeddings is a clear state error.
    cli(dir.path())
        .args(["query", "similar", "--unit", &alpha_id])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no embeddings"));

    // With fixture embeddings, similar/qbe return scored neighbors.
    embed_fixture(dir.path())?;
    let similar = json_stdout(
        cli(dir.path()).args(["query", "similar", "--unit", &alpha_id, "--why", "--json"]),
    );
    assert_eq!(similar["summary"]["matched"], 1);
    let neighbor = &similar["items"][0];
    assert_eq!(neighbor["name"], "beta");
    assert!(neighbor["score"].as_f64().unwrap() < 1.0);
    assert_eq!(neighbor["why"]["decision"]["reason"], "vector_rank");
    let qbe = json_stdout(cli(dir.path()).args([
        "query",
        "qbe",
        "--unit",
        &alpha_id,
        "--neighbors",
        "1",
        "--json",
    ]));
    assert_eq!(qbe["query"]["mode"], "qbe");
    assert_eq!(qbe["items"].as_array().unwrap().len(), 1);

    let caps = json_stdout(cli(dir.path()).args(["query", "capabilities", "--json"]));
    assert_eq!(caps["query_commands"]["similar"], true);
    assert_eq!(caps["embeddings"]["pending_bodies"], 0);
    Ok(())
}

#[test]
fn analyze_and_compare_emit_json_on_stdout() -> Result<()> {
    let dir = indexed_fixture();
    embed_fixture(dir.path())?;
    let value = json_stdout(cli(dir.path()).args(["analyze", "duplicates", "--json"]));
    assert_eq!(value["schema_version"], "decombine.duplicates.v1");
    assert!(value["summary"]["matched"].is_number());
    // No markdown report directory is written in JSON mode.
    assert!(!dir.path().join("decombine-report").exists());
    Ok(())
}
