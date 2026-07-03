use anyhow::{Context, Result, bail};
use clap::Parser;

use decombine::cli::{Cli, Command, LanguagesCommand};
use decombine::config::{CONFIG_TEMPLATE, Config};
use decombine::index::indexer;
use decombine::index::language::LanguageRegistry;

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
                    "{}: indexed={} skipped={} removed={} failed={} units={}",
                    project.label,
                    project.indexed,
                    project.skipped,
                    project.removed,
                    project.failed,
                    project.units
                );
            }
            Ok(())
        }
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
        command => bail!("`{command:?}` is not implemented yet"),
    }
}
