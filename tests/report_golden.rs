//! Markdown report golden tests (Phase 7). Regenerate snapshots with
//! `UPDATE_GOLDEN=1 cargo test --test report_golden`.

use std::path::Path;

use anyhow::Result;
use decombine::analyze::compare::CompareAnalyzer;
use decombine::analyze::concerns::ConcernAnalyzer;
use decombine::analyze::duplicate::DuplicateAnalyzer;
use decombine::analyze::vector_store::VectorStore;
use decombine::analyze::{AnalysisContext, Analyzer, CodeUnitRef};
use decombine::config::{AnalysisConfig, ConcernQuery, ConcernsConfig, RetentionMode};
use decombine::db::{ModelIdentity, Project};
use decombine::embed::Embedder;
use decombine::report;

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

fn unit(project: &str, path: &str, name: &str, scope: Option<&str>, hash: &str) -> CodeUnitRef {
    CodeUnitRef {
        id: 0,
        project_label: project.into(),
        relative_path: path.into(),
        language_id: "rust".into(),
        kind: "function".into(),
        name: name.into(),
        scope: scope.map(str::to_string),
        start_byte: 0,
        end_byte: 80,
        start_line: 1,
        end_line: 9,
        body_node_count: 20,
        normalized_body_hash: hash.into(),
        display_source: Some(format!(
            "fn {name}(input: u32) -> u32 {{\n    let doubled = input * 2;\n    doubled + 1\n}}"
        )),
    }
}

fn ctx(
    units: Vec<CodeUnitRef>,
    vectors: Vec<Option<Vec<f32>>>,
    labels: &[&str],
) -> AnalysisContext {
    AnalysisContext {
        model_id: 1,
        identity: identity(),
        projects: labels
            .iter()
            .enumerate()
            .map(|(i, label)| Project {
                id: i as i64 + 1,
                label: label.to_string(),
                source_dir: format!("/repo/{label}"),
                role: None,
            })
            .collect(),
        vectors: VectorStore::from_unit_vectors(4, vectors),
        units,
    }
}

fn meta(ctx: &AnalysisContext, retention: RetentionMode) -> report::ReportMeta {
    report::ReportMeta {
        identity: ctx.identity.clone(),
        analysis: analysis_config(),
        retention,
        timestamp: "2026-07-03 00:00:00Z".into(),
        projects: ctx
            .projects
            .iter()
            .map(|p| (p.label.clone(), p.source_dir.clone()))
            .collect(),
        ignore_file: ".decombineignore".into(),
    }
}

fn analysis_config() -> AnalysisConfig {
    // Pinned to the historical BGE-scale duplicate thresholds so these golden
    // fixtures stay stable independent of the product default (now CodeRank
    // scale). The hash backend's geometry is model-agnostic; these numbers just
    // fix the classifier the fixtures were generated against.
    serde_yaml::from_str(
        "candidate_threshold: 0.88\nsimilarity_threshold: 0.92\nrerank_threshold: 0.94\n",
    )
    .unwrap()
}

fn assert_matches_golden(dir: &Path, files: &[&str], golden_subdir: &str) {
    let golden_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/report")
        .join(golden_subdir);
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    for file in files {
        let actual = std::fs::read_to_string(dir.join(file))
            .unwrap_or_else(|_| panic!("report did not produce {file}"));
        let golden_path = golden_root.join(file);
        if update {
            std::fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
            std::fs::write(&golden_path, &actual).unwrap();
        }
        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
            panic!("missing golden {} (UPDATE_GOLDEN=1)", golden_path.display())
        });
        assert_eq!(actual, expected, "golden mismatch for {file}");
    }
}

fn duplicate_context() -> AnalysisContext {
    let dup = normalize(vec![1.0, 0.02, 0.0, 0.0]);
    let near = normalize(vec![1.0, 0.09, 0.0, 0.0]);
    let units = vec![
        unit(
            "main",
            "billing/invoice.rs",
            "send_total",
            Some("Invoice"),
            "hash-dup",
        ),
        unit(
            "main",
            "shipping/label.rs",
            "send_total",
            Some("Label"),
            "hash-dup",
        ),
        unit(
            "main",
            "reports/summary.rs",
            "send_summary",
            None,
            "hash-near",
        ),
    ];
    let vectors = vec![Some(dup.clone()), Some(dup), Some(near)];
    ctx(units, vectors, &["main"])
}

