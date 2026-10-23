//! Owned Kafka resources for process lifecycle and real Worker acceptance checks.

use super::Fixture;
use acorn_backend::scheduler::kafka;
use acorn_kafka::KafkaConfig;
use acorn_protocol::{records, worker::v1::GetJobReply};
use rdkafka::{
    Offset, TopicPartitionList,
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    consumer::{CommitMode, Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
};
use std::time::Duration;
use uuid::Uuid;

/// A dedicated topic pair and initialized groups; no deployment resources are touched.
pub struct Broker {
    /// Backend role configuration copied into temporary process deployments.
    pub config: KafkaConfig,
    /// Shared task group used by the test Workers.
    pub task_group: String,
    /// Admin owner used for resource cleanup.
    admin: AdminClient<DefaultClientContext>,
    /// Bounded topic and group operations.
    options: AdminOptions,
    /// Producer used for explicit controlled events.
    producer: FutureProducer,
}

impl Broker {
    /// Create isolated topics and initialize both groups at their beginning.
    pub async fn start(fixture: &mut Fixture) -> Self {
        let id = Uuid::new_v4().simple().to_string();
        let config = &mut fixture.config.kafka;
        config.bootstrap_servers = std::env::var("ACORN_TEST_KAFKA_SERVERS")
            .expect("Set ACORN_TEST_KAFKA_SERVERS for process lifecycle tests")
            .split(',')
            .map(str::to_owned)
            .collect();
        config.tasks_topic = format!("acorntest.tasks.{id}");
        config.events_topic = format!("acorntest.events.{id}");
        config.group_id = format!("acorntest.events.{id}");
        let task_group = format!("acorntest.tasks.{id}");
        let admin: AdminClient<DefaultClientContext> =
            kafka::producer_config(config).create().unwrap();
        let options = AdminOptions::new()
            .operation_timeout(Some(Duration::from_secs(10)))
            .request_timeout(Some(Duration::from_secs(10)));
        let task_bytes = (config.max_task_record_bytes + 1024).to_string();
        let event_bytes = (config.max_event_record_bytes + 1024).to_string();
        let created = admin
            .create_topics(
                &[
                    NewTopic::new(&config.tasks_topic, 8, TopicReplication::Fixed(1))
                        .set("max.message.bytes", &task_bytes),
                    NewTopic::new(&config.events_topic, 1, TopicReplication::Fixed(1))
                        .set("max.message.bytes", &event_bytes),
                ],
                &options,
            )
            .await
            .unwrap();
        assert!(created.iter().all(Result::is_ok), "{created:?}");
        let initial = config.clone();
        let group = task_group.clone();
        tokio::task::spawn_blocking(move || {
            for (topic, group, count) in [
                (&initial.events_topic, &initial.group_id, 1),
                (&initial.tasks_topic, &group, 8),
            ] {
                let mut config = initial.clone();
                config.group_id = group.clone();
                let consumer: StreamConsumer = kafka::consumer_config(&config).create().unwrap();
                let mut offsets = TopicPartitionList::new();
                for partition in 0..count {
                    offsets
                        .add_partition_offset(topic, partition, Offset::Offset(0))
                        .unwrap();
                }
                consumer.commit(&offsets, CommitMode::Sync).unwrap();
            }
        })
        .await
        .unwrap();
        let producer = kafka::producer_config(config).create().unwrap();
        Self {
            config: config.clone(),
            task_group,
            admin,
            options,
            producer,
        }
    }

    /// Publish one controlled event and await its broker delivery acknowledgement.
    pub async fn event(&self, reply: &GetJobReply) {
        let encoded = records::encode_event(reply, self.config.max_event_record_bytes).unwrap();
        self.producer
            .send(
                FutureRecord::to(&self.config.events_topic)
                    .key(encoded.key())
                    .payload(encoded.payload()),
                Duration::from_millis(self.config.queue_timeout_ms),
            )
            .await
            .unwrap();
    }

    /// Assign an independent task observer without joining or changing the Worker group.
    pub fn tasks(&self) -> StreamConsumer {
        let mut config = self.config.clone();
        config.group_id = self.task_group.clone();
        let consumer: StreamConsumer = acorn_worker::kafka::consumer_config(&config)
            .create()
            .unwrap();
        let mut offsets = TopicPartitionList::new();
        for partition in 0..8 {
            offsets
                .add_partition_offset(&config.tasks_topic, partition, Offset::Beginning)
                .unwrap();
        }
        consumer.assign(&offsets).unwrap();
        consumer
    }

    /// Delete only this fixture's topics and groups after all participating services stop.
    pub async fn close(self) {
        let deleted = self
            .admin
            .delete_topics(
                &[&self.config.tasks_topic, &self.config.events_topic],
                &self.options,
            )
            .await
            .unwrap();
        assert!(deleted.iter().all(Result::is_ok), "{deleted:?}");
        let deleted = self
            .admin
            .delete_groups(&[&self.config.group_id, &self.task_group], &self.options)
            .await
            .unwrap();
        assert!(deleted.iter().all(Result::is_ok), "{deleted:?}");
    }
}
