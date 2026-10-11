//! English, single-line Worker logs written to stderr.

use anyhow::{Context, Result};
use tracing::level_filters::LevelFilter;

use crate::config::LoggingConfig;

/// Install the process-wide text subscriber using the configured minimum level.
pub fn init(config: &LoggingConfig) -> Result<()> {
    let level = config
        .level
        .parse::<LevelFilter>()
        .context("Invalid logging.level")?;
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!("Failed to initialize Worker logging: {error}"))
}