#[test]
fn duplicate_report_golden() -> Result<()> {
    let context = duplicate_context();
    let output = DuplicateAnalyzer {
        ignored_hashes: Default::default(),
    }
    .run(&context, &analysis_config())?;
    assert_eq!(output.clusters.len(), 1);

    let dir = tempfile::tempdir()?;
    report::write_duplicate_report(
        dir.path(),
        &meta(&context, RetentionMode::Report),
        &context,
        &output,
    )?;
    assert_matches_golden(dir.path(), &["index.md", "cluster-01.md"], "duplicates");

    // Exact copies appear once, with both locations listed.
    let cluster = std::fs::read_to_string(dir.path().join("cluster-01.md"))?;
    assert_eq!(cluster.matches("fn send_total").count(), 1);
    assert!(cluster.contains("billing/invoice.rs"));
    assert!(cluster.contains("shipping/label.rs"));
    assert!(cluster.contains("```rust"), "language-tagged fences");
    Ok(())
}

#[test]
fn ignored_cluster_disappears_and_reappears_on_change() -> Result<()> {
    let context = duplicate_context();
    let base = DuplicateAnalyzer {
        ignored_hashes: Default::default(),
    }
    .run(&context, &analysis_config())?;
    let hash = base.clusters[0].hash.clone();

    // Reviewed: hash in the ignore set suppresses the cluster.
    let ignored = DuplicateAnalyzer {
        ignored_hashes: [hash.clone()].into_iter().collect(),
    }
    .run(&context, &analysis_config())?;
    assert!(ignored.clusters.is_empty());

    // The body changes: the stable hash changes, the cluster reappears.
    let mut changed = duplicate_context();
    for unit in &mut changed.units {
        if unit.normalized_body_hash == "hash-dup" {
            unit.normalized_body_hash = "hash-dup-v2".into();
        }
    }
    let reappeared = DuplicateAnalyzer {
        ignored_hashes: [hash.clone()].into_iter().collect(),
    }
    .run(&changed, &analysis_config())?;
    assert_eq!(reappeared.clusters.len(), 1);
    assert_ne!(reappeared.clusters[0].hash, hash);

    // A path change alone also invalidates the reviewed hash.
    let mut moved = duplicate_context();
    moved.units[0].relative_path = "billing/moved.rs".into();
    let reappeared = DuplicateAnalyzer {
        ignored_hashes: [hash].into_iter().collect(),
    }
    .run(&moved, &analysis_config())?;
    assert_eq!(reappeared.clusters.len(), 1);
    Ok(())
}

#[test]
fn retention_modes_control_report_source() -> Result<()> {
    // minimal retention: no stored source; the report rereads the file.
    let dir = tempfile::tempdir()?;
    let source_dir = dir.path().join("src");
    std::fs::create_dir_all(&source_dir)?;
    let body = "fn from_disk(x: u32) -> u32 {\n    x + 41\n}\n";
    std::fs::write(source_dir.join("live.rs"), body)?;

    let dup = normalize(vec![1.0, 0.02, 0.0, 0.0]);
    let mut a = unit("main", "live.rs", "from_disk", None, "h1");
    a.display_source = None;
    a.end_byte = body.len() - 1;
    let mut b = unit("main", "gone.rs", "vanished", None, "h2");
    b.display_source = None;
    let mut context = ctx(vec![a, b], vec![Some(dup.clone()), Some(dup)], &["main"]);
    context.projects[0].source_dir = source_dir.display().to_string();

    let output = DuplicateAnalyzer {
        ignored_hashes: Default::default(),
    }
    .run(&context, &analysis_config())?;
    let report_dir = dir.path().join("report");
    let mut report_meta = meta(&context, RetentionMode::Minimal);
    report_meta.projects = vec![("main".into(), source_dir.display().to_string())];
    report::write_duplicate_report(&report_dir, &report_meta, &context, &output)?;

    let page = std::fs::read_to_string(report_dir.join("cluster-01.md"))?;
    assert!(page.contains("x + 41"), "reread from disk:\n{page}");
    assert!(
        page.contains("source unavailable"),
        "missing file degrades gracefully:\n{page}"
    );

    // full/report retention uses the stored display source.
    let context = duplicate_context();
    let output = DuplicateAnalyzer {
        ignored_hashes: Default::default(),
    }
    .run(&context, &analysis_config())?;
    let full_dir = dir.path().join("report-full");
    report::write_duplicate_report(
        &full_dir,
        &meta(&context, RetentionMode::Full),
        &context,
        &output,
    )?;
    let page = std::fs::read_to_string(full_dir.join("cluster-01.md"))?;
    assert!(page.contains("let doubled = input * 2;"));
    Ok(())
}

