use anyhow::{Context, Result, bail};
use clap::Parser;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use decombine::analyze::compare::CompareAnalyzer;
use decombine::analyze::concerns::ConcernAnalyzer;
use decombine::analyze::drift::{DriftReport, DriftSide, compute_drift};
use decombine::analyze::duplicate::{DuplicateAnalyzer, ignore::load_ignored_hashes};
use decombine::analyze::{AnalysisContext, Analyzer};
use decombine::cli::{
    AnalysisCommand, Cli, Command, CompareArgs, DoctorArgs, DriftArgs, LanguagesCommand,
    ModelsCommand,
};
use decombine::config::{CONFIG_TEMPLATE, Config};
use decombine::db::Db;
use decombine::index::indexer;
use decombine::index::language::LanguageRegistry;
use decombine::report;

fn project_scope(ctx: &AnalysisContext) -> Vec<String> {
    ctx.projects.iter().map(|p| p.label.clone()).collect()
}

fn run_doctor(config_path: &std::path::Path, args: &DoctorArgs) -> Result<()> {
    let mut config = Config::load(config_path)?;
    println!("config: ok ({})", config_path.display());
    for project in config.resolved_projects() {
        println!(
            "project {}: {}",
            project.label,
            project.source_dir.display()
        );
    }
    println!(
        "embedding: backend={} model={} dims={}",
        config.embedding.backend,
        config.embedding.model,
        config.embedding.dimensions()
    );
    println!(
        "runtime: ort {} (fastembed {})",
        option_env!("DECOMBINE_ORT_VERSION").unwrap_or("unknown"),
        option_env!("DECOMBINE_FASTEMBED_VERSION").unwrap_or("unknown"),
    );
    println!(
        "execution provider: {} (mode {})",
        config.embedding.execution_provider,
        config.embedding.provider_mode.as_str()
    );
    println!("accelerators:");
    for diag in decombine::embed::accelerator_diagnostics() {
        let status = if !diag.compiled {
            "not compiled in".to_string()
        } else {
            match (diag.available, diag.platform_supported) {
                (Some(true), Some(true)) => "compiled, available".to_string(),
                (Some(true), Some(false)) => "compiled, unsupported on this platform".to_string(),
                (Some(false), _) => "compiled, but ONNX Runtime lacks it".to_string(),
                _ => "compiled".to_string(),
            }
        };
        println!("  {}: {}", diag.name, status);
    }

    if let Some(provider) = &args.provider {
        config.embedding.execution_provider = provider.clone();
        config.validate()?;
        println!("smoke test: loading model with execution_provider={provider} ...");
        let mut embedder = decombine::embed::embedder_from_config(&config)?;
        let vectors = embedder.embed(&["decombine execution provider smoke test".to_string()])?;
        let dims = vectors.first().map(|v| v.len()).unwrap_or(0);
        println!(
            "smoke test: ok — {} dims, provider in effect: {}",
            dims,
            embedder.identity().execution_provider
        );
    }
    Ok(())
}

fn report_meta(config: &Config, db: &Db, ctx: &AnalysisContext) -> report::ReportMeta {
    let timestamp: String = db
        .conn()
        .query_row("SELECT datetime('now') || 'Z'", [], |row| row.get(0))
        .unwrap_or_default();
    report::ReportMeta {
        identity: ctx.identity.clone(),
        analysis: config.analysis.clone(),
        retention: config.index.retention,
        timestamp,
        projects: ctx
            .projects
            .iter()
            .map(|p| (p.label.clone(), p.source_dir.clone()))
            .collect(),
        ignore_file: config.ignore_file.display().to_string(),
    }
}

fn with_progress_indicator<T>(
    message: impl Into<String>,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let message = message.into();
    let done = Arc::new(AtomicBool::new(false));
    let worker_done = Arc::clone(&done);
    let worker_message = message.clone();
    eprintln!("model: [----------] {message}");
    let progress = thread::spawn(move || {
        let started = Instant::now();
        let mut tick = 0usize;
        while !worker_done.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(2));
            if worker_done.load(Ordering::Relaxed) {
                break;
            }
            tick = tick.wrapping_add(1);
            let pos = tick % 10;
            let mut bar = String::with_capacity(10);
            for idx in 0..10 {
                bar.push(if idx == pos { '>' } else { '=' });
            }
            eprintln!(
                "model: [{bar}] {worker_message} ({}s)",
                started.elapsed().as_secs()
            );
        }
    });
    let result = f();
    done.store(true, Ordering::Relaxed);
    let _ = progress.join();
    match &result {
        Ok(_) => eprintln!("model: [==========] {message}: done"),
        Err(_) => eprintln!("model: [!!!!!!!!!!] {message}: failed"),
    }
    result
}

