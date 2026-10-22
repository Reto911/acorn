//! Isolated real Kafka and MySQL coverage of the backend publication and event chain.

use std::time::{Duration, Instant};

use acorn_backend::{
    database::Database,
    llm_review::ReviewService,
    scheduler::{
        Scheduler,
        kafka::{self, EventConsumer, Publisher},
    },
    submissions::{CreateSubmission, CreationService, SubmissionState, Verdict},
};
use acorn_kafka::KafkaConfig;
use acorn_protocol::{
    records,
    worker::v1::{
        GetJobReply, JobFailure, JobPhase, JobState, JudgeResult, Verdict as RpcVerdict,
        get_job_reply::Terminal,
    },
};
use anyhow::{Context, Result, ensure};
use rdkafka::{
    Message, Offset, TopicPartitionList,
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    consumer::{CommitMode, Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
};
use tokio::{sync::watch, task::JoinSet};
use uuid::Uuid;

use crate::support::Fixture;

/// Wait for one asynchronous state transition using the real shared MySQL pool.
async fn state(database: &Database, id: &str, expected: SubmissionState) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if database.submission_state(id).await? == Some(expected) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("Backend state transition timed out")?
}

/// Observe actual committed offsets without joining the event group or changing its position.
async fn committed(config: KafkaConfig, expected: i64) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let observer: StreamConsumer = kafka::consumer_config(&config).create()?;
        let mut partition = TopicPartitionList::new();
        partition.add_partition(&config.events_topic, 0);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let offsets = observer.committed_offsets(partition.clone(), Duration::from_secs(1))?;
            if offsets
                .find_partition(&config.events_topic, 0)
                .context("Missing event partition")?
                .offset()
                == Offset::Offset(expected)
            {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "Actual event commit was not observed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    })
    .await?
}

