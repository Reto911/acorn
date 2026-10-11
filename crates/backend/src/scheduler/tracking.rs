//! One selected Job's admission, polling, conditional persistence and single ACK.

use std::time::Duration;

use acorn_protocol::worker::v1::JobFailure;

use super::{
    Pending,
    worker_client::{Error, ErrorKind, Observation, WorkerClient},
};
use crate::{config::Config, database::Database, submissions::TerminalOutcome};

/// Completion event for the dispatcher, retaining capacity only on explicit refusal.
pub(super) struct Completion {
    /// Task to return to the queue head with its original permit.
    pub deferred: Option<Pending>,
    /// Temporarily defer the endpoint after refusal or a failed query/submit.
    pub cooldown: bool,
}

/// Follow one Job at one address, never resubmitting ambiguous or admitted work.
pub(super) async fn track(
    pending: Pending,
    client: &WorkerClient,
    address: &str,
    database: &Database,
    config: &Config,
) -> anyhow::Result<Completion> {
    let id = pending.task.request.job_id.clone();
    if !database.mark_dispatching(&id).await? {
        return Ok(Completion {
            deferred: None,
            cooldown: false,
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
            save_failure(database, &id, address, &error, config).await?;
            return Ok(Completion {
                deferred: None,
                cooldown: error.kind != ErrorKind::Rejected,
            });
        }
    };
    let language = pending.task.request.language_id;
    let total = pending.task.request.cases.len() as u32;
    drop(pending.permit);
    drop(pending.task.request.source);
    drop(pending.task.request.cases);
    if !matches!(first, Some(Err(_))) {
        database.mark_running(&id).await?;
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
                let saved = database.save_terminal(&id, &terminal).await?;
                tracing::info!(submission = %id, worker = address, saved, "Submission terminal committed");
                if let Err(error) = client.acknowledge(&id).await {
                    tracing::warn!(submission = %id, worker = address, error = %error, "Result acknowledgement failed");
                }
                return Ok(Completion {
                    deferred: None,
                    cooldown: false,
                });
            }
            Err(error) => {
                save_failure(database, &id, address, &error, config).await?;
                return Ok(Completion {
                    deferred: None,
                    cooldown: true,
                });
            }
        }
    }
}

/// Persist an execution failure with actual context truncated at a UTF-8 boundary.
async fn save_failure(
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
    database.save_terminal(id, &terminal).await?;
    Ok(())
}