fn load_embedder_with_progress(config: &Config) -> Result<Box<dyn decombine::embed::Embedder>> {
    with_progress_indicator(
        format!(
            "loading/downloading embedding model {}",
            config.embedding.model
        ),
        || decombine::embed::embedder_from_config(config),
    )
}

fn analyze_duplicates(config: &Config, db: &Db) -> Result<()> {
    let ctx = AnalysisContext::load(db, &[])?;
    let analyzer = DuplicateAnalyzer {
        ignored_hashes: load_ignored_hashes(&config.ignore_file)?,
    };
    let output = analyzer.run(&ctx, &config.analysis)?;
    report::clean_report_dir(&config.report_dir)?;
    report::write_duplicate_report(
        &config.report_dir,
        &report_meta(config, db, &ctx),
        &ctx,
        &output,
    )?;
    db.create_analysis_run(
        "duplicates",
        ctx.model_id,
        &project_scope(&ctx),
        &serde_json::to_string(&config.analysis)?,
    )?;
    println!(
        "{} clusters ({} ignored), {} cross-directory candidates → {}",
        output.clusters.len(),
        output.ignored.len(),
        output.cross_directory.len(),
        config.report_dir.display()
    );
    Ok(())
}

fn analyze_concerns(config: &Config, db: &Db) -> Result<()> {
    if config.analysis.concerns.queries.is_empty() {
        bail!("no concern queries configured under `analysis.concerns.queries`");
    }
    let ctx = AnalysisContext::load(db, &[])?;
    let mut embedder = decombine::embed::embedder_from_config(config)?;
    let mut analyzer = ConcernAnalyzer {
        embedder: embedder.as_mut(),
    };
    let output = analyzer.run_mut(&ctx, &config.analysis.concerns)?;
    report::write_concern_report(
        &config.report_dir,
        &report_meta(config, db, &ctx),
        &ctx,
        &output,
    )?;
    db.create_analysis_run(
        "concerns",
        ctx.model_id,
        &project_scope(&ctx),
        &serde_json::to_string(&config.analysis.concerns)?,
    )?;
    println!(
        "{} candidate concerns → {}/concerns",
        output.findings.len(),
        config.report_dir.display()
    );
    Ok(())
}

fn run_compare(config: &Config, db: &Db, args: &CompareArgs) -> Result<()> {
    let comparison = config.comparison.clone().unwrap_or_default();
    let left = args
        .left
        .clone()
        .or(comparison.left.clone())
        .context("no `left` project: pass --left or set comparison.left")?;
    let right = args
        .right
        .clone()
        .or(comparison.right.clone())
        .context("no `right` project: pass --right or set comparison.right")?;
    let ctx = AnalysisContext::load(db, &[left.clone(), right.clone()])?;
    let analyzer = CompareAnalyzer {
        left_label: left,
        right_label: right,
    };
    let output = analyzer.run_with_progress(&ctx, &comparison, |phase| {
        eprintln!("compare: {phase}");
    })?;
    report::write_comparison_report(
        &config.report_dir,
        &report_meta(config, db, &ctx),
        &ctx,
        &output,
    )?;
    db.create_analysis_run(
        "compare",
        ctx.model_id,
        &project_scope(&ctx),
        &serde_json::to_string(&comparison)?,
    )?;
    println!(
        "compared `{}` vs `{}`: {} match records → {}/compare",
        output.left_label,
        output.right_label,
        output.matches.len(),
        config.report_dir.display()
    );
    Ok(())
}

/// One-line token-length summary after an embed run; silent when nothing
/// was embedded.
fn print_embed_progress(progress: decombine::embed::EmbedProgress) {
    eprintln!(
        "embedded {}/{} bodies (batch {}, size {}, unresolved {})",
        progress.embedded + progress.unresolved,
        progress.pending_total,
        progress.batches,
        progress.current_batch,
        progress.unresolved
    );
}

type LoadedSide = (decombine::db::EmbeddingModelRecord, Vec<(String, Vec<f32>)>);

/// Load the single embedding model of a database and its stored vectors.
fn load_drift_side(path: &std::path::Path) -> Result<LoadedSide> {
    let db = decombine::db::open_or_create(path)?;
    let model = match db.list_models()?.as_slice() {
        [one] => one.clone(),
        [] => bail!("{} has no embedding model", path.display()),
        many => bail!(
            "{} has {} embedding models; drift needs one embedding set per database",
            path.display(),
            many.len()
        ),
    };
    let embeddings = db.all_embeddings(model.id)?;
    Ok((model, embeddings))
}

