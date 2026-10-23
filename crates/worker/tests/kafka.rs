//! Isolated Kafka contracts using existing controlled managers and an optional real Isolate box.

mod support;

use acorn_protocol::{
    records,
    worker::v1::{
        GetJobReply, GetWorkerStatusRequest, JobPhase, JobState, SubmitJobRequest, Verdict,
        WorkerExecutionState, get_job_reply::Terminal,
    },
};
use acorn_worker::{
    config::Config,
    kafka::{self, KafkaWorker},
    rpc::{JudgeWorker, WorkerService},
};
use anyhow::{Context, Result, ensure};
use rdkafka::{
    Message, Offset, TopicPartitionList,
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    client::DefaultClientContext,
    consumer::{CommitMode, Consumer, StreamConsumer},
    producer::{FutureProducer, FutureRecord},
};
use std::{fs, sync::Arc, time::Duration};
use support::{Fixture, job};
use tokio::{
    sync::{oneshot, watch},
    task::JoinHandle,
    time::Instant,
};
use tonic::Request;

/// Test-owned topics, initial group position and an observer that never joins the task group.
struct Broker {
    /// Validated Worker deployment using dedicated Kafka resource names.
    config: Config,
    /// Admin client for explicit creation and deletion.
    admin: AdminClient<DefaultClientContext>,
    /// Finite administrative operation budgets.
    options: AdminOptions,
    /// Test task publisher.
    producer: FutureProducer,
    /// Explicitly assigned event reader, also used to query committed task positions.
    events: Arc<StreamConsumer>,
}

impl Broker {
    /// Create a small deterministic test topic pair and initialize only the new task group.
    async fn new(mut config: Config, suffix: &str) -> Result<Self> {
        config.kafka.bootstrap_servers = std::env::var("ACORN_TEST_KAFKA_SERVERS")
            .context("Set ACORN_TEST_KAFKA_SERVERS")?
            .split(',')
            .map(str::to_owned)
            .collect();
        config.kafka.tasks_topic =
            format!("acorntest.worker.tasks.{}.{suffix}", std::process::id());
        config.kafka.events_topic =
            format!("acorntest.worker.events.{}.{suffix}", std::process::id());
        config.kafka.group_id = format!("acorntest.worker.group.{}.{suffix}", std::process::id());
        config.validate()?;
        let admin: AdminClient<DefaultClientContext> =
            kafka::producer_config(&config.kafka).create()?;
        let options = AdminOptions::new()
            .operation_timeout(Some(Duration::from_secs(10)))
            .request_timeout(Some(Duration::from_secs(10)));
        let created = admin
            .create_topics(
                &[
                    NewTopic::new(&config.kafka.tasks_topic, 1, TopicReplication::Fixed(1))
                        .set("retention.ms", "86400000")
                        .set("min.insync.replicas", "1")
                        .set(
                            "max.message.bytes",
                            &(config.kafka.max_task_record_bytes + 1024).to_string(),
                        ),
                    NewTopic::new(&config.kafka.events_topic, 1, TopicReplication::Fixed(1))
                        .set("retention.ms", "86400000")
                        .set("min.insync.replicas", "1")
                        .set(
                            "max.message.bytes",
                            &(config.kafka.max_event_record_bytes + 1024).to_string(),
                        ),
                ],
                &options,
            )
            .await?;
        ensure!(
            created.iter().all(Result::is_ok),
            "Create test topics: {created:?}"
        );
        let producer = kafka::producer_config(&config.kafka).create()?;
        let events: StreamConsumer = config
            .kafka
            .consumer_config(config.kafka.max_event_record_bytes)
            .create()?;
        let mut initial = TopicPartitionList::new();
        initial.add_partition_offset(&config.kafka.tasks_topic, 0, Offset::Offset(0))?;
        // First deployment initialization, never a business-group reset.
        tokio::task::spawn_blocking({
            let kafka = config.kafka.clone();
            move || -> Result<()> {
                let initial_consumer: StreamConsumer = kafka::consumer_config(&kafka).create()?;
                initial_consumer.commit(&initial, CommitMode::Sync)?;
                Ok(())
            }
        })
        .await??;
        let mut assignment = TopicPartitionList::new();
        assignment.add_partition_offset(&config.kafka.events_topic, 0, Offset::Beginning)?;
        events.assign(&assignment)?;
        Ok(Self {
            config,
            admin,
            options,
            producer,
            events: Arc::new(events),
        })
    }

