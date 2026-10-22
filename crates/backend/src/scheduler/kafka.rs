//! Role-specific Kafka TOML fields and task/event direction mapping.

use acorn_kafka::ClientSettings;
use anyhow::Result;
use rdkafka::config::ClientConfig;
use serde::Deserialize;

/// Kafka settings loaded and validated without opening a connection.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KafkaConfig {
    /// Bootstrap host:port endpoints, without a URL scheme.
    pub bootstrap_servers: Vec<String>,
    /// Pre-created topic carrying complete tasks.
    pub tasks_topic: String,
    /// Pre-created topic carrying running snapshots and terminal events.
    pub events_topic: String,
    /// Fixed deployment consumer group for events.
    pub events_group_id: String,
    /// Single producer delivery and internal retry budget in milliseconds.
    #[serde(default = "acorn_kafka::defaults::delivery_timeout_ms")]
    pub delivery_timeout_ms: u64,
    /// Maximum wait to enter the producer queue in milliseconds.
    #[serde(default = "acorn_kafka::defaults::queue_timeout_ms")]
    pub queue_timeout_ms: u64,
    /// Application wait for an actual offset-commit result in milliseconds.
    #[serde(default = "acorn_kafka::defaults::commit_timeout_ms")]
    pub commit_timeout_ms: u64,
    /// Total message handoff budget during normal shutdown in milliseconds.
    #[serde(default = "acorn_kafka::defaults::shutdown_timeout_ms")]
    pub shutdown_timeout_ms: u64,
    /// Maximum encoded task payload, at most sixteen MiB.
    #[serde(default = "acorn_kafka::defaults::max_task_record_bytes")]
    pub max_task_record_bytes: usize,
    /// Maximum encoded event payload, at most one MiB.
    #[serde(default = "acorn_kafka::defaults::max_event_record_bytes")]
    pub max_event_record_bytes: usize,
}

impl KafkaConfig {
    /// Apply the shared configuration contract with this role's consumer group.
    pub fn validate(&self) -> Result<()> {
        self.settings().validate()
    }

    /// Build producer options for this role's outgoing messages.
    pub fn producer_config(&self) -> ClientConfig {
        self.settings().producer_config(self.max_task_record_bytes)
    }

    /// Build consumer options for this role's incoming messages and consumer group.
    pub fn consumer_config(&self) -> ClientConfig {
        self.settings().consumer_config(self.max_event_record_bytes)
    }

    /// Borrow the existing fields for shared validation and client construction.
    fn settings(&self) -> ClientSettings<'_> {
        ClientSettings {
            bootstrap_servers: &self.bootstrap_servers,
            tasks_topic: &self.tasks_topic,
            events_topic: &self.events_topic,
            delivery_timeout_ms: self.delivery_timeout_ms,
            queue_timeout_ms: self.queue_timeout_ms,
            commit_timeout_ms: self.commit_timeout_ms,
            shutdown_timeout_ms: self.shutdown_timeout_ms,
            max_task_record_bytes: self.max_task_record_bytes,
            max_event_record_bytes: self.max_event_record_bytes,
            group_id: &self.events_group_id,
            group_field: "kafka.events_group_id",
        }
    }
}
