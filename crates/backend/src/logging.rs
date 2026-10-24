//! English, single-line backend logs on stderr with startup-selected filtering.

use std::io::IsTerminal;

use anyhow::{Context, Result};

use crate::config::LoggingConfig;

/// Install the process-wide text subscriber without environment-level overrides.
pub fn init(config: &LoggingConfig) -> Result<()> {
    let level = config
        .level
        .parse::<tracing::level_filters::LevelFilter>()
        .context("Invalid logging.level")?;
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!("Failed to initialize backend logging: {error}"))
}
