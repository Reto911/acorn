//! Worker executable entry point.

use std::process::ExitCode;

use acorn_worker::{
    config::{Cli, Config},
    language, logging,
};
use anyhow::{Result, bail};
use clap::Parser;

/// Validate startup configuration and report the pending RPC implementation.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match prepare(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Worker startup failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Load configuration, install logging and validate both built-in toolchains.
fn prepare(cli: &Cli) -> Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(&config.logging)?;
    config.check_tools()?;
    language::plan("c", &config)?;
    language::plan("python", &config)?;
    tracing::info!(listen = %config.server.listen, "Worker configuration and language plans validated");
    bail!("Worker RPC service is not implemented yet")
}
