//! Shared HTTP, Kafka, status RPC and Isolate assembly for acceptance.

use std::{net::SocketAddr, time::Duration};

use acorn_backend::{
    health::Health, http, llm_review::ReviewService, membership::Membership, scheduler::Scheduler,
    submissions::CreationService,
};
use acorn_worker::{config::Config as WorkerConfig, rpc::WorkerService};
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
    async fn start(box_id: u32, broker: &super::broker::Broker) -> Self {
        let mut config =
            WorkerConfig::parse(include_str!("../../../../config/worker.example.toml")).unwrap();
        config.sandbox.box_id = box_id;
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
    /// Independent Workers, each owning its explicitly selected Isolate box.
    workers: Vec<RealWorker>,
    /// Owned topic pair and initial offsets for both services.
    broker: super::broker::Broker,
    /// Actual HTTP listener address.
    address: SocketAddr,
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
    pub async fn start(box_ids: &[u32]) -> Self {
        let mut fixture = Fixture::new().await;
        let broker = super::broker::Broker::start(&mut fixture).await;
        let mut workers = Vec::new();
        for &box_id in box_ids {
            workers.push(RealWorker::start(box_id, &broker).await);
        }
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
    pub async fn submit(&self, language: &str, source: &str) -> String {
        let (status, body) = network_call(
            self.address,
            "POST",
            "/api/v1/submissions",
            Some(json!({"problem_id":1,"language_id":language,"source":source})),
            Some(Uuid::new_v4()),
        )
        .await;
        assert_eq!(status, 202, "{body}");
        body["submission_id"].as_str().unwrap().to_owned()
    }

    /// Check the HTTP-persisted verdict after the Scheduler finishes its single event handoff attempt.
    pub async fn terminal(&self, id: &str, verdict: &str) -> Value {
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
        assert_eq!(body["state"], "finished", "{body}");
        assert_eq!(body["verdict"], verdict, "{body}");
        let stored = self.fixture.database.submission(id).await.unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(stored.terminal_details).unwrap(),
            body["terminal_details"]
        );
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
