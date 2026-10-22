//! Ordinary Kafka configuration shared by the backend and Worker.

use acorn_protocol::records::{MAX_EVENT_RECORD_BYTES, MAX_TASK_RECORD_BYTES};
use anyhow::{Result, ensure};
use rdkafka::config::ClientConfig;
use serde::Deserialize;

use crate::validate_group_id;

/// Kafka framing allowance above an encoded Protobuf payload.
const RECORD_OVERHEAD_BYTES: usize = 1024;

/// Shared TOML configuration; each service selects its message directions separately.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KafkaConfig {
    /// Bootstrap host:port endpoints, without a URL scheme.
    pub bootstrap_servers: Vec<String>,
    /// Pre-created topic carrying complete tasks.
    pub tasks_topic: String,
    /// Pre-created topic carrying running snapshots and terminal events.
    pub events_topic: String,
    /// Consumer group for this service's incoming topic.
    pub group_id: String,
    /// Single producer delivery and internal retry budget in milliseconds.
    #[serde(default = "crate::defaults::delivery_timeout_ms")]
    pub delivery_timeout_ms: u64,
    /// Maximum wait to enter the producer queue in milliseconds.
    #[serde(default = "crate::defaults::queue_timeout_ms")]
    pub queue_timeout_ms: u64,
    /// Application wait for an actual offset-commit result in milliseconds.
    #[serde(default = "crate::defaults::commit_timeout_ms")]
    pub commit_timeout_ms: u64,
    /// Total message handoff budget during normal shutdown in milliseconds.
    #[serde(default = "crate::defaults::shutdown_timeout_ms")]
    pub shutdown_timeout_ms: u64,
    /// Maximum encoded task payload, at most sixteen MiB.
    #[serde(default = "crate::defaults::max_task_record_bytes")]
    pub max_task_record_bytes: usize,
    /// Maximum encoded event payload, at most one MiB.
    #[serde(default = "crate::defaults::max_event_record_bytes")]
    pub max_event_record_bytes: usize,
}

impl KafkaConfig {
    /// Check required names and the ranges supported by the Kafka client and protocol.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.bootstrap_servers.is_empty(),
            "kafka.bootstrap_servers must not be empty"
        );
        for endpoint in &self.bootstrap_servers {
            let (host, port) = endpoint
                .rsplit_once(':')
                .ok_or_else(|| anyhow::anyhow!("Kafka endpoint must use host:port"))?;
            let valid_host = if host.starts_with('[') && host.ends_with(']') {
                host[1..host.len() - 1]
                    .parse::<std::net::Ipv6Addr>()
                    .is_ok()
            } else {
                !host.is_empty() && !host.contains([':', '/', '@', '[', ']', '?', '#'])
            };
            ensure!(
                valid_host
                    && !endpoint
                        .chars()
                        .any(|c| c.is_whitespace() || c == '\0' || c == ',')
                    && port.parse::<u16>().is_ok_and(|port| port > 0),
                "Kafka endpoint must use host:port with a positive TCP port"
            );
        }
        for topic in [&self.tasks_topic, &self.events_topic] {
            ensure!(
                !topic.is_empty()
                    && topic.len() <= 249
                    && topic != "."
                    && topic != ".."
                    && topic
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
                "Kafka topic must be a valid topic name"
            );
        }
        ensure!(
            self.tasks_topic != self.events_topic,
            "Kafka task and event topics must differ"
        );
        validate_group_id(&self.group_id, "kafka.group_id")?;
        for (name, budget) in [
            ("delivery_timeout_ms", self.delivery_timeout_ms),
            ("queue_timeout_ms", self.queue_timeout_ms),
            ("commit_timeout_ms", self.commit_timeout_ms),
            ("shutdown_timeout_ms", self.shutdown_timeout_ms),
        ] {
            ensure!(
                (1..=i32::MAX as u64).contains(&budget),
                "kafka.{name} must be positive and fit client milliseconds"
            );
        }
        ensure!(
            (1..=MAX_TASK_RECORD_BYTES).contains(&self.max_task_record_bytes),
            "kafka.max_task_record_bytes must be between 1 and 16777216"
        );
        ensure!(
            (1..=MAX_EVENT_RECORD_BYTES).contains(&self.max_event_record_bytes),
            "kafka.max_event_record_bytes must be between 1 and 1048576"
        );
        Ok(())
    }

    /// Build producer options using the role's validated outgoing record ceiling.
    pub fn producer_config(&self, record_limit: usize) -> ClientConfig {
        let bytes = record_limit + RECORD_OVERHEAD_BYTES;
        let mut config = self.base_config();
        config
            .set("enable.idempotence", "true")
            .set("acks", "all")
            .set("delivery.timeout.ms", self.delivery_timeout_ms.to_string())
            .set("message.max.bytes", bytes.to_string())
            .set(
                "queue.buffering.max.kbytes",
                (2 * bytes.div_ceil(1024)).to_string(),
            )
            .set("queue.buffering.max.messages", "128");
        config
    }

    /// Build manual-offset consumer options using the role's validated incoming ceiling.
    pub fn consumer_config(&self, record_limit: usize) -> ClientConfig {
        let bytes = record_limit + RECORD_OVERHEAD_BYTES;
        let mut config = self.base_config();
        config
            .set("group.id", &self.group_id)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", "error")
            .set("group.protocol", "classic")
            .set("message.max.bytes", bytes.to_string())
            .set("fetch.message.max.bytes", bytes.to_string())
            .set("fetch.max.bytes", bytes.to_string())
            .set("receive.message.max.bytes", (bytes + 512).to_string())
            .set(
                "queued.max.messages.kbytes",
                bytes.div_ceil(1024).to_string(),
            )
            .set("queued.min.messages", "1");
        config
    }

    /// Apply the same transport and explicit topic-creation policy to both clients.
    fn base_config(&self) -> ClientConfig {
        let mut config = ClientConfig::new();
        config
            .set("bootstrap.servers", self.bootstrap_servers.join(","))
            .set("security.protocol", "PLAINTEXT")
            .set("allow.auto.create.topics", "false");
        config
    }
}
