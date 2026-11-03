//! Shared HTTP, Kafka, status RPC and Isolate assembly for acceptance.

use std::{net::SocketAddr, time::Duration};

use acorn_backend::{
    health::Health, http, llm_review::ReviewService, membership::Membership, scheduler::Scheduler,
    submissions::CreationService,
};
use acorn_protocol::{
    records,
    worker::v1::{JobState, Verdict, get_job_reply::Terminal},
};
use acorn_worker::{config::Config as WorkerConfig, rpc::WorkerService};
use base64::{Engine, prelude::BASE64_STANDARD};
use rdkafka::{
    Message, Offset, TopicPartitionList,
    consumer::{Consumer, StreamConsumer},
};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{oneshot, watch},
    task::JoinHandle,
};
use tonic::{codegen::tokio_stream::wrappers::TcpListenerStream, transport::Server};
use uuid::Uuid;

use super::{Fixture, network_call};

/// Read an explicitly reserved box ID; never choose a deployment box implicitly.
pub fn box_id(variable: &str) -> u32 {
    std::env::var(variable)
        .unwrap_or_else(|_| panic!("Choose a dedicated Isolate box ID with {variable}"))
        .parse()
        .unwrap()
}

/// Production Worker service using the actual Isolate backend at a unique endpoint.
pub struct RealWorker {
    /// Endpoint used by both the Scheduler and direct protocol observations.
    pub address: String,
    /// Stop signal for the Kafka service that owns execution cleanup.
    kafka_stop: watch::Sender<bool>,
    /// Observe execution and message cleanup before stopping status transport.
    kafka_task: JoinHandle<anyhow::Result<()>>,
    /// Stop serving after execution cleanup.
    stop: oneshot::Sender<()>,
    /// Observe transport errors during teardown.
    task: JoinHandle<Result<(), tonic::transport::Error>>,
}

impl RealWorker {
    /// Bind status transport and keep the Kafka result queue bounded to one terminal.
    async fn start(box_id: u32, checker_box_id: u32, broker: &super::broker::Broker) -> Self {
        let mut config =
            WorkerConfig::parse(include_str!("../../../../config/worker.example.toml")).unwrap();
        config.sandbox.box_id = box_id;
        config.sandbox.checker_box_id = checker_box_id;
        config.validate().unwrap();
        config.results.max_unacknowledged_results = 1;
        config.kafka = broker.config.clone();
        config.kafka.group_id = broker.task_group.clone();
        let producer = acorn_worker::kafka::producer_config(&config.kafka)
            .create()
            .unwrap();
        let service = WorkerService::new(config);
        let kafka = acorn_worker::kafka::KafkaWorker::new(service.clone(), producer).unwrap();
        let (kafka_stop, stopping) = watch::channel(false);
        let kafka_task = tokio::spawn(kafka.run(stopping));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(
            Server::builder()
                .add_service(service.clone().into_server())
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                }),
        );
        Self {
            address,
            kafka_stop,
            kafka_task,
            stop,
            task,
        }
    }

    /// Finish execution before stopping the actual transport.
    async fn close(self) {
        self.kafka_stop.send_replace(true);
        self.kafka_task.await.unwrap().unwrap();
        self.stop.send(()).unwrap();
        self.task.await.unwrap().unwrap();
    }
}

/// Real single- or multi-Worker business workflow over the shipped example database.
pub struct RealHarness {
    /// Database initialized through the formal SQL scripts.
    pub fixture: Fixture,
    /// Scheduler state used to wait until persistence and the bounded event handoff have completed.
    pub scheduler: Scheduler,
    /// Independent Workers, each owning its explicitly selected user/checker box pair.
    pub workers: Vec<RealWorker>,
    /// Owned topic pair and initial offsets for both services.
    pub broker: super::broker::Broker,
    /// Actual HTTP listener address.
    pub address: SocketAddr,
    /// Creation operations owned independently of HTTP connections.
    creation: CreationService,
    /// Real independent group observation and its bounded lifecycle.
    membership: Membership,
    /// Membership query task joined before storage teardown.
    membership_task: JoinHandle<anyhow::Result<()>>,
    /// Dispatcher task observed at teardown.
    scheduler_task: JoinHandle<anyhow::Result<()>>,
    /// Independent review lifecycle, disabled during these Isolate checks.
    review: ReviewService,
    /// Wait for local review cleanup before closing shared storage.
    review_task: JoinHandle<anyhow::Result<()>>,
    /// Shared collector exposed by the actual HTTP listener.
    health: Health,
    /// Collector joined after HTTP ends and before database closure.
    health_task: JoinHandle<anyhow::Result<()>>,
    /// Stop HTTP after all business operations finish.
    http_stop: oneshot::Sender<()>,
    /// Observe HTTP transport failures during teardown.
    http_task: JoinHandle<std::io::Result<()>>,
}