fn run_drift(args: &DriftArgs) -> Result<()> {
    let (baseline_model, baseline_emb) = load_drift_side(&args.baseline)?;
    let (candidate_model, candidate_emb) = load_drift_side(&args.candidate)?;
    let baseline = DriftSide {
        label: "baseline",
        identity: &baseline_model.identity,
        embeddings: baseline_emb,
    };
    let candidate = DriftSide {
        label: "candidate",
        identity: &candidate_model.identity,
        embeddings: candidate_emb,
    };
    let report = compute_drift(&baseline, &candidate, args.top_k, args.sample)?;
    print_drift(&report);

    let cosine_ok = report.cosine_min >= args.min_cosine;
    let recall_ok = report.recall_queries == 0 || report.mean_neighbor_recall >= args.min_recall;
    if cosine_ok && recall_ok {
        println!(
            "gate: PASS (min cosine >= {:.6}, mean recall >= {:.4})",
            args.min_cosine, args.min_recall
        );
        Ok(())
    } else {
        bail!(
            "drift gate FAILED: min cosine {:.6} (need >= {:.6}), mean recall {:.4} (need >= {:.4})",
            report.cosine_min,
            args.min_cosine,
            report.mean_neighbor_recall,
            args.min_recall,
        );
    }
}

fn print_drift(r: &DriftReport) {
    println!(
        "drift: baseline {} (provider {}, {} bodies) vs candidate {} (provider {}, {} bodies)",
        r.baseline_model,
        r.baseline_provider,
        r.baseline_count,
        r.candidate_model,
        r.candidate_provider,
        r.candidate_count,
    );
    println!("shared bodies: {} ({}-dim)", r.shared, r.dimensions);
    println!(
        "cosine: mean {:.6} / p50 {:.6} / p05 {:.6} / min {:.6}; max |Δcomponent| {:.6}",
        r.cosine_mean, r.cosine_p50, r.cosine_p05, r.cosine_min, r.max_abs_component_delta,
    );
    if r.recall_queries > 0 {
        println!(
            "top-{} neighbour recall: {:.4} (over {} queries)",
            r.top_k, r.mean_neighbor_recall, r.recall_queries,
        );
    } else {
        println!(
            "top-{} neighbour recall: n/a (fewer than 2 shared units)",
            r.top_k
        );
    }
}

fn print_token_stats(tokens: &decombine::embed::TokenStats) {
    if tokens.count() == 0 {
        return;
    }
    println!(
        "token lengths: p50 {} / p90 {} / p99 {} / max {}, truncated {} of {} ({:.1}%), padding waste {:.1}%",
        tokens.percentile(0.50),
        tokens.percentile(0.90),
        tokens.percentile(0.99),
        tokens.max(),
        tokens.truncated,
        tokens.count(),
        100.0 * tokens.truncated as f64 / tokens.count() as f64,
        100.0 * tokens.padding_waste(),
    );
}