    /// Deliver one small task to the only test partition.
    async fn task(&self, task: SubmitJobRequest) -> Result<()> {
        let encoded = records::encode_task(&task, self.config.kafka.max_task_record_bytes)?;
        self.producer
            .send(
                FutureRecord::to(&self.config.kafka.tasks_topic)
                    .partition(0)
                    .key(encoded.key())
                    .payload(encoded.payload()),
                Duration::from_secs(1),
            )
            .await
            .map_err(|(error, _)| error)?;
        Ok(())
    }

    /// Read the next matching bounded event while continuing to drive the event observer.
    async fn event(&self, id: &str, terminal: bool) -> Result<GetJobReply> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let message = self.events.recv().await?;
                let reply = records::decode_event(
                    message.payload().context("Missing event payload")?,
                    self.config.kafka.max_event_record_bytes,
                )?;
                if reply.job_id == id && (reply.state != JobState::Running as i32) == terminal {
                    return Ok(reply);
                }
            }
        })
        .await
        .context("Worker event timed out")?
    }

    /// Query persistent group offsets off the async runtime without joining the group.
    async fn offset(&self) -> Result<Offset> {
        let events = self.events.clone();
        let mut partition = TopicPartitionList::new();
        partition.add_partition(&self.config.kafka.tasks_topic, 0);
        tokio::task::spawn_blocking(move || {
            let offsets = events.committed_offsets(partition, Duration::from_secs(2))?;
            Ok(offsets.elements()[0].offset())
        })
        .await?
    }

    /// Wait for the expected actual commit after the terminal event was delivered.
    async fn committed(&self, expected: i64) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self.offset().await? != Offset::Offset(expected) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok(())
        })
        .await
        .context("Worker task commit timed out")?
    }

    /// Remove only the isolated topics and group after all Worker services have closed.
    async fn close(self) -> Result<()> {
        drop(self.events);
        let topics = self
            .admin
            .delete_topics(
                &[
                    &self.config.kafka.tasks_topic,
                    &self.config.kafka.events_topic,
                ],
                &self.options,
            )
            .await?;
        let groups = self
            .admin
            .delete_groups(&[&self.config.kafka.group_id], &self.options)
            .await?;
        ensure!(
            topics.iter().all(Result::is_ok),
            "Delete test topics: {topics:?}"
        );
        ensure!(
            groups.iter().all(Result::is_ok),
            "Delete test group: {groups:?}"
        );
        Ok(())
    }
}

/// A Kafka service and the shared handle used by its retained status RPC.
struct Running {
    /// Status and common execution-stop handle.
    worker: WorkerService,
    /// Signal initiating the service's normal bounded shutdown.
    stop: watch::Sender<bool>,
    /// Observe service errors and actual close completion.
    task: JoinHandle<Result<()>>,
}

impl Running {
    /// Start only the callable Kafka service; the shipped binary still uses the old entry point.
    fn start(config: Config) -> Result<Self> {
        let producer = kafka::producer_config(&config.kafka).create()?;
        Self::with_producer(config, producer)
    }

    /// Keep task polling on the real broker while controlling only result publication.
    fn with_producer(config: Config, producer: FutureProducer) -> Result<Self> {
        let worker = WorkerService::new(config);
        let service = KafkaWorker::new(worker.clone(), producer)?;
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(service.run(stopped));
        Ok(Self { worker, stop, task })
    }

    /// Request stop and wait for the shared sandbox and total message budgets.
    async fn close(self) -> Result<()> {
        self.stop.send_replace(true);
        tokio::time::timeout(Duration::from_secs(15), self.task)
            .await
            .context("Worker service did not stop")??
    }
}

