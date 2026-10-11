//! Command-line arguments selecting the backend configuration file.

use std::path::PathBuf;

use clap::Parser;

/// Command-line options for the backend process.
#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    /// Path to the backend TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: PathBuf,
}