fn print_token_report(report: &[decombine::embed::LanguageTokens], max_len: usize) {
    if report.is_empty() {
        println!("no indexed units to measure (is the database indexed?)");
        return;
    }
    // Over-cap thresholds: 512 (the BGE/catalog ceiling) and the model's own
    // cap, deduped so a 512-cap model shows one column.
    let mut thresholds = vec![512usize, max_len];
    thresholds.sort_unstable();
    thresholds.dedup();

    let over_headers: String = thresholds
        .iter()
        .map(|t| format!("{:>13}", format!(">{t}")))
        .collect();
    println!(
        "untruncated token lengths per language (model cap {max_len} tokens):\n{:<12}{:>8}{:>7}{:>7}{:>7}{:>8}{}",
        "language", "units", "p50", "p90", "p99", "max", over_headers,
    );

    let mut total = decombine::embed::TokenStats::default();
    let print_row = |name: &str, s: &decombine::embed::TokenStats| {
        let overs: String = thresholds
            .iter()
            .map(|&t| {
                let n = s.over(t);
                format!(
                    "{:>13}",
                    format!("{n} ({:.1}%)", 100.0 * n as f64 / s.count().max(1) as f64)
                )
            })
            .collect();
        println!(
            "{:<12}{:>8}{:>7}{:>7}{:>7}{:>8}{}",
            name,
            s.count(),
            s.percentile(0.50),
            s.percentile(0.90),
            s.percentile(0.99),
            s.max(),
            overs,
        );
    };
    for lang in report {
        print_row(&lang.language, &lang.stats);
        total.merge(&lang.stats);
    }
    if report.len() > 1 {
        print_row("ALL", &total);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Init(args) => {
            if cli.config.exists() && !args.force {
                bail!(
                    "{} already exists; use --force to overwrite",
                    cli.config.display()
                );
            }
            std::fs::write(&cli.config, CONFIG_TEMPLATE)
                .with_context(|| format!("cannot write {}", cli.config.display()))?;
            println!("wrote {}", cli.config.display());
            Ok(())
        }
        Command::ShowConfig => {
            let config = Config::load(&cli.config)?;
            print!("{}", serde_yaml::to_string(&config)?);
            Ok(())
        }
        Command::Index(args) => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            let stats = indexer::index(&db, &config, args.project.as_deref())?;
            for project in &stats {
                println!(
                    "{}: indexed={} skipped={} removed={} failed={} units_indexed={} units_total={}",
                    project.label,
                    project.indexed,
                    project.skipped,
                    project.removed,
                    project.failed,
                    project.units,
                    project.total_units
                );
            }
            Ok(())
        }
        Command::Embed => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            let mut embedder = load_embedder_with_progress(&config)?;
            let stats = decombine::embed::embed_pending_with_progress(
                &db,
                embedder.as_mut(),
                &config,
                print_embed_progress,
            )?;
            println!(
                "embedded {} new bodies in {} batches ({} unresolved)",
                stats.embedded, stats.batches, stats.unresolved
            );
            print_token_stats(&stats.tokens);
            Ok(())
        }
        Command::Tokens => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            let embedder = load_embedder_with_progress(&config)?;
            let report = decombine::embed::token_report(&db, &config, embedder.as_ref())?;
            print_token_report(&report, embedder.max_sequence_length());
            Ok(())
        }
        Command::Models(args) => match args.command {
            ModelsCommand::List => {
                for m in decombine::config::MANAGED_MODELS {
                    println!(
                        "{}: {} dims, {}-token context (managed: downloaded + verified on first use)",
                        m.name, m.dimensions, m.max_length
                    );
                }
                for (name, dims, has_quantized) in decombine::config::SUPPORTED_MODELS {
                    println!(
                        "{name}: {dims} dims, 512-token context (fastembed catalog){}",
                        if *has_quantized {
                            ", quantized variant available"
                        } else {
                            ""
                        }
                    );
                }
                Ok(())
            }
            ModelsCommand::Download => {
                let config = Config::load(&cli.config)?;
                // Constructing the backend downloads the model into cache.
                let embedder = load_embedder_with_progress(&config)?;
                let identity = embedder.identity();
                println!(
                    "model {} ready (dims={}, cache={})",
                    identity.model,
                    identity.dimensions,
                    identity.cache_path.as_deref().unwrap_or("default")
                );
                Ok(())
            }
        },
        Command::Languages(args) => match args.command {
            LanguagesCommand::List => {
                let config = Config::load(&cli.config).ok();
                let enabled = config.map(|c| c.languages.enabled);
                for id in LanguageRegistry::global().ids() {
                    let state = match &enabled {
                        Some(list) if list.iter().any(|e| e == id) => "enabled",
                        Some(_) => "disabled",
                        None => "available",
                    };
                    println!("{id}: {state}");
                }
                Ok(())
            }
        },
        Command::Doctor(args) => run_doctor(&cli.config, args),
        Command::Drift(args) => run_drift(args),
        Command::Analyze(args) => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            match args.analysis {
                None | Some(AnalysisCommand::Duplicates) => analyze_duplicates(&config, &db),
                Some(AnalysisCommand::Concerns) => analyze_concerns(&config, &db),
            }
        }
        Command::Compare(args) => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            run_compare(&config, &db, args)
        }
        Command::Run(args) => {
            let config = Config::load(&cli.config)?;
            let db = decombine::db::open_or_create(&config.db_file)?;
            let stats = indexer::index(&db, &config, None)?;
            for project in &stats {
                println!(
                    "{}: indexed={} skipped={} removed={} failed={} units_indexed={} units_total={}",
                    project.label,
                    project.indexed,
                    project.skipped,
                    project.removed,
                    project.failed,
                    project.units,
                    project.total_units
                );
            }
            let mut embedder = decombine::embed::embedder_from_config(&config)?;
            let embed_stats = decombine::embed::embed_pending_with_progress(
                &db,
                embedder.as_mut(),
                &config,
                print_embed_progress,
            )?;
            println!(
                "embedded {} new bodies in {} batches ({} unresolved)",
                embed_stats.embedded, embed_stats.batches, embed_stats.unresolved
            );
            print_token_stats(&embed_stats.tokens);
            drop(embedder);
            match args.analysis {
                None | Some(AnalysisCommand::Duplicates) => {
                    analyze_duplicates(&config, &db)?;
                    if config.analysis.concerns.enabled {
                        analyze_concerns(&config, &db)?;
                    }
                    Ok(())
                }
                Some(AnalysisCommand::Concerns) => analyze_concerns(&config, &db),
            }
        }
    }
}
