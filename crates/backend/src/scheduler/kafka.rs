//! Backend Kafka client options for task production and event consumption.

use acorn_kafka::KafkaConfig;
use rdkafka::config::ClientConfig;

/// Build this service's task producer using the shared transport policy.
pub fn producer_config(config: &KafkaConfig) -> ClientConfig {
    config.producer_config(config.max_task_record_bytes)
}

/// Build this service's event consumer using its configured group and shared policy.
pub fn consumer_config(config: &KafkaConfig) -> ClientConfig {
    config.consumer_config(config.max_event_record_bytes)
}
