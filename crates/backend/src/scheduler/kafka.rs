//! Backend task publication, event processing and role-specific Kafka client options.

mod commit;
mod events;
mod publisher;

pub use events::{EventConsumer, EventProcessor};
pub use publisher::Publisher;

use acorn_kafka::KafkaConfig;
use rdkafka::config::ClientConfig;
use rdkafka::{error::KafkaError, types::RDKafkaErrorCode};

/// Build this service's task producer using the shared transport policy.
pub fn producer_config(config: &KafkaConfig) -> ClientConfig {
    config.producer_config(config.max_task_record_bytes)
}

/// Build this service's event consumer using its configured group and shared policy.
pub fn consumer_config(config: &KafkaConfig) -> ClientConfig {
    config.consumer_config(config.max_event_record_bytes)
}

/// Retry transport outages and exhausted delivery budgets, leaving other failures explicit.
fn temporary(error: &KafkaError) -> bool {
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
