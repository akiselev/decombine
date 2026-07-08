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
use decombine::analyze::duplicate::{DuplicateAnalyzer, ignore::load_ignored_hashes};
use decombine::analyze::{AnalysisContext, Analyzer};
use decombine::cli::{AnalysisCommand, Cli, Command, CompareArgs, LanguagesCommand, ModelsCommand};
use decombine::config::{CONFIG_TEMPLATE, Config};
use decombine::db::Db;
use decombine::index::indexer;
use decombine::index::language::LanguageRegistry;
use decombine::report;

fn project_scope(ctx: &AnalysisContext) -> Vec<String> {
    ctx.projects.iter().map(|p| p.label.clone()).collect()
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
                |progress| {
                    eprintln!(
                        "embedded {}/{} bodies (batch {}, size {}, unresolved {})",
                        progress.embedded + progress.unresolved,
                        progress.pending_total,
                        progress.batches,
                        progress.current_batch,
                        progress.unresolved
                    );
                },
            )?;
            println!(
                "embedded {} new bodies in {} batches ({} unresolved)",
                stats.embedded, stats.batches, stats.unresolved
            );
            Ok(())
        }
        Command::Models(args) => match args.command {
            ModelsCommand::List => {
                for (name, dims, has_quantized) in decombine::config::SUPPORTED_MODELS {
                    println!(
                        "{name}: {dims} dims{}",
                        if *has_quantized {
                            " (quantized variant available)"
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
        Command::Doctor => {
            let config = Config::load(&cli.config)?;
            println!("config: ok ({})", cli.config.display());
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
            Ok(())
        }
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
                |progress| {
                    eprintln!(
                        "embedded {}/{} bodies (batch {}, size {}, unresolved {})",
                        progress.embedded + progress.unresolved,
                        progress.pending_total,
                        progress.batches,
                        progress.current_batch,
                        progress.unresolved
                    );
                },
            )?;
            println!(
                "embedded {} new bodies in {} batches ({} unresolved)",
                embed_stats.embedded, embed_stats.batches, embed_stats.unresolved
            );
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
