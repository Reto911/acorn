//! One selected Job's admission, polling, conditional persistence and single ACK.

use std::time::Duration;

use acorn_protocol::worker::v1::JobFailure;

use super::{
    Pending, Scheduler,
    worker_client::{Error, ErrorKind, Observation, WorkerClient},
};
use crate::{
    config::Config,
    database::Database,
    llm_review::{PreparedReview, ReviewService},
    submissions::TerminalOutcome,
};

/// Completion event for the dispatcher, retaining capacity only on explicit refusal.
pub(super) struct Completion {
    /// Task to return to the queue head while its ID retains the original permit.
    pub deferred: Option<Pending>,
    /// Temporarily defer the endpoint after refusal or a failed query/submit.
    pub cooldown: bool,
    /// Committed automatic intent, handed off only after dispatcher node release.
    pub review: Option<PreparedReview>,
}

/// Follow one Job at one address, never resubmitting ambiguous or admitted work.
pub(super) async fn track(
    pending: Pending,
    client: &WorkerClient,
    address: &str,
    database: &Database,
    config: &Config,
    review: &ReviewService,
    scheduler: &Scheduler,
) -> anyhow::Result<Completion> {
    let id = pending.task.request.job_id.clone();
    if !database.mark_dispatching(&id).await? {
        return Ok(Completion {
            deferred: None,
            cooldown: false,
            review: None,
        });
    }
    tracing::info!(submission = %id, worker = address, problem = pending.task.problem_id, language = %pending.task.request.language_id, "Dispatching submission");
    let first = match client.submit(pending.task.request.clone()).await {
        Ok(()) => None,
        Err(error) if error.kind == ErrorKind::Deferred => {
            tracing::warn!(submission = %id, worker = address, error = %error, "Worker refused admission; returning task to queue");
            let requeued = database.requeue(&id).await?;
            return Ok(Completion {
                deferred: requeued.then_some(pending),
                cooldown: true,
                review: None,
            });
        }
        Err(error) if error.kind == ErrorKind::Unavailable => {
            tracing::warn!(submission = %id, worker = address, error = %error, "Submission outcome unknown; querying once");
            Some(
                client
                    .get(
                        &id,
                        &pending.task.request.language_id,
                        pending.task.request.cases.len() as u32,
                    )
                    .await,
            )
        }
        Err(error) => {
            save_failure(scheduler, database, &id, address, &error, config).await?;
            return Ok(Completion {
                deferred: None,
                cooldown: error.kind != ErrorKind::Rejected,
                review: None,
            });
        }
    };
    let language = pending.task.request.language_id;
    let total = pending.task.request.cases.len() as u32;
    let review_mode = pending.task.review_mode;
    drop(pending.task.request.source);
    drop(pending.task.request.cases);
    if !matches!(first, Some(Err(_))) {
        scheduler.mark_running(database, &id).await?;
    }
    let mut observation = first;
    loop {
        let current = match observation.take() {
            Some(current) => current,
            None => client.get(&id, &language, total).await,
        };
        match current {
            Ok(Observation::Running) => {
                tokio::time::sleep(Duration::from_millis(config.scheduler.poll_interval_ms)).await
            }
            Ok(Observation::Terminal(terminal)) => {
                let prepared = review.prepare_automatic(review_mode, &terminal)?;
                let saved = scheduler
                    .save_terminal(
                        database,
                        &id,
                        &terminal,
                        prepared.as_ref().map(|review| &review.attempt),
                    )
                    .await?;
                let prepared = if saved {
                    prepared
                } else {
                    drop(prepared);
                    None
                };
                tracing::info!(submission = %id, worker = address, saved, "Submission terminal committed");
                if let Err(error) = client.acknowledge(&id).await {
                    tracing::warn!(submission = %id, worker = address, error = %error, "Result acknowledgement failed");
                }
                return Ok(Completion {
                    deferred: None,
                    cooldown: false,
                    review: prepared,
                });
            }
            Err(error) => {
                save_failure(scheduler, database, &id, address, &error, config).await?;
                return Ok(Completion {
                    deferred: None,
                    cooldown: true,
                    review: None,
                });
            }
        }
    }
}

/// Persist an execution failure with actual context truncated at a UTF-8 boundary.
async fn save_failure(
    scheduler: &Scheduler,
    database: &Database,
    id: &str,
    address: &str,
    error: &Error,
    config: &Config,
) -> anyhow::Result<()> {
    tracing::warn!(submission = id, worker = address, error = %error, "Worker tracking failed");
    let mut diagnostic = error.diagnostic.clone();
    let mut end = diagnostic
        .len()
        .min(config.results.failure_diagnostic_max_bytes);
    while !diagnostic.is_char_boundary(end) {
        end -= 1;
    }
    diagnostic.truncate(end);
    let terminal = TerminalOutcome::failed(
        &JobFailure {
            code: error.code.into(),
            diagnostic,
        },
        &config.results,
    )?;
    scheduler
        .save_terminal(database, id, &terminal, None)
        .await?;
    Ok(())
}
