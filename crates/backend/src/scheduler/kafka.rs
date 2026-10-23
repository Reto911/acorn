//! Backend task publication, event processing and role-specific Kafka client options.

mod events;
mod observation;
mod publisher;

pub use events::{EventConsumer, EventProcessor};
pub use observation::Observer;
pub use publisher::Publisher;

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

pub(crate) use acorn_kafka::temporary;
