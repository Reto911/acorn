//! Serial event persistence, actual offset confirmation and automatic review handoff.

use std::{future::Future, sync::Arc, time::Duration};

use acorn_protocol::records;
use anyhow::{Context, Result};
use rdkafka::{
    Message, Offset, TopicPartitionList,
    consumer::{Consumer, StreamConsumer},
    message::BorrowedMessage,
};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    config::{Config, ResultsConfig},
    database::Database,
    llm_review::ReviewService,
    scheduler::{
        Scheduler,
        validation::{self, Observation},
    },
    submissions::{SubmissionState, TerminalOutcome},
};

/// Business processing remains independent of the transport's confirmation mechanism.
pub struct EventProcessor {
    /// Local ID ownership and once-only admission release.
    scheduler: Scheduler,
    /// Fixed-version metadata and conditional machine-state writes.
    database: Database,
    /// Existing automatic review preparation and local handoff.
    review: ReviewService,
    /// Existing result and diagnostic validation bounds.
    results: ResultsConfig,
    /// Maximum encoded event payload before decoding.
    record_limit: usize,
    /// Application wait for an actual broker commit result.
    commit_timeout: Duration,
}

impl EventProcessor {
    /// Assemble event processing using the same Scheduler, pool and review service.
    pub fn new(
        config: &Config,
        scheduler: Scheduler,
        database: Database,
        review: ReviewService,
    ) -> Self {
        Self {
            scheduler,
            database,
            review,
            results: config.results.clone(),
            record_limit: config.kafka.max_event_record_bytes,
            commit_timeout: Duration::from_millis(config.kafka.commit_timeout_ms),
        }
    }

    /// Validate and persist before polling confirmation; always hand off a committed review.
    ///
    /// Confirmation must concern this record only. Errors return after local handoff,
    /// so the consumer stops without fetching or confirming a later event.
    pub async fn process(
        &self,
        payload: &[u8],
        confirmation: impl Future<Output = Result<()>>,
    ) -> Result<()> {
        let event = records::decode_event(payload, self.record_limit)?;
        Uuid::parse_str(&event.job_id).context("KafkaInvalidJobId")?;
        let id = event.job_id.clone();
        let context = self
            .database
            .judging_context(&id)
            .await?
            .context("KafkaUnknownSubmission: event references no submission")?;
        let observation = match validation::validate(
            event,
            &id,
            &context.language_id,
            context.total_cases,
            &self.results,
        ) {
            Ok(observation) => observation,
            Err(error) => {
                tracing::warn!(submission = %id, error = %error, "Invalid Worker event");
                Observation::Terminal(TerminalOutcome::local_failure(
                    "InvalidWorkerResponse",
                    &error.to_string(),
                    &self.results,
                )?)
            }
        };
        let ended = matches!(
            context.state,
            SubmissionState::Finished | SubmissionState::Failed
        );
        let terminal = ended || matches!(observation, Observation::Terminal(_));
        let prepared = if ended {
            None
        } else {
            match observation {
                Observation::Running => {
                    if context.state != SubmissionState::Running {
                        self.scheduler.mark_running(&self.database, &id).await?;
                    }
                    None
                }
                Observation::Terminal(outcome) => {
                    self.scheduler
                        .persist_terminal(
                            &self.database,
                            &self.review,
                            &id,
                            context.review_mode,
                            &outcome,
                        )
                        .await?
                }
            }
        };
        let confirmed = tokio::time::timeout(self.commit_timeout, confirmation)
            .await
            .context("KafkaEventCommitTimedOut")
            .and_then(|result| result);
        // A failed or unknown commit cannot discard the already persisted review intent.
        let handoff = match prepared {
            Some(prepared) => self.review.handoff(id.clone(), prepared).await.map(|_| ()),
            None => Ok(()),
        };
        if terminal {
            self.scheduler.complete(&id);
        }
        if let Err(error) = handoff {
            return Err(error).with_context(|| {
                format!("Review handoff failed; event confirmation: {confirmed:?}")
            });
        }
        confirmed
    }
}

/// Event consumer owns subscription and confirmation; it never joins the Worker task group.
pub struct EventConsumer {
    /// SDK consumer with explicit offsets and auto-reset disabled.
    consumer: Arc<StreamConsumer>,
    /// Shared state transition, review and commit-wait flow.
    processor: EventProcessor,
}

impl EventConsumer {
    /// Subscribe to the configured event group; deployment must initialize its offsets.
    pub fn new(
        config: &Config,
        scheduler: Scheduler,
        database: Database,
        review: ReviewService,
    ) -> Result<Self> {
        let consumer: StreamConsumer = super::consumer_config(&config.kafka).create()?;
        consumer.subscribe(&[&config.kafka.events_topic])?;
        Ok(Self {
            consumer: Arc::new(consumer),
            processor: EventProcessor::new(config, scheduler, database, review),
        })
    }

    /// Process one event at a time, finishing its persistence and handoff before observing stop.
    pub async fn run(self, mut stop: watch::Receiver<bool>) -> Result<()> {
        loop {
            if *stop.borrow() {
                return Ok(());
            }
            let received = tokio::select! {
                _ = stop.changed() => return Ok(()),
                received = self.consumer.recv() => received,
            };
            let message = match received {
                Ok(message) => message,
                Err(error) if super::temporary(&error) => {
                    tracing::warn!(error = %error, "Kafka event transport unavailable");
                    continue;
                }
                Err(error) => return Err(error).context("KafkaEventReceiveFailed"),
            };
            let topic = message.topic();
            let partition = message.partition();
            let offset = message.offset();
            let result = async {
                let payload = message
                    .payload()
                    .context("KafkaRecordDecodeFailed: missing event payload")?;
                self.processor
                    .process(payload, self.confirm(&message))
                    .await
            }
            .await;
            if let Err(error) = result {
                tracing::error!(topic, partition, offset, error = %error, "Kafka event processing failed");
                return Err(error)
                    .with_context(|| format!("Kafka event {topic}/{partition}/{offset}"));
            }
        }
    }

    /// Wait for this record's explicit commit result off the Tokio executor.
    async fn confirm(&self, message: &BorrowedMessage<'_>) -> Result<()> {
        let mut offsets = TopicPartitionList::new();
        offsets.add_partition_offset(
            message.topic(),
            message.partition(),
            Offset::Offset(message.offset() + 1),
        )?;
        let consumer = self.consumer.clone();
        let timeout = self.processor.commit_timeout;
        tokio::task::spawn_blocking(move || super::commit::confirm(&consumer, &offsets, timeout))
            .await?
    }
}
