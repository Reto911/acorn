//! Shared Kafka configuration, defaults, validation and client policy.

use anyhow::{Result, ensure};

mod config;
pub mod defaults;

pub use config::KafkaConfig;

/// Validate a configured consumer group with the caller's field name in diagnostics.
pub fn validate_group_id(group_id: &str, field: &str) -> Result<()> {
    ensure!(
        !group_id.trim().is_empty() && !group_id.contains('\0'),
        "{field} must not be empty or contain NUL"
    );
    Ok(())
}
