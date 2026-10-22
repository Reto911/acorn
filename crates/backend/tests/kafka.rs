//! Isolated real-broker message checks for the Kafka transport foundation.

use std::time::Duration;

use acorn_backend::{config::Config, scheduler::kafka as backend_kafka};
use acorn_protocol::{
    records::{self, EncodedRecord},
    worker::v1::{
        GetJobReply, JobFailure, JobPhase, JobState, RunLimits, SubmitJobRequest, TestCase,
        get_job_reply::Terminal,
    },
};
use acorn_worker::kafka as worker_kafka;
use anyhow::{Context, Result, ensure};
use rdkafka::{
    Message, Offset, TopicPartitionList,
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    consumer::{Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
};

/// Send and receive one record through the configured clients without a business-state write.
async fn transfer(
    producer: &FutureProducer,
    consumer: &StreamConsumer,
    topic: &str,
    record: &EncodedRecord,
    queue_timeout_ms: u64,
) -> Result<Vec<u8>> {
    producer
        .send(
            FutureRecord::to(topic)
                .partition(0)
                .key(record.key())
                .payload(record.payload()),
            Duration::from_millis(queue_timeout_ms),
        )
        .await
        .map_err(|(error, _)| error)
        .context("Deliver test record")?;
    let received = tokio::time::timeout(Duration::from_secs(10), consumer.recv()).await??;
    ensure!(
        received.key() == Some(record.key().as_bytes()),
        "Kafka key changed"
    );
    ensure!(
        received.payload() == Some(record.payload()),
        "Kafka payload changed"
    );
    Ok(received.payload().context("Missing test payload")?.to_vec())
}

/// Assign an isolated test topic explicitly; production groups and their offsets remain untouched.
fn assign_test_topic(consumer: &StreamConsumer, topic: &str) -> Result<()> {
    let mut assignment = TopicPartitionList::new();
    assignment.add_partition_offset(topic, 0, Offset::Beginning)?;
    consumer.assign(&assignment)?;
    Ok(())
}

/// Both roles exchange raw task and event messages through dedicated, subsequently deleted topics.
#[tokio::test]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and permission to create/delete isolated test topics"]
async fn real_kafka_message_roundtrip() -> Result<()> {
    let servers =
        std::env::var("ACORN_TEST_KAFKA_SERVERS").context("Set ACORN_TEST_KAFKA_SERVERS")?;
    let mut backend = Config::parse(include_str!("../../../config/backend.example.toml"))?;
    let mut worker =
        acorn_worker::config::Config::parse(include_str!("../../../config/worker.example.toml"))?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    backend.kafka.bootstrap_servers = servers.split(',').map(str::to_owned).collect();
    backend.kafka.tasks_topic = format!("acorntest.tasks.{id}");
    backend.kafka.events_topic = format!("acorntest.events.{id}");
    backend.kafka.group_id = format!("acorntest.events.{id}");
    worker
        .kafka
        .bootstrap_servers
        .clone_from(&backend.kafka.bootstrap_servers);
    worker
        .kafka
        .tasks_topic
        .clone_from(&backend.kafka.tasks_topic);
    worker
        .kafka
        .events_topic
        .clone_from(&backend.kafka.events_topic);
    worker.kafka.group_id = format!("acorntest.tasks.{id}");
    backend.validate()?;
    worker.validate()?;
    let admin: AdminClient<DefaultClientContext> =
        backend_kafka::producer_config(&backend.kafka).create()?;
    let options = AdminOptions::new()
        .operation_timeout(Some(Duration::from_secs(10)))
        .request_timeout(Some(Duration::from_secs(10)));
    let task_bytes = (backend.kafka.max_task_record_bytes + 1024).to_string();
    let event_bytes = (backend.kafka.max_event_record_bytes + 1024).to_string();
    let topics = [
        backend.kafka.tasks_topic.as_str(),
        backend.kafka.events_topic.as_str(),
    ];
    let created = admin
        .create_topics(
            &[
                NewTopic::new(topics[0], 8, TopicReplication::Fixed(1))
                    .set("max.message.bytes", &task_bytes)
                    .set("retention.ms", "86400000")
                    .set("min.insync.replicas", "1"),
                NewTopic::new(topics[1], 1, TopicReplication::Fixed(1))
                    .set("max.message.bytes", &event_bytes)
                    .set("retention.ms", "86400000")
                    .set("min.insync.replicas", "1"),
            ],
            &options,
        )
        .await?;
    let result: Result<()> = async {
        ensure!(
            created.iter().all(Result::is_ok),
            "Create test topics: {created:?}"
        );
        let task_producer: FutureProducer =
            backend_kafka::producer_config(&backend.kafka).create()?;
        let event_producer: FutureProducer =
            worker_kafka::producer_config(&worker.kafka).create()?;
        let task_consumer: StreamConsumer =
            worker_kafka::consumer_config(&worker.kafka).create()?;
        let event_consumer: StreamConsumer =
            backend_kafka::consumer_config(&backend.kafka).create()?;
        assign_test_topic(&task_consumer, topics[0])?;
        assign_test_topic(&event_consumer, topics[1])?;
        let task = SubmitJobRequest {
            job_id: uuid::Uuid::new_v4().to_string(),
            language_id: "python".into(),
            source: vec![b'#'; 65_536],
            cases: (1_u8..=127)
                .map(|index| TestCase {
                    input: vec![index; 32_768],
                    expected_output: vec![255 - index; 32_768],
                })
                .collect(),
            run_limits: Some(RunLimits {
                cpu_time_ms: 1000,
                wall_time_ms: 2000,
                memory_kib: 65536,
            }),
        };
        let encoded = records::encode_task(&task, backend.kafka.max_task_record_bytes)?;
        let bytes = transfer(
            &task_producer,
            &task_consumer,
            topics[0],
            &encoded,
            backend.kafka.queue_timeout_ms,
        )
        .await?;
        ensure!(
            records::decode_task(&bytes, worker.kafka.max_task_record_bytes)? == task,
            "Task bytes or order changed"
        );
        for phase in [JobPhase::Preparing, JobPhase::Done] {
            let event = GetJobReply {
                job_id: task.job_id.clone(),
                state: if phase == JobPhase::Done {
                    JobState::Failed
                } else {
                    JobState::Running
                }
                .into(),
                phase: phase.into(),
                completed_cases: 0,
                total_cases: 127,
                terminal: (phase == JobPhase::Done).then(|| {
                    Terminal::Failure(JobFailure {
                        code: "InvalidJob".into(),
                        diagnostic: "Test transport failure".into(),
                    })
                }),
            };
            let encoded = records::encode_event(&event, worker.kafka.max_event_record_bytes)?;
            let bytes = transfer(
                &event_producer,
                &event_consumer,
                topics[1],
                &encoded,
                worker.kafka.queue_timeout_ms,
            )
            .await?;
            ensure!(
                records::decode_event(&bytes, backend.kafka.max_event_record_bytes)? == event,
                "Event changed"
            );
        }
        Ok(())
    }
    .await;
    let cleanup = admin.delete_topics(&topics, &options).await;
    ensure!(
        cleanup
            .as_ref()
            .is_ok_and(|results| results.iter().all(Result::is_ok)),
        "Delete test topics: {cleanup:?}; transport result: {result:?}"
    );
    result
}
