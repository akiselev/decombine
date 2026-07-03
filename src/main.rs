mod cli;

use anyhow::Result;
use clap::Parser;

use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    anyhow::bail!("`{:?}` is not implemented yet", cli.command)
}
