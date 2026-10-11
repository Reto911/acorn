//! Backend executable entry point.

use std::process::ExitCode;

use acorn_backend::{
    config::{Cli, Config},
    database::Database,
    logging,
};
use anyhow::{Result, bail};
use clap::Parser;

/// Validate backend startup prerequisites before the later HTTP and Scheduler stages.
#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match prepare(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Backend startup failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Load configuration, initialize logging, and close the validated database cleanly.
async fn prepare(cli: &Cli) -> Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(&config.logging)?;
    let database = Database::open(&config).await?;
    database.close().await;
    bail!("Backend HTTP and Scheduler services are not implemented yet")
}
