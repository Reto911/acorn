//! Worker Kafka client options for event production and task consumption.

use acorn_kafka::KafkaConfig;
use rdkafka::config::ClientConfig;

/// Build this service's event producer using the shared transport policy.
pub fn producer_config(config: &KafkaConfig) -> ClientConfig {
    config.producer_config(config.max_event_record_bytes)
}

/// Build this service's task consumer using its configured group and shared policy.
pub fn consumer_config(config: &KafkaConfig) -> ClientConfig {
    config.consumer_config(config.max_task_record_bytes)
}
