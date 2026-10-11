//! Backend executable entry point.

use std::process::ExitCode;

use acorn_backend::config::Cli;
use clap::Parser;

/// Parse startup arguments and report the pending service implementation.
fn main() -> ExitCode {
    let cli = Cli::parse();
    eprintln!(
        "Backend service is not implemented yet (config: {}).",
        cli.config.display()
    );
    ExitCode::FAILURE
}