struct FixtureEmbedder(ModelIdentity);

impl Embedder for FixtureEmbedder {
    fn identity(&self) -> &ModelIdentity {
        &self.0
    }
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(inputs.iter().map(|_| vec![1.0, 0.0, 0.0, 0.0]).collect())
    }
}

#[test]
fn concern_report_golden() -> Result<()> {
    let high = normalize(vec![1.0, 0.05, 0.0, 0.0]);
    let low = normalize(vec![1.0, 0.9, 0.0, 0.0]);
    let units = vec![
        unit("main", "api/retry.rs", "retry_call", None, "h1"),
        unit("main", "worker/backoff.rs", "with_backoff", None, "h2"),
    ];
    let context = ctx(units, vec![Some(high), Some(low)], &["main"]);
    let config = ConcernsConfig {
        enabled: true,
        min_projection: 0.45,
        top_units_per_concern: 10,
        queries: vec![ConcernQuery {
            name: "error-handling".into(),
            query: "error handling and retries".into(),
        }],
    };
    let mut embedder = FixtureEmbedder(identity());
    let output = ConcernAnalyzer {
        embedder: &mut embedder,
    }
    .run_mut(&context, &config)?;

    let dir = tempfile::tempdir()?;
    report::write_concern_report(
        dir.path(),
        &meta(&context, RetentionMode::Report),
        &context,
        &output,
    )?;
    assert_matches_golden(
        dir.path(),
        &["concerns/index.md", "concerns/error-handling.md"],
        "concerns",
    );

    // Candidate wording, projection ordering, and spread must be present.
    let page = std::fs::read_to_string(dir.path().join("concerns/error-handling.md"))?;
    assert!(page.contains("Candidate concern"));
    let retry = page.find("retry_call").unwrap();
    let backoff = page.find("with_backoff").unwrap();
    assert!(retry < backoff, "higher projection listed first");
    Ok(())
}

#[test]
fn comparison_report_golden() -> Result<()> {
    let strong = normalize(vec![1.0, 0.1, 0.0, 0.0]);
    let strong_match = normalize(vec![1.0, 0.15, 0.05, 0.0]);
    let lonely = normalize(vec![0.0, 0.0, 0.0, 1.0]);
    let fresh = normalize(vec![0.0, 1.0, 0.0, 0.0]);
    let units = vec![
        unit("v1", "src/exact.rs", "same", None, "hash-exact"),
        unit("v1", "src/api.rs", "handle", None, "h-l1"),
        unit("v1", "src/gone.rs", "legacy", None, "h-l2"),
        unit("v2", "lib/exact.rs", "same", None, "hash-exact"),
        unit("v2", "lib/api.rs", "handle_req", None, "h-r1"),
        unit("v2", "lib/fresh.rs", "brand_new", None, "h-r2"),
    ];
    let vectors = vec![
        Some(normalize(vec![0.4, 0.4, 0.4, 0.4])),
        Some(strong),
        Some(lonely),
        Some(normalize(vec![0.4, 0.4, 0.4, 0.4])),
        Some(strong_match),
        Some(fresh),
    ];
    let context = ctx(units, vectors, &["v1", "v2"]);
    let output = CompareAnalyzer {
        left_label: "v1".into(),
        right_label: "v2".into(),
    }
    .run(&context, &serde_yaml::from_str("{}").unwrap())?;

    let dir = tempfile::tempdir()?;
    report::write_comparison_report(
        dir.path(),
        &meta(&context, RetentionMode::Report),
        &context,
        &output,
    )?;
    assert_matches_golden(
        dir.path(),
        &[
            "compare/index.md",
            "compare/exact_copy.md",
            "compare/strong_match.md",
            "compare/missing_in_right.md",
            "compare/new_in_right.md",
        ],
        "compare",
    );
    Ok(())
}
