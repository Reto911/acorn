//! One bounded task publisher, retaining uncertain deliveries until progress or shutdown.

use std::{future::Future, time::Duration};

use acorn_kafka::KafkaConfig;
use acorn_protocol::records;
use anyhow::{Context, Result};
use rdkafka::{
    error::KafkaError,
    producer::{FutureProducer, FutureRecord},
};
use tokio::sync::watch;

use crate::{
    config::Config,
    database::Database,
    scheduler::Scheduler,
    submissions::{JudgingTask, SubmissionState},
};

/// Publisher owns only the active payload; admission permits remain keyed in Scheduler.
pub struct Publisher {
    /// Shared bounded queue and admission ownership.
    scheduler: Scheduler,
    /// Short state reads and conditional delivery writes.
    database: Database,
    /// Existing role-configured producer, with bounded SDK buffers and delivery time.
    producer: FutureProducer,
    /// Outgoing topic, encoding ceiling and network budgets.
    config: KafkaConfig,
    /// Existing Scheduler retry interval for uncertain deliveries.
    retry_interval: Duration,
}

impl Publisher {
    /// Construct the task publisher without starting dispatch or changing persisted work.
    pub fn new(config: &Config, scheduler: Scheduler, database: Database) -> Result<Self> {
        Ok(Self {
            producer: scheduler.kafka_observer().producer,
            scheduler,
            database,
            config: config.kafka.clone(),
            retry_interval: Duration::from_millis(config.scheduler.retry_interval_ms),
        })
    }

    /// Publish until paused or stopped; finish an already started delivery before returning.
    ///
    /// The lifecycle owner bounds overall shutdown and supervises returned errors.
    pub async fn run(self, mut stop: watch::Receiver<bool>) -> Result<()> {
        loop {
            let changed = self.scheduler.shared.changed.notified();
            let pending = {
                let mut state = self
                    .scheduler
                    .shared
                    .state
                    .lock()
                    .expect("Scheduler state lock poisoned");
                if *stop.borrow() || state.stopping || state.closed {
                    return Ok(());
                }
                state.queue.pop_front()
            };
            if let Some(pending) = pending {
                self.publish(pending.task, &mut stop).await?;
            } else {
                tokio::select! {
                    _ = changed => {},
                    _ = stop.changed() => return Ok(()),
                }
            }
        }
    }

    /// Confirm initial eligibility once, then rely on local committed progress to stop retries.
    async fn publish(&self, task: JudgingTask, stop: &mut watch::Receiver<bool>) -> Result<()> {
        let id = &task.request.job_id;
        let state = self
            .database
            .submission_state(id)
            .await?
            .context("KafkaUnknownSubmission: queued task no longer exists")?;
        if state != SubmissionState::Queued {
            if state != SubmissionState::Dispatching {
                self.scheduler.release_pending(id);
                if matches!(state, SubmissionState::Finished | SubmissionState::Failed) {
                    self.scheduler.complete(id);
                }
            }
            return Ok(());
        }
        let encoded = records::encode_task(&task.request, self.config.max_task_record_bytes)
            .with_context(|| {
                format!(
                    "KafkaTaskEncodeFailed: submission {id}, topic {}",
                    self.config.tasks_topic
                )
            })?;
        let delivered = deliver(&self.scheduler, id, self.retry_interval, stop, || async {
            self.producer.send(FutureRecord::to(&self.config.tasks_topic)
                .key(encoded.key()).payload(encoded.payload()),
                Duration::from_millis(self.config.queue_timeout_ms)).await
                .map(|delivery| {
                    tracing::info!(submission = id, topic = %self.config.tasks_topic,
                        partition = delivery.partition, offset = delivery.offset, "Kafka task delivered");
                }).map_err(|(error, _)| error)
        }).await;
        match delivered {
            Ok(true) => {
                self.database.mark_dispatching(id).await?;
            }
            Ok(false) => {}
            Err(error) => return Err(error).context("KafkaTaskDeliveryFailed"),
        }
        Ok(())
    }
}

/// Keep the same encoded send operation across retries, without cancelling an in-flight send.
async fn deliver<F: Future<Output = Result<(), KafkaError>>>(
    scheduler: &Scheduler,
    id: &str,
    retry_interval: Duration,
    stop: &mut watch::Receiver<bool>,
    mut send: impl FnMut() -> F,
) -> Result<bool, KafkaError> {
    loop {
        if *stop.borrow()
            || !scheduler.awaiting_admission(id)
            || scheduler
                .shared
                .state
                .lock()
                .expect("Scheduler state lock poisoned")
                .stopping
        {
            return Ok(false);
        }
        match send().await {
            Ok(()) => return Ok(true),
            Err(error) => {
                tracing::warn!(submission = id, error = %error, "Kafka task delivery failed");
                if !scheduler.awaiting_admission(id) {
                    return Ok(false);
                }
                if !super::temporary(&error) {
                    return Err(error);
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(retry_interval) => {},
            _ = stop.changed() => return Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Delivery windows use the production retry flow with controlled completion futures.

    use super::*;
    use crate::{llm_review::ReviewMode, submissions::JudgingRecovery};
    use acorn_protocol::worker::v1::SubmitJobRequest;
    use rdkafka::types::RDKafkaErrorCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Retry uncertain results, and observe an in-flight result before stopping on committed progress.
    #[tokio::test]
    async fn delivery_retries_uncertain_results_and_stops_on_progress() {
        let config =
            Config::parse(include_str!("../../../../../config/backend.example.toml")).unwrap();
        for (outcome, progress) in [
            (Ok(()), true),
            (
                Err(KafkaError::MessageProduction(
                    RDKafkaErrorCode::MessageTimedOut,
                )),
                true,
            ),
            (
                Err(KafkaError::MessageProduction(
                    RDKafkaErrorCode::MessageSizeTooLarge,
                )),
                true,
            ),
            (
                Err(KafkaError::MessageProduction(
                    RDKafkaErrorCode::MessageTimedOut,
                )),
                false,
            ),
        ] {
            let scheduler = Scheduler::restore(&config, JudgingRecovery::default()).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            scheduler
                .enqueue(
                    JudgingTask {
                        problem_id: 1,
                        version_no: 1,
                        review_mode: ReviewMode::Disabled,
                        request: SubmitJobRequest {
                            job_id: id.clone(),
                            ..Default::default()
                        },
                    },
                    scheduler.reserve().unwrap(),
                )
                .unwrap();
            let calls = AtomicUsize::new(0);
            let finished = AtomicUsize::new(0);
            let (_stop, mut stopping) = watch::channel(false);
            let result = deliver(
                &scheduler,
                &id,
                Duration::from_millis(1),
                &mut stopping,
                || async {
                    let attempt = calls.fetch_add(1, Ordering::SeqCst);
                    if progress {
                        scheduler.release_pending(&id);
                    }
                    tokio::task::yield_now().await;
                    finished.fetch_add(1, Ordering::SeqCst);
                    if attempt == 0 {
                        outcome.clone()
                    } else {
                        Ok(())
                    }
                },
            )
            .await
            .unwrap();
            assert_eq!(result, outcome.is_ok() || !progress);
            let expected = if progress { 1 } else { 2 };
            assert_eq!(calls.load(Ordering::SeqCst), expected);
            assert_eq!(finished.load(Ordering::SeqCst), expected);
            assert_eq!(scheduler.snapshot().pending_used, u32::from(!progress));
        }
    }
}
