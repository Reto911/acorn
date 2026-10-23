//! Bounded read-only broker observations through the existing task producer.

use std::time::{Duration, Instant};

use acorn_kafka::KafkaConfig;
use anyhow::{Result, bail};
use rdkafka::{
    producer::{FutureProducer, Producer},
    types::RDKafkaErrorCode,
};

use crate::health::{ProbeError, ProbeErrorCode};

/// Shared client and configured topics; observation never joins a consumer group.
#[derive(Clone)]
pub struct Observer {
    /// The same producer used by the task publisher.
    pub(crate) producer: FutureProducer,
    /// Only the configured task and event topics are queried.
    topics: [String; 2],
}

impl Observer {
    /// Create the bounded producer once for publication and both independent collectors.
    pub(crate) fn new(config: &KafkaConfig) -> Result<Self> {
        Ok(Self {
            producer: super::producer_config(config).create()?,
            topics: [config.tasks_topic.clone(), config.events_topic.clone()],
        })
    }

    /// Query one classic consumer group without subscribing or changing its offsets.
    pub fn members(&self, group: &str, timeout: Duration) -> Result<usize> {
        let groups = self
            .producer
            .client()
            .fetch_group_list(Some(group), timeout)?;
        let Some(group) = groups.groups().iter().find(|info| info.name() == group) else {
            return Ok(0);
        };
        // The safe SDK does not expose per-group errors. A missing/unsupported
        // state must not turn such a response into an observed zero-member group.
        if !matches!(
            group.state(),
            "Empty" | "Dead" | "Stable" | "PreparingRebalance" | "CompletingRebalance"
        ) || !matches!(group.protocol_type(), "" | "consumer")
        {
            bail!(
                "WorkerMembershipInvalidGroup: state={}, protocol={}",
                group.state(),
                group.protocol_type()
            );
        }
        Ok(group.members().len())
    }

    /// Inspect both topics within one SDK-query budget; no probe records are produced.
    pub(crate) fn metadata(&self, timeout: Duration) -> Result<(), ProbeError> {
        let started = Instant::now();
        for name in &self.topics {
            let remaining = timeout
                .checked_sub(started.elapsed())
                .filter(|left| !left.is_zero())
                .ok_or_else(|| {
                    ProbeError::new(
                        ProbeErrorCode::ProbeTimeout,
                        "Kafka metadata budget exhausted",
                    )
                })?;
            let metadata = self
                .producer
                .client()
                .fetch_metadata(Some(name), remaining)
                .map_err(|error| {
                    let code = if matches!(
                        error.rdkafka_error_code(),
                        Some(
                            RDKafkaErrorCode::OperationTimedOut | RDKafkaErrorCode::RequestTimedOut
                        )
                    ) {
                        ProbeErrorCode::ProbeTimeout
                    } else {
                        ProbeErrorCode::KafkaMetadataFailed
                    };
                    ProbeError::new(code, format!("Read Kafka metadata for {name}: {error}"))
                })?;
            let usable = metadata
                .topics()
                .iter()
                .find(|topic| topic.name() == name)
                .is_some_and(|topic| {
                    topic.error().is_none()
                        && !topic.partitions().is_empty()
                        && topic
                            .partitions()
                            .iter()
                            .all(|partition| partition.error().is_none() && partition.leader() >= 0)
                });
            if !usable {
                return Err(ProbeError::new(
                    ProbeErrorCode::KafkaMetadataFailed,
                    format!("Kafka topic metadata unavailable: {name}"),
                ));
            }
        }
        Ok(())
    }
}