/// Real clients publish a fixed task, persist events and confirm a duplicate terminal once processed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and ACORN_TEST_DATABASE_CONFIG for isolated resources"]
async fn real_kafka_backend_publication_and_events() -> Result<()> {
    let servers =
        std::env::var("ACORN_TEST_KAFKA_SERVERS").context("Set ACORN_TEST_KAFKA_SERVERS")?;
    let mut fixture = Fixture::new().await;
    let id = Uuid::new_v4().simple().to_string();
    fixture.config.kafka.bootstrap_servers = servers.split(',').map(str::to_owned).collect();
    fixture.config.kafka.tasks_topic = format!("acorntest.tasks.{id}");
    fixture.config.kafka.events_topic = format!("acorntest.events.{id}");
    fixture.config.kafka.group_id = format!("acorntest.events.{id}");
    let config = &fixture.config;
    let topics = [
        config.kafka.tasks_topic.as_str(),
        config.kafka.events_topic.as_str(),
    ];
    let admin: AdminClient<DefaultClientContext> =
        kafka::producer_config(&config.kafka).create()?;
    let options = AdminOptions::new()
        .operation_timeout(Some(Duration::from_secs(10)))
        .request_timeout(Some(Duration::from_secs(10)));
    let task_bytes = (config.kafka.max_task_record_bytes + 1024).to_string();
    let event_bytes = (config.kafka.max_event_record_bytes + 1024).to_string();
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
    let scheduler = Scheduler::restore(config, Default::default())?;
    let (review, review_task) = ReviewService::start(&config.llm_review, fixture.database.clone())?;
    let (stop, _) = watch::channel(false);
    let mut services = JoinSet::new();
    let result: Result<()> = async {
        ensure!(
            created.iter().all(Result::is_ok),
            "Create test topics: {created:?}"
        );
        // Initialize only this new test group; production uses auto.offset.reset=error.
        let initial_config = config.kafka.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let initial: StreamConsumer = kafka::consumer_config(&initial_config).create()?;
            let mut offsets = TopicPartitionList::new();
            offsets.add_partition_offset(&initial_config.events_topic, 0, Offset::Offset(0))?;
            initial.commit(&offsets, CommitMode::Sync)?;
            Ok(())
        })
        .await??;
        let creation = CreationService::new(config, fixture.database.clone(), scheduler.clone());
        let created = creation
            .create(CreateSubmission {
                problem_id: 1,
                language_id: "python".into(),
                source: b"raw source\r\n\t\n".to_vec(),
                request_key: Uuid::new_v4(),
            })
            .await?;
        let submission = &created.info.id;
        let original = fixture
            .database
            .queued_task(submission)
            .await?
            .context("Missing queued task")?;
        let mut task_config = config.kafka.clone();
        task_config.group_id = format!("acorntest.tasks.{id}");
        let tasks: StreamConsumer = acorn_worker::kafka::consumer_config(&task_config).create()?;
        let mut assignment = TopicPartitionList::new();
        for partition in 0..8 {
            assignment.add_partition_offset(topics[0], partition, Offset::Beginning)?;
        }
        tasks.assign(&assignment)?;
        let producer: FutureProducer =
            acorn_worker::kafka::producer_config(&config.kafka).create()?;
        let publisher = Publisher::new(config, scheduler.clone(), fixture.database.clone())?;
        let events = EventConsumer::new(
            config,
            scheduler.clone(),
            fixture.database.clone(),
            review.clone(),
        )?;
        services.spawn(publisher.run(stop.subscribe()));
        services.spawn(events.run(stop.subscribe()));
        let message = tokio::time::timeout(Duration::from_secs(10), tasks.recv()).await??;
        let task = records::decode_task(
            message.payload().context("Missing task payload")?,
            config.kafka.max_task_record_bytes,
        )?;
        ensure!(
            task == original.request,
            "Fixed task or raw bytes changed during publication"
        );
        state(&fixture.database, submission, SubmissionState::Dispatching).await?;
        ensure!(
            scheduler.snapshot().pending_used == 1,
            "Delivery confirmation released admission too early"
        );
        let total = task.cases.len() as u32;
        let mut event = GetJobReply {
            job_id: submission.clone(),
            state: JobState::Running.into(),
            phase: JobPhase::Preparing.into(),
            total_cases: total,
            ..Default::default()
        };
        let mut last_offset = 0;
        for (index, phase) in [JobPhase::Preparing, JobPhase::Done, JobPhase::Done]
            .into_iter()
            .enumerate()
        {
            event.phase = phase.into();
            if phase == JobPhase::Done {
                event.state = JobState::Finished.into();
                event.completed_cases = total;
                event.terminal = Some(Terminal::Result(JudgeResult {
                    verdict: RpcVerdict::Accepted.into(),
                    ..Default::default()
                }));
                if index == 2 {
                    // A later execution failure cannot replace the first accepted result.
                    event.state = JobState::Failed.into();
                    event.terminal = Some(Terminal::Failure(JobFailure {
                        code: "LateFailure".into(),
                        diagnostic: "Duplicate terminal".into(),
                    }));
                }
            }
            let encoded = records::encode_event(&event, config.kafka.max_event_record_bytes)?;
            let delivery = producer
                .send(
                    FutureRecord::to(topics[1])
                        .key(encoded.key())
                        .payload(encoded.payload()),
                    Duration::from_millis(config.kafka.queue_timeout_ms),
                )
                .await
                .map_err(|(error, _)| error)?;
            last_offset = delivery.offset;
            if phase == JobPhase::Preparing {
                state(&fixture.database, submission, SubmissionState::Running).await?;
                ensure!(
                    scheduler.snapshot().pending_used == 0,
                    "Running did not release admission"
                );
            }
        }
        committed(config.kafka.clone(), last_offset + 1).await?;
        let stored = fixture
            .database
            .submission(submission)
            .await?
            .context("Missing terminal")?;
        ensure!(
            stored.info.state == SubmissionState::Finished
                && stored.info.verdict == Some(Verdict::AC),
            "Duplicate terminal replaced the first result"
        );
        ensure!(
            !scheduler.contains(submission),
            "Terminal ID ownership was not completed"
        );
        Ok(())
    }
    .await;
    stop.send_replace(true);
    let drained = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(service) = services.join_next().await {
            service??;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;
    services.abort_all();
    review.stop();
    let reviewed = review_task.await;
    let cleanup = admin.delete_topics(&topics, &options).await;
    let groups = admin
        .delete_groups(&[&config.kafka.group_id], &options)
        .await;
    fixture.close().await;
    ensure!(
        cleanup
            .as_ref()
            .is_ok_and(|items| items.iter().all(Result::is_ok)),
        "Delete test topics: {cleanup:?}; pipeline: {result:?}"
    );
    ensure!(
        groups
            .as_ref()
            .is_ok_and(|items| items.iter().all(Result::is_ok)),
        "Delete test event group: {groups:?}; pipeline: {result:?}"
    );
    drained.context("Backend Kafka services did not stop")??;
    reviewed??;
    result
}
