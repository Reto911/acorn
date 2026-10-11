//! Command-line arguments selecting the Worker configuration file.

use std::path::PathBuf;

use clap::Parser;

/// Command-line options for the Worker process.
#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    /// Path to the Worker TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: PathBuf,
}