/// Capacity one reserves the future terminal, resumes after commit, and rejects bad content safely.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and isolated topic/group administration"]
async fn real_kafka_worker_capacity_and_shutdown() -> Result<()> {
    let mut fixture = Fixture::new("normal");
    fixture.config.results.max_unacknowledged_results = 1;
    let broker = Broker::new(
        fixture.config.clone(),
        fixture
            .directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .await?;
    let running = Running::start(broker.config.clone())?;
    let mut observer = None;
    let result: Result<()> = async {
        let release = fixture.directory.path().join("release");
        let source = format!("import pathlib,time\np=pathlib.Path({:?})\nwhile not p.exists(): time.sleep(.01)\nprint(42)", release.to_str().unwrap());
        broker.task(job("first", "python", &source, &[(b"", b"42")])).await?;
        broker.event("first", false).await?;
        broker.task(job("second", "python", "print(42)", &[(b"", b"42")])).await?;
        ensure!(broker.offset().await? == Offset::Offset(0), "Active task committed early");
        let status = running.worker.get_worker_status(Request::new(GetWorkerStatusRequest {})).await?.into_inner();
        ensure!(status.execution_state == WorkerExecutionState::Running as i32 && status.unacknowledged_results == 0, "Unexpected active snapshot: {status:?}");
        // An eager rebalance revokes the active execution. A passive member takes
        // the only partition, so the old execution cannot hide an erroneous commit.
        let passive: StreamConsumer = kafka::consumer_config(&broker.config.kafka)
            .set("client.id", "000-passive-worker-test")
            .set("partition.assignment.strategy", "range")
            .create()?;
        passive.subscribe(&[&broker.config.kafka.tasks_topic])?;
        let (park, mut parked) = oneshot::channel();
        let (leave, mut leaving) = watch::channel(false);
        let task: JoinHandle<Result<()>> = tokio::spawn(async move {
            let mut park = Some(park);
            loop {
                tokio::select! {
                    _ = leaving.changed() => break,
                    message = passive.recv() => {
                        let message = message?;
                        if let Some(park) = park.take() { let _ = park.send(message.offset()); }
                    }
                }
            }
            passive.unsubscribe();
            tokio::task::spawn_blocking(move || drop(passive)).await?;
            Ok(())
        });
        observer = Some((leave, task));
        ensure!(tokio::time::timeout(Duration::from_secs(15), &mut parked).await?? == 0, "Passive member did not obtain the uncommitted task");
        fs::write(&release, "go")?;
        let original = broker.event("first", true).await?;
        ensure!(original.state == JobState::Finished as i32, "Revocation stopped the active execution");
        ensure!(broker.offset().await? == Offset::Offset(0), "Revoked execution committed its old position");
        let (leave, task) = observer.take().unwrap();
        leave.send_replace(true);
        task.await??;
        // The same partition is assigned back; its record must execute under the new lease.
        for id in ["first", "second"] {
            let terminal = broker.event(id, true).await?;
            ensure!(matches!(terminal.terminal, Some(Terminal::Result(result)) if result.verdict == Verdict::Accepted as i32), "Expected AC");
        }
        broker.committed(2).await?;
        broker.task(job("invalid", "unknown", "print(42)", &[(b"", b"42")])).await?;
        let invalid = broker.event("invalid", true).await?;
        ensure!(matches!(invalid.terminal, Some(Terminal::Failure(failure)) if failure.code == "UnsupportedLanguage"), "Invalid task was not rejected");
        broker.committed(3).await?;
        ensure!(fixture.operations().lines().filter(|line| line.contains("--init")).count() == 3, "Revoked record was not reexecuted or invalid task executed");
        broker.task(job("shutdown", "python", "import time;time.sleep(30)", &[(b"", b"42")])).await?;
        broker.event("shutdown", false).await?;
        running.stop.send_replace(true);
        let terminal = broker.event("shutdown", true).await?;
        ensure!(matches!(terminal.terminal, Some(Terminal::Failure(failure)) if failure.code == "WorkerShutdown"), "Shutdown did not use the common stop path");
        broker.committed(4).await?;
        Ok(())
    }.await;
    if let Some((leave, task)) = observer {
        leave.send_replace(true);
        task.await??;
    }
    let stopped = running.close().await;
    let cleaned = broker.close().await;
    result.and(stopped).and(cleaned)
}

/// A blocked Worker leaves the group, retains its status RPC and publishes its failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and isolated topic/group administration"]
async fn real_kafka_blocked_worker_leaves_group_for_replacement() -> Result<()> {
    let fixture = Fixture::new("manager_failure");
    let replacement = Fixture::new("normal");
    let broker = Broker::new(
        fixture.config.clone(),
        fixture
            .directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .await?;
    let blocked = Running::start(broker.config.clone())?;
    let mut next = None;
    let result: Result<()> = async {
        broker.task(job("blocked", "python", "print(42)", &[(b"", b"42")])).await?;
        let failure = broker.event("blocked", true).await?;
        ensure!(matches!(failure.terminal, Some(Terminal::Failure(failure)) if failure.code == "StopUnconfirmed"), "Expected StopUnconfirmed");
        ensure!(broker.offset().await? == Offset::Offset(0), "Blocked Worker committed its task");
        let status = blocked.worker.get_worker_status(Request::new(GetWorkerStatusRequest {})).await?.into_inner();
        ensure!(status.execution_state == WorkerExecutionState::Blocked as i32, "Status RPC did not retain BLOCKED");
        let mut config = replacement.config.clone();
        config.kafka = broker.config.kafka.clone();
        next = Some(Running::start(config)?);
        let terminal = broker.event("blocked", true).await?;
        ensure!(matches!(terminal.terminal, Some(Terminal::Result(result)) if result.verdict == Verdict::Accepted as i32), "Replacement did not execute the uncommitted task");
        broker.committed(1).await?;
        ensure!(!blocked.task.is_finished(), "Expected consumer close terminated the blocked Worker service");
        Ok(())
    }.await;
    let blocked_stop = blocked.close().await;
    let replacement_stop = if let Some(next) = next {
        next.close().await
    } else {
        Ok(())
    };
    let cleaned = broker.close().await;
    let error = blocked_stop.expect_err("Blocked execution stop must remain an error");
    ensure!(
        format!("{error:#}").contains("execution stop remains unconfirmed"),
        "Unexpected blocked shutdown error: {error:#}"
    );
    result.and(replacement_stop).and(cleaned)
}

/// One real Isolate execution crosses the complete Worker Kafka boundary.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and a prepared dedicated ACORN_ISOLATE_BOX_ID"]
async fn real_kafka_isolate_execution() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut config = Config::parse(include_str!("../../../config/worker.example.toml"))?;
    config.sandbox.box_id = std::env::var("ACORN_ISOLATE_BOX_ID")
        .context("Set a dedicated unused Isolate box ID")?
        .parse()?;
    let broker = Broker::new(
        config,
        directory.path().file_name().unwrap().to_str().unwrap(),
    )
    .await?;
    let running = Running::start(broker.config.clone())?;
    let result: Result<()> = async {
        broker.task(job("real", "python", "a,b=map(int,input().split());print(a+b)", &[(b"20 22\n", b"42")])).await?;
        let terminal = broker.event("real", true).await?;
        ensure!(terminal.phase == JobPhase::Done as i32 && terminal.completed_cases == 1, "Invalid terminal progress");
        ensure!(matches!(terminal.terminal, Some(Terminal::Result(result)) if result.verdict == Verdict::Accepted as i32), "Real Isolate execution did not return AC");
        broker.committed(1).await?;
        Ok(())
    }.await;
    let stopped = running.close().await;
    let cleaned = broker.close().await;
    result.and(stopped).and(cleaned)
}

/// A full result FIFO stops admission and shares one shutdown budget across all pending results.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and isolated topic/group administration"]
async fn real_kafka_result_backpressure_and_total_shutdown_budget() -> Result<()> {
    let mut fixture = Fixture::new("normal");
    fixture.config.results.max_unacknowledged_results = 2;
    fixture.config.kafka.delivery_timeout_ms = 1_000;
    fixture.config.kafka.shutdown_timeout_ms = 500;
    let broker = Broker::new(
        fixture.config.clone(),
        fixture
            .directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .await?;
    // Only the event producer uses a refused test endpoint. The real task broker
    // remains available and no broker or business-client settings are changed.
    let producer = kafka::producer_config(&broker.config.kafka)
        .set("bootstrap.servers", "127.0.0.1:1")
        .create()?;
    let running = Running::with_producer(broker.config.clone(), producer)?;
    let result: Result<()> = async {
        for id in ["buffered-first", "buffered-second", "unadmitted-third"] {
            broker
                .task(job(id, "python", "print(42)", &[(b"", b"42")]))
                .await?;
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let status = running
                    .worker
                    .get_worker_status(Request::new(GetWorkerStatusRequest {}))
                    .await?
                    .into_inner();
                if status.unacknowledged_results == 2
                    && status.execution_state == WorkerExecutionState::Idle as i32
                {
                    return Ok::<(), anyhow::Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("Result buffer did not fill")??;
        ensure!(
            fixture
                .operations()
                .lines()
                .filter(|line| line.contains("--init"))
                .count()
                == 2,
            "Full result queue admitted another task"
        );
        ensure!(
            broker.offset().await? == Offset::Offset(0),
            "Unpublished results committed task positions"
        );
        Ok(())
    }
    .await;
    let started = Instant::now();
    let stopped = running.close().await;
    let elapsed = started.elapsed();
    let position = broker.offset().await;
    let cleaned = broker.close().await;
    result?;
    let error = stopped.expect_err("Unfinished result handoff must report shutdown failure");
    ensure!(
        format!("{error:#}").contains("KafkaWorkerShutdownTimedOut"),
        "Wrong shutdown failure: {error:#}"
    );
    ensure!(
        elapsed < Duration::from_secs(2),
        "Each result received a fresh shutdown budget: {elapsed:?}"
    );
    ensure!(
        position? == Offset::Offset(0),
        "Shutdown committed unfinished records"
    );
    cleaned
}

/// The executable consumes Kafka tasks, serves status and uses the common stop path on SIGTERM.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ACORN_TEST_KAFKA_SERVERS and isolated topic/group administration"]
async fn real_kafka_executable_handles_sigterm() -> Result<()> {
    use acorn_protocol::worker::v1::judge_worker_client::JudgeWorkerClient;
    let fixture = Fixture::new("normal");
    let broker = Broker::new(
        fixture.config.clone(),
        fixture
            .directory
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .await?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let mut config: toml::Value =
        toml::from_str(include_str!("../../../config/worker.example.toml"))?;
    config["server"]["listen"] = address.to_string().into();
    config["sandbox"]["executable"] = fixture.config.sandbox.executable.to_str().unwrap().into();
    config["kafka"]["bootstrap_servers"] = toml::Value::Array(
        broker
            .config
            .kafka
            .bootstrap_servers
            .iter()
            .cloned()
            .map(Into::into)
            .collect(),
    );
    config["kafka"]["tasks_topic"] = broker.config.kafka.tasks_topic.clone().into();
    config["kafka"]["events_topic"] = broker.config.kafka.events_topic.clone().into();
    config["kafka"]["group_id"] = broker.config.kafka.group_id.clone().into();
    let path = fixture.directory.path().join("worker.toml");
    fs::write(&path, toml::to_string(&config)?)?;
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_acorn-worker"))
        .arg("--config")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let result: Result<()> = async {
        let mut client = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(client) = JudgeWorkerClient::connect(format!("http://{address}")).await { return client; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await?;
        let ready = fixture.directory.path().join("ready");
        let source = format!("import pathlib,time\npathlib.Path({:?}).write_text('ready')\ntime.sleep(30)", ready.to_str().unwrap());
        broker.task(job("signal", "python", &source, &[(b"", b"42")])).await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !ready.exists() { tokio::time::sleep(Duration::from_millis(10)).await; }
        }).await?;
        let status = client.get_worker_status(GetWorkerStatusRequest {}).await?.into_inner();
        ensure!(status.execution_state == WorkerExecutionState::Running as i32, "Status RPC lost active execution");
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(child.id().unwrap() as i32), nix::sys::signal::Signal::SIGTERM)?;
        let terminal = broker.event("signal", true).await?;
        ensure!(matches!(terminal.terminal, Some(Terminal::Failure(failure)) if failure.code == "WorkerShutdown"), "SIGTERM did not use the execution stop path");
        Ok(())
    }.await;
    let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output()).await??;
    let logs = String::from_utf8_lossy(&output.stderr);
    ensure!(
        output.status.success() && logs.contains("Worker stopped"),
        "Executable failed: {logs}; scenario: {result:?}"
    );
    ensure!(
        fixture.operations().matches("--cleanup").count() == 1,
        "Execution was not cleaned exactly once"
    );
    broker.close().await?;
    result
}
