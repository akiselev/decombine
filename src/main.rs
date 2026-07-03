mod cli;
mod config;
mod db;

use anyhow::{Context, Result, bail};
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::config::{CONFIG_TEMPLATE, Config};

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
