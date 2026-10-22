//! Shared Kafka configuration, client policy and transport errors.

use anyhow::{Result, ensure};
use rdkafka::{error::KafkaError, types::RDKafkaErrorCode};

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

/// Retry transport outages and exhausted delivery budgets, leaving other failures explicit.
pub fn temporary(error: &KafkaError) -> bool {
    matches!(
        error.rdkafka_error_code(),
        Some(
            RDKafkaErrorCode::BrokerTransportFailure
                | RDKafkaErrorCode::Resolve
                | RDKafkaErrorCode::AllBrokersDown
                | RDKafkaErrorCode::MessageTimedOut
                | RDKafkaErrorCode::OperationTimedOut
                | RDKafkaErrorCode::QueueFull
                | RDKafkaErrorCode::RequestTimedOut
                | RDKafkaErrorCode::LeaderNotAvailable
                | RDKafkaErrorCode::NotLeaderForPartition
                | RDKafkaErrorCode::BrokerNotAvailable
                | RDKafkaErrorCode::ReplicaNotAvailable
                | RDKafkaErrorCode::NetworkException
                | RDKafkaErrorCode::NotEnoughReplicas
                | RDKafkaErrorCode::NotEnoughReplicasAfterAppend
        )
    )
}