impl RealHarness {
    /// Assemble production services, keeping periodic retry outside the acceptance wait.
    pub async fn start(box_ids: &[(u32, u32)]) -> Self {
        let unique: std::collections::HashSet<_> = box_ids
            .iter()
            .flat_map(|&(user, checker)| [user, checker])
            .collect();
        assert_eq!(
            unique.len(),
            box_ids.len() * 2,
            "All real Worker box IDs must differ"
        );
        let mut fixture = Fixture::new().await;
        let broker = super::broker::Broker::start(&mut fixture).await;
        eprintln!(
            "Real acceptance resources: database={} tasks={} events={}",
            fixture.config.database.name, broker.config.tasks_topic, broker.config.events_topic
        );
        let mut workers = Vec::new();
        for &(box_id, checker_box_id) in box_ids {
            workers.push(RealWorker::start(box_id, checker_box_id, &broker).await);
        }
        broker.assigned(box_ids.len()).await;
        fixture.config.scheduler.workers = workers.iter().map(|w| w.address.clone()).collect();
        let (review, review_task) =
            ReviewService::start(&fixture.config.llm_review, fixture.database.clone()).unwrap();
        let (scheduler, scheduler_task) = Scheduler::start(
            &fixture.config,
            fixture.database.clone(),
            Default::default(),
            review.clone(),
        )
        .unwrap();
        let (membership, membership_task) = Membership::start(
            &fixture.config.worker_membership,
            scheduler.kafka_observer(),
        )
        .unwrap();
        super::wait_for(|| membership.snapshot().members.is_some_and(|count| count > 0)).await;
        let creation = CreationService::new(
            &fixture.config,
            fixture.database.clone(),
            scheduler.clone(),
            membership.clone(),
        );
        let (health, health_task) =
            Health::start(&fixture.config.health, fixture.database.clone(), &scheduler).unwrap();
        let app = http::router(
            &fixture.config,
            fixture.database.clone(),
            creation.clone(),
            scheduler.clone(),
            health.clone(),
            review.clone(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (http_stop, stopped) = oneshot::channel();
        let http_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Self {
            fixture,
            scheduler,
            workers,
            broker,
            address,
            creation,
            membership,
            membership_task,
            scheduler_task,
            review,
            review_task,
            health,
            health_task,
            http_stop,
            http_task,
        }
    }

    /// Submit a source sample through the actual HTTP listener with a fresh idempotency key.
    pub async fn submit(&self, problem_id: i64, language: &str, source: &str) -> String {
        let (status, body) = network_call(
            self.address,
            "POST",
            "/api/v1/submissions",
            Some(json!({"problem_id":problem_id,"language_id":language,"source":source})),
            Some(Uuid::new_v4()),
        )
        .await;
        assert_eq!(status, 202, "{body}");
        body["submission_id"].as_str().unwrap().to_owned()
    }

    /// Match actual Kafka terminal counts and details against MySQL and the HTTP result.
    pub async fn terminal(
        &self,
        id: &str,
        verdict: Option<&str>,
        total: u32,
        completed: u32,
    ) -> Value {
        let body = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (status, body) = network_call(
                    self.address,
                    "GET",
                    &format!("/api/v1/submissions/{id}"),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, 200, "{body}");
                if matches!(body["state"].as_str(), Some("finished" | "failed"))
                    && !self.scheduler.contains(id)
                {
                    break body;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            body["state"],
            if verdict.is_some() {
                "finished"
            } else {
                "failed"
            },
            "{body}"
        );
        assert_eq!(body["verdict"], json!(verdict), "{body}");
        let stored = self.fixture.database.submission(id).await.unwrap().unwrap();
        assert_eq!(body["state"], stored.info.state.as_str());
        assert_eq!(
            body["verdict"],
            serde_json::to_value(stored.info.verdict).unwrap()
        );
        assert_eq!(
            serde_json::to_value(stored.terminal_details).unwrap(),
            body["terminal_details"]
        );
        let consumer: StreamConsumer =
            acorn_backend::scheduler::kafka::consumer_config(&self.broker.config)
                .create()
                .unwrap();
        let mut assignment = TopicPartitionList::new();
        assignment
            .add_partition_offset(&self.broker.config.events_topic, 0, Offset::Beginning)
            .unwrap();
        consumer.assign(&assignment).unwrap();
        let event = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let message = consumer.recv().await.unwrap();
                let event = records::decode_event(
                    message.payload().unwrap(),
                    self.broker.config.max_event_record_bytes,
                )
                .unwrap();
                assert_eq!(message.key(), Some(event.job_id.as_bytes()));
                if event.job_id == id && event.terminal.is_some() {
                    break event;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(event.total_cases, total);
        assert_eq!(event.completed_cases, completed);
        match event.terminal.unwrap() {
            Terminal::Result(result) => {
                assert_eq!(event.state, JobState::Finished as i32);
                let actual = match Verdict::try_from(result.verdict).unwrap() {
                    Verdict::Accepted => "AC",
                    Verdict::WrongAnswer => "WA",
                    Verdict::CompileError => "CE",
                    Verdict::RuntimeError => "RE",
                    Verdict::TimeLimitExceeded => "TLE",
                    Verdict::MemoryLimitExceeded => "MLE",
                    Verdict::OutputLimitExceeded => "OLE",
                    Verdict::Unspecified => panic!("Missing actual verdict"),
                };
                assert_eq!(Some(actual), verdict);
                if let Some(case) = result.failed_case {
                    let details: acorn_backend::submissions::CaseDetails =
                        serde_json::from_value(body["terminal_details"].clone()).unwrap();
                    let saved = details.failed_case;
                    assert_eq!(saved.case_index, case.case_index);
                    assert_eq!(saved.stdout_base64, BASE64_STANDARD.encode(case.stdout));
                    assert_eq!(saved.stderr_base64, BASE64_STANDARD.encode(case.stderr));
                    assert_eq!(saved.stdout_truncated, case.stdout_truncated);
                    assert_eq!(saved.stderr_truncated, case.stderr_truncated);
                    assert_eq!(
                        saved.resources.map(|usage| (
                            usage.cpu_time_us,
                            usage.wall_time_us,
                            usage.peak_memory_bytes
                        )),
                        case.resources.map(|usage| (
                            usage.cpu_time_us,
                            usage.wall_time_us,
                            usage.peak_memory_bytes
                        )),
                    );
                    let exit = saved
                        .exit_code
                        .map(acorn_protocol::worker::v1::failed_case::Exit::ExitCode)
                        .or_else(|| {
                            saved
                                .signal
                                .map(acorn_protocol::worker::v1::failed_case::Exit::Signal)
                        });
                    assert_eq!(exit, case.exit);
                } else if result.verdict == Verdict::CompileError as i32 {
                    assert_eq!(
                        body["terminal_details"]["compile_diagnostic_base64"],
                        BASE64_STANDARD.encode(result.compile_diagnostic)
                    );
                    assert_eq!(
                        body["terminal_details"]["compile_diagnostic_truncated"],
                        result.compile_diagnostic_truncated
                    );
                } else {
                    assert!(body["terminal_details"].is_null());
                }
            }
            Terminal::Failure(failure) => {
                assert_eq!(event.state, JobState::Failed as i32);
                assert!(verdict.is_none());
                assert_eq!(
                    body["terminal_details"],
                    json!({"code":failure.code,"diagnostic":failure.diagnostic})
                );
            }
        }
        tokio::task::spawn_blocking(move || drop(consumer))
            .await
            .unwrap();
        body
    }

    /// Drain creation, persistence and event handoff before stopping transports and the database.
    pub async fn close(self) {
        self.creation.stop();
        self.membership.stop();
        self.scheduler.pause();
        self.review.stop();
        self.creation.wait_idle().await;
        self.scheduler.shutdown();
        self.scheduler_task.await.unwrap().unwrap();
        self.review_task.await.unwrap().unwrap();
        self.http_stop.send(()).unwrap();
        self.http_task.await.unwrap().unwrap();
        self.health.stop();
        self.health_task.await.unwrap().unwrap();
        self.membership_task.await.unwrap().unwrap();
        for worker in self.workers {
            worker.close().await;
        }
        self.broker.close().await;
        self.fixture.close().await;
    }
}
