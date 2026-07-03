//! Analysis acceptance tests (Phases 6, 6b, 6c) over synthetic contexts
//! with deterministic fixture vectors, plus database-backed context tests.

use anyhow::Result;
use decombine::analyze::compare::{CompareAnalyzer, MatchClass};
use decombine::analyze::concerns::ConcernAnalyzer;
use decombine::analyze::duplicate::DuplicateAnalyzer;
use decombine::analyze::vector_store::VectorStore;
use decombine::analyze::{AnalysisContext, Analyzer, CodeUnitRef};
use decombine::config::{AnalysisConfig, ComparisonConfig, ConcernQuery, ConcernsConfig};
use decombine::db::{ModelIdentity, Project};
use decombine::embed::Embedder;

fn identity() -> ModelIdentity {
    ModelIdentity {
        backend: "test".into(),
        backend_version: "0".into(),
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

/// (project, path, name, scope, hash, start_line) → CodeUnitRef
fn unit(
    project: &str,
    path: &str,
    name: &str,
    scope: Option<&str>,
    hash: &str,
    start_line: usize,
) -> CodeUnitRef {
    CodeUnitRef {
        id: 0,
        project_label: project.into(),
        relative_path: path.into(),
        language_id: "rust".into(),
        kind: "function".into(),
        name: name.into(),
        scope: scope.map(str::to_string),
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

fn analysis_config() -> AnalysisConfig {
    serde_yaml::from_str("{}").unwrap()
}

fn run_duplicates(ctx: &AnalysisContext) -> decombine::analyze::duplicate::DuplicateReport {
    run_duplicates_ignoring(ctx, &[])
}

fn run_duplicates_ignoring(
    ctx: &AnalysisContext,
    ignored: &[&str],
) -> decombine::analyze::duplicate::DuplicateReport {
    let analyzer = DuplicateAnalyzer {
        ignored_hashes: ignored.iter().map(|s| s.to_string()).collect(),
    };
    analyzer.run(ctx, &analysis_config()).unwrap()
}

// ---------- duplicate pipeline ----------

#[test]
fn near_duplicates_cluster_and_exact_copies_fold() {
    let same = normalize(vec![1.0, 0.1, 0.0, 0.0]);
    let close = normalize(vec![1.0, 0.2, 0.05, 0.0]);
    let other = normalize(vec![0.0, 0.0, 1.0, 0.0]);
    let units = vec![
        unit("main", "a/one.rs", "f1", None, "hash-same", 10),
        unit("main", "b/two.rs", "f2", None, "hash-same", 10), // exact copy of f1
        unit("main", "c/three.rs", "f3", None, "hash-close", 10),
        unit("main", "d/four.rs", "f4", None, "hash-other", 10),
    ];
    let vectors = vec![
        Some(same.clone()),
        Some(same.clone()),
        Some(close),
        Some(other),
    ];
    let report = run_duplicates(&ctx(units, vectors, &["main"]));

    assert_eq!(report.clusters.len(), 1);
    let cluster = &report.clusters[0];
    assert_eq!(cluster.members, vec![0, 1, 2]);
    // Exact-copy folding: two distinct hashes, one group has two members.
    assert_eq!(cluster.exact_groups.len(), 2);
    let same_group = cluster
        .exact_groups
        .iter()
        .find(|g| g.normalized_body_hash == "hash-same")
        .unwrap();
    assert_eq!(same_group.members, vec![0, 1]);
    assert_eq!(cluster.hash.len(), 16);
}

#[test]
fn overlapping_same_file_ranges_excluded_by_bytes() {
    let v = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    // Nested: same file, overlapping byte ranges (100..180 vs 100..180 offset)
    let mut outer = unit("main", "x.rs", "outer", None, "h1", 1);
    outer.start_byte = 0;
    outer.end_byte = 500;
    let mut inner = unit("main", "x.rs", "inner", None, "h2", 3);
    inner.start_byte = 100;
    inner.end_byte = 300;
    // Same lines would collide but bytes are what must matter: give the
    // pair distinct line ranges that still overlap in bytes.
    inner.start_line = 30;
    inner.end_line = 40;

    let report = run_duplicates(&ctx(
        vec![outer, inner],
        vec![Some(v.clone()), Some(v)],
        &["main"],
    ));
    assert!(report.clusters.is_empty(), "nested pair must be excluded");
}

#[test]
fn rerank_rescues_far_apart_near_miss() {
    // Raw cosine ~0.905: below similarity_threshold 0.92, but boosted by
    // the max path boost (x1.15 => ~1.04) above rerank_threshold 0.94.
    let a = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    let b = normalize(vec![1.0, 0.45, 0.0, 0.0]); // cos ≈ 0.912
    let units_far = vec![
        unit("main", "mod1/deep/one.rs", "f1", None, "h1", 10),
        unit("main", "mod2/other/deep/two.rs", "f2", None, "h2", 10),
    ];
    let report = run_duplicates(&ctx(
        units_far,
        vec![Some(a.clone()), Some(b.clone())],
        &["main"],
    ));
    assert_eq!(report.clusters.len(), 1, "distance boost rescues the pair");
    let pair = report.clusters[0].pairs[0];
    assert!(pair.raw < 0.92 && pair.boosted > 0.94);

    // The same vectors side by side in one directory are dropped.
    let units_near = vec![
        unit("main", "mod1/one.rs", "f1", None, "h1", 10),
        unit("main", "mod1/two.rs", "f2", None, "h2", 10),
    ];
    let report = run_duplicates(&ctx(units_near, vec![Some(a), Some(b)], &["main"]));
    assert!(report.clusters.is_empty());
}

#[test]
fn ignore_file_hashes_suppress_clusters() {
    let v = normalize(vec![1.0, 0.05, 0.0, 0.0]);
    let units = vec![
        unit("main", "a/one.rs", "f1", None, "h1", 10),
        unit("main", "b/two.rs", "f2", None, "h1", 10),
    ];
    let vectors = vec![Some(v.clone()), Some(v)];
    let report = run_duplicates(&ctx(units.clone(), vectors.clone(), &["main"]));
    assert_eq!(report.clusters.len(), 1);
    let hash = report.clusters[0].hash.clone();

    let report = run_duplicates_ignoring(&ctx(units, vectors, &["main"]), &[hash.as_str()]);
    assert!(report.clusters.is_empty());
    assert_eq!(report.ignored, vec![hash]);
}

#[test]
fn low_complexity_semantic_pairs_are_filtered_but_exact_copies_remain() {
    let v1 = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    let v2 = normalize(vec![1.0, 0.02, 0.0, 0.0]);
    let mut small_a = unit("main", "a.rs", "a", None, "ha", 10);
    let mut small_b = unit("main", "b.rs", "b", None, "hb", 10);
    small_a.body_node_count = 10;
    small_b.body_node_count = 10;
    let report = run_duplicates(&ctx(
        vec![small_a.clone(), small_b.clone()],
        vec![Some(v1.clone()), Some(v2.clone())],
        &["main"],
    ));
    assert!(report.clusters.is_empty());

    small_b.normalized_body_hash = small_a.normalized_body_hash.clone();
    let report = run_duplicates(&ctx(
        vec![small_a, small_b],
        vec![Some(v1.clone()), Some(v1)],
        &["main"],
    ));
    assert_eq!(report.clusters.len(), 1);
}

#[test]
fn cross_directory_candidates_span_and_suppression() {
    let dup = normalize(vec![1.0, 0.02, 0.0, 0.0]);
    let local = normalize(vec![0.0, 0.0, 1.0, 0.02]);
    let helper = normalize(vec![0.0, 1.0, 0.0, 0.02]);
    let units = vec![
        // Cluster A: spans two top-level modules, different class scopes.
        unit(
            "main",
            "billing/invoice.rs",
            "send",
            Some("Invoice"),
            "ha1",
            10,
        ),
        unit(
            "main",
            "shipping/label.rs",
            "send",
            Some("Label"),
            "ha2",
            10,
        ),
        // Cluster B: two files in the same directory (local only).
        unit("main", "core/a.rs", "l1", None, "hb1", 10),
        unit("main", "core/b.rs", "l2", None, "hb2", 10),
        // Cluster C: generic helpers under utils/, spans modules.
        unit("main", "utils/strings.rs", "pad", None, "hc1", 10),
        unit("main", "api/utils/text.rs", "pad2", None, "hc2", 10),
    ];
    let vectors = vec![
        Some(dup.clone()),
        Some(dup),
        Some(local.clone()),
        Some(local),
        Some(helper.clone()),
        Some(helper),
    ];
    let report = run_duplicates(&ctx(units, vectors, &["main"]));
    assert_eq!(report.clusters.len(), 3);

    let candidates = &report.cross_directory;
    // The local-only cluster is suppressed.
    assert_eq!(candidates.len(), 2);
    // The cross-scope method cluster outranks the generic helper cluster.
    assert!(candidates[0].scopes.len() >= 2, "{candidates:?}");
    assert!(!candidates[0].generic_helper);
    assert!(candidates[1].generic_helper);
    assert!(candidates[0].score > candidates[1].score);
    assert_eq!(candidates[0].modules.len(), 2);
    assert!(candidates[0].dispersion >= 1.0);
}

#[test]
fn reports_are_deterministic() {
    let v1 = normalize(vec![1.0, 0.03, 0.0, 0.0]);
    let v2 = normalize(vec![0.0, 1.0, 0.03, 0.0]);
    let units = vec![
        unit("main", "a/one.rs", "f1", None, "h1", 10),
        unit("main", "b/two.rs", "f2", None, "h2", 10),
        unit("main", "c/three.rs", "f3", None, "h3", 10),
        unit("main", "d/four.rs", "f4", None, "h4", 10),
    ];
    let vectors = vec![Some(v1.clone()), Some(v1), Some(v2.clone()), Some(v2)];
    let a = run_duplicates(&ctx(units.clone(), vectors.clone(), &["main"]));
    let b = run_duplicates(&ctx(units, vectors, &["main"]));
    assert_eq!(a.clusters, b.clusters);
    assert_eq!(a.cross_directory, b.cross_directory);
}

// ---------- context loading from the database ----------

#[test]
fn context_loads_projects_units_and_missing_embeddings() -> Result<()> {
    use decombine::embed::hash::HashEmbedder;
    let dir = tempfile::tempdir()?;
    for project in ["src-a", "src-b"] {
        std::fs::create_dir_all(dir.path().join(project))?;
    }
    let config_path = dir.path().join("decombine.yaml");
    std::fs::write(
        &config_path,
        "projects:\n  - label: v1\n    source_dir: src-a\n  - label: v2\n    source_dir: src-b\nanalysis:\n  body_node_count_threshold: 5\n",
    )?;
    let config = decombine::config::Config::load(&config_path)?;
    let write = |p: &str, f: &str, body: &str| {
        std::fs::write(dir.path().join(p).join(f), body).unwrap();
    };
    let big_fn = |name: &str, var: &str| {
        format!(
            "fn {name}(values: Vec<i64>) -> i64 {{\n    let mut {var} = 0;\n    for value in values {{\n        if value > 0 {{\n            {var} += value;\n        }}\n    }}\n    {var}\n}}\n"
        )
    };
    write("src-a", "one.rs", &big_fn("alpha", "total"));
    write("src-b", "two.rs", &big_fn("beta", "count"));
    write("src-b", "three.rs", &big_fn("gamma", "sum"));

    let db = decombine::db::open_or_create(&config.db_file)?;
    decombine::index::indexer::index(&db, &config, None)?;
    let mut embedder = HashEmbedder::new(4);
    decombine::embed::embed_pending(&db, &mut embedder, &config)?;

    // Selected-project filtering.
    let ctx = AnalysisContext::load(&db, &["v2".to_string()])?;
    assert_eq!(ctx.projects.len(), 1);
    assert_eq!(ctx.units.len(), 2);
    assert!(ctx.units.iter().all(|u| u.project_label == "v2"));

    // Full load.
    let ctx = AnalysisContext::load(&db, &[])?;
    assert_eq!(ctx.projects.len(), 2);
    assert_eq!(ctx.units.len(), 3);
    assert_eq!(ctx.vectors.len(), 3);

    // A unit indexed after embedding has no vector: sparse ids tolerated.
    write("src-a", "late.rs", &big_fn("late", "acc"));
    decombine::index::indexer::index(&db, &config, None)?;
    let ctx = AnalysisContext::load(&db, &[])?;
    assert_eq!(ctx.units.len(), 4);
    assert_eq!(ctx.vectors.len(), 3, "missing embedding leaves no row");
    let late = ctx.units.iter().position(|u| u.name == "late").unwrap();
    assert!(ctx.vectors.row_for_unit(late).is_none());

    assert!(AnalysisContext::load(&db, &["nope".to_string()]).is_err());
    Ok(())
}

// ---------- concerns (6b) ----------

/// Embedder returning pre-baked vectors for query texts.
struct FixtureEmbedder {
    identity: ModelIdentity,
}

impl Embedder for FixtureEmbedder {
    fn identity(&self) -> &ModelIdentity {
        &self.identity
    }
    fn embed(&mut self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(inputs
            .iter()
            .map(|text| match text.as_str() {
                "error handling and retries" => vec![1.0, 0.0, 0.0, 0.0],
                "database access" => vec![0.0, 1.0, 0.0, 0.0],
                other => panic!("unexpected query {other}"),
            })
            .collect())
    }
}

#[test]
fn concern_projection_scoring_and_spread() {
    let errorish = normalize(vec![1.0, 0.1, 0.0, 0.0]);
    let very_errorish = normalize(vec![1.0, 0.02, 0.0, 0.0]);
    let dbish = normalize(vec![0.1, 1.0, 0.0, 0.0]);
    let unrelated = normalize(vec![0.0, 0.0, 1.0, 0.0]);
    let units = vec![
        unit("main", "api/handler.rs", "retry", None, "h1", 10),
        unit("main", "worker/job.rs", "backoff", None, "h2", 10),
        unit("main", "store/db.rs", "query", None, "h3", 10),
        unit("main", "ui/view.rs", "render", None, "h4", 10),
    ];
    let vectors = vec![
        Some(errorish),
        Some(very_errorish),
        Some(dbish),
        Some(unrelated),
    ];
    let context = ctx(units, vectors, &["main"]);

    let config = ConcernsConfig {
        enabled: true,
        min_projection: 0.45,
        top_units_per_concern: 10,
        // Deliberately unsorted: output must sort by name.
        queries: vec![
            ConcernQuery {
                name: "errors".into(),
                query: "error handling and retries".into(),
            },
            ConcernQuery {
                name: "database".into(),
                query: "database access".into(),
            },
        ],
    };
    let mut embedder = FixtureEmbedder {
        identity: identity(),
    };
    let report = ConcernAnalyzer {
        embedder: &mut embedder,
    }
    .run_mut(&context, &config)
    .unwrap();

    let names: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["database", "errors"], "stable sorted names");

    let errors = &report.findings[1];
    assert_eq!(errors.units.len(), 2);
    // Ordering by projection: very_errorish beats errorish.
    assert_eq!(errors.units[0].unit, 1);
    assert!(errors.units[0].projection > errors.units[1].projection);
    // Cross-directory spread: two files, two dirs, two top modules.
    assert_eq!(errors.spread.files, 2);
    assert_eq!(errors.spread.directories, 2);
    assert_eq!(errors.spread.top_level_modules, 2);
    assert!(errors.spread.directory_entropy > 0.9);
    assert!(errors.spread.dispersion >= 1.0);

    let database = &report.findings[0];
    assert_eq!(database.units.len(), 1);
    // Same-file spread degenerates to zero entropy/dispersion.
    assert_eq!(database.spread.directories, 1);
    assert_eq!(database.spread.directory_entropy, 0.0);
    assert_eq!(database.spread.dispersion, 0.0);
}

#[test]
fn concern_analyzer_rejects_mismatched_model() {
    let context = ctx(
        vec![unit("main", "a.rs", "f", None, "h", 1)],
        vec![Some(normalize(vec![1.0, 0.0, 0.0, 0.0]))],
        &["main"],
    );
    let mut wrong = identity();
    wrong.model = "different".into();
    let mut embedder = FixtureEmbedder { identity: wrong };
    let config: ConcernsConfig = serde_yaml::from_str("{}").unwrap();
    let error = ConcernAnalyzer {
        embedder: &mut embedder,
    }
    .run_mut(&context, &config)
    .unwrap_err();
    assert!(error.to_string().contains("same model"));
}

// ---------- comparison (6c) ----------

fn comparison_config() -> ComparisonConfig {
    serde_yaml::from_str("{}").unwrap()
}

fn compare(context: &AnalysisContext) -> decombine::analyze::compare::ComparisonReport {
    CompareAnalyzer {
        left_label: "v1".into(),
        right_label: "v2".into(),
    }
    .run(context, &comparison_config())
    .unwrap()
}

#[test]
fn comparison_rejects_bad_label_setups() {
    let context = ctx(
        vec![unit("v1", "a.rs", "f", None, "h", 1)],
        vec![Some(normalize(vec![1.0, 0.0, 0.0, 0.0]))],
        &["v1"],
    );
    // One-project context.
    assert!(compare_err(&context, "v1", "v2").contains("two projects"));

    let two = ctx(
        vec![unit("v1", "a.rs", "f", None, "h", 1)],
        vec![Some(normalize(vec![1.0, 0.0, 0.0, 0.0]))],
        &["v1", "v2"],
    );
    assert!(compare_err(&two, "v1", "v1").contains("different"));
    assert!(compare_err(&two, "v1", "nope").contains("not part"));
}

fn compare_err(context: &AnalysisContext, left: &str, right: &str) -> String {
    CompareAnalyzer {
        left_label: left.into(),
        right_label: right.into(),
    }
    .run(context, &comparison_config())
    .unwrap_err()
    .to_string()
}

#[test]
fn comparison_classes_cover_all_cases() {
    let strong = normalize(vec![1.0, 0.1, 0.0, 0.0]);
    let strong_match = normalize(vec![1.0, 0.15, 0.05, 0.0]); // cos ≈ 0.996
    let split_left = normalize(vec![0.0, 1.0, 0.3, 0.0]);
    let split_r1 = normalize(vec![0.0, 1.0, 0.25, 0.05]);
    let split_r2 = normalize(vec![0.05, 1.0, 0.35, 0.0]);
    let lonely_left = normalize(vec![0.0, 0.0, 0.0, 1.0]);
    let new_right = normalize(vec![0.5, 0.0, 0.86, 0.0]);

    let units = vec![
        // v1 (left)
        unit("v1", "src/exact.rs", "same", None, "hash-exact", 1), // 0
        unit("v1", "src/api.rs", "handle", None, "h-l1", 10),      // 1 strong
        unit("v1", "src/big.rs", "monolith", None, "h-l2", 20),    // 2 split
        unit("v1", "src/gone.rs", "legacy", None, "h-l3", 30),     // 3 missing
        // v2 (right)
        unit("v2", "lib/exact.rs", "same", None, "hash-exact", 1), // 4
        unit("v2", "lib/api.rs", "handle", None, "h-r1", 10),      // 5 strong
        unit("v2", "lib/part_one.rs", "part1", None, "h-r2", 20),  // 6 split
        unit("v2", "lib/part_two.rs", "part2", None, "h-r3", 25),  // 7 split
        unit("v2", "lib/fresh.rs", "brand_new", None, "h-r4", 40), // 8 new
    ];
    let vectors = vec![
        Some(normalize(vec![0.3, 0.3, 0.3, 0.3])),
        Some(strong.clone()),
        Some(split_left),
        Some(lonely_left),
        Some(normalize(vec![0.3, 0.3, 0.3, 0.3])),
        Some(strong_match),
        Some(split_r1),
        Some(split_r2),
        Some(new_right),
    ];
    let context = ctx(units, vectors, &["v1", "v2"]);
    let report = compare(&context);

    assert_eq!(report.count(MatchClass::ExactCopy), 1);
    assert_eq!(report.count(MatchClass::StrongMatch), 1);
    assert_eq!(report.count(MatchClass::Split), 1);
    assert_eq!(report.count(MatchClass::MissingInRight), 1);
    assert_eq!(report.count(MatchClass::NewInRight), 1);

    // Exact copies are classified by hash, separately from semantics.
    let exact = report
        .matches
        .iter()
        .find(|m| m.class == MatchClass::ExactCopy)
        .unwrap();
    assert_eq!(
        (exact.left.as_slice(), exact.right.as_slice()),
        (&[0][..], &[4][..])
    );

    let strong = report
        .matches
        .iter()
        .find(|m| m.class == MatchClass::StrongMatch)
        .unwrap();
    assert_eq!(
        (strong.left.as_slice(), strong.right.as_slice()),
        (&[1][..], &[5][..])
    );

    let split = report
        .matches
        .iter()
        .find(|m| m.class == MatchClass::Split)
        .unwrap();
    assert_eq!(split.left, vec![2]);
    assert_eq!(split.right, vec![6, 7]);

    // Cross-project only: no match pairs two units of the same project.
    for record in &report.matches {
        for &l in &record.left {
            assert_eq!(context.units[l].project_label, "v1");
        }
        for &r in &record.right {
            assert_eq!(context.units[r].project_label, "v2");
        }
    }

    // Coverage rows aggregate the left side.
    let directory = &report.coverage_by_directory;
    assert_eq!(directory.len(), 1);
    assert_eq!(directory[0].group, "src");
    assert_eq!(directory[0].total_left, 4);
    assert_eq!(directory[0].covered, 3);
    assert_eq!(directory[0].missing, 1);
}

#[test]
fn merge_classification() {
    let merged = normalize(vec![0.0, 1.0, 0.3, 0.0]);
    let left_a = normalize(vec![0.0, 1.0, 0.25, 0.05]);
    let left_b = normalize(vec![0.05, 1.0, 0.35, 0.0]);
    let units = vec![
        unit("v1", "src/a.rs", "part1", None, "h1", 1),
        unit("v1", "src/b.rs", "part2", None, "h2", 10),
        unit("v2", "lib/all.rs", "combined", None, "h3", 1),
    ];
    let context = ctx(
        units,
        vec![Some(left_a), Some(left_b), Some(merged)],
        &["v1", "v2"],
    );
    let report = compare(&context);
    assert_eq!(report.count(MatchClass::Merge), 1);
    let merge = report
        .matches
        .iter()
        .find(|m| m.class == MatchClass::Merge)
        .unwrap();
    assert_eq!(merge.left, vec![0, 1]);
    assert_eq!(merge.right, vec![2]);
}

#[test]
fn hints_cannot_rescue_weak_edges() {
    // Cosine ≈ 0.83: above candidate (0.78) but below match (0.86) even
    // though name and directory hints both apply.
    let a = normalize(vec![1.0, 0.0, 0.0, 0.0]);
    let b = normalize(vec![1.0, 0.67, 0.0, 0.0]);
    let units = vec![
        unit("v1", "src/x.rs", "same_name", None, "h1", 1),
        unit("v2", "src/y.rs", "same_name", None, "h2", 1),
    ];
    let context = ctx(units, vec![Some(a), Some(b)], &["v1", "v2"]);
    let report = compare(&context);
    assert_eq!(report.count(MatchClass::StrongMatch), 0);
    assert_eq!(report.count(MatchClass::PossibleMatch), 1);
    let possible = &report.matches[0];
    assert!(possible.hint_bonus > 0.0 && possible.hint_bonus <= 0.04);
}
