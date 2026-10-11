//! Creation operations own transaction and local handoff beyond HTTP disconnects.

use std::sync::{Arc, Mutex};

use tokio::sync::Notify;

use super::{CreateSubmission, CreatedSubmission, SubmissionState};
use crate::{
    config::Config,
    database::{self, Database},
    scheduler::{AdmissionError, Scheduler},
};

/// Creation errors retain the original submission identity when rehandoff is full.
#[derive(Debug, thiserror::Error)]
pub enum CreationError {
    /// Storage, pre-existing task data or validated business input failed.
    #[error(transparent)]
    Database(#[from] database::Error),
    /// Capacity or lifecycle refused local admission.
    #[error("{cause}")]
    Admission {
        /// Queue-full or stopping classification.
        cause: AdmissionError,
        /// Existing queued record, included only for a queue-full response.
        submission_id: Option<String>,
    },
    /// The independently owned creation task failed unexpectedly.
    #[error("Creation task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Lifecycle of short creation operations, separate from waiting-admission capacity.
#[derive(Default)]
struct Lifecycle {
    /// Reject operations that have not started before shutdown.
    stopping: bool,
    /// Number of owned short operations still processing transaction or handoff.
    active: usize,
}

/// Shared creation lifecycle and notification for normal shutdown.
#[derive(Default)]
struct Shared {
    /// Synchronous lifecycle lock, never held across an await.
    lifecycle: Mutex<Lifecycle>,
    /// Wake the shutdown waiter when all short operations have finished.
    drained: Notify,
}

/// Scope guard guarantees operation completion is counted even on an error or panic.
struct Active(Arc<Shared>);

impl Drop for Active {
    /// Finish exactly one short creation operation.
    fn drop(&mut self) {
        let mut lifecycle = self
            .0
            .lifecycle
            .lock()
            .expect("Creation lifecycle lock poisoned");
        lifecycle.active -= 1;
        if lifecycle.active == 0 {
            self.0.drained.notify_one();
        }
    }
}

/// Cloneable creation service; the request handler waits but does not own execution.
#[derive(Clone)]
pub struct CreationService {
    /// Existing single-connection database handle.
    database: Database,
    /// In-memory handoff and capacity owner.
    scheduler: Scheduler,
    /// Source validation ceilings.
    config: Arc<Config>,
    /// Outstanding creation operations and stop flag.
    shared: Arc<Shared>,
}

impl CreationService {
    /// Assemble the service using the same database and Scheduler as the HTTP queries.
    pub fn new(config: &Config, database: Database, scheduler: Scheduler) -> Self {
        Self {
            database,
            scheduler,
            config: Arc::new(config.clone()),
            shared: Arc::new(Shared::default()),
        }
    }

    /// Start a short independently owned operation and wait for its creation response.
    pub async fn create(
        &self,
        request: CreateSubmission,
    ) -> Result<CreatedSubmission, CreationError> {
        {
            let mut lifecycle = self
                .shared
                .lifecycle
                .lock()
                .expect("Creation lifecycle lock poisoned");
            if lifecycle.stopping {
                return Err(CreationError::Admission {
                    cause: AdmissionError::Stopping,
                    submission_id: None,
                });
            }
            lifecycle.active += 1;
        }
        let active = Active(self.shared.clone());
        let service = self.clone();
        tokio::spawn(async move {
            let _active = active;
            service.create_owned(request).await
        })
        .await?
    }

    /// Reject later creation operations while existing ones finish their handoffs.
    pub fn stop(&self) {
        self.shared
            .lifecycle
            .lock()
            .expect("Creation lifecycle lock poisoned")
            .stopping = true;
    }

    /// Wait for independently owned transaction-and-handoff operations to finish.
    pub async fn wait_idle(&self) {
        loop {
            let drained = self.shared.drained.notified();
            if self
                .shared
                .lifecycle
                .lock()
                .expect("Creation lifecycle lock poisoned")
                .active
                == 0
            {
                return;
            }
            drained.await;
        }
    }

    /// Resolve idempotency before reserving, then commit and hand off without network waits.
    async fn create_owned(
        &self,
        request: CreateSubmission,
    ) -> Result<CreatedSubmission, CreationError> {
        if request.problem_id <= 0 || request.source.is_empty() {
            return Err(
                database::Error::InvalidRequest("Problem ID and source must be nonempty").into(),
            );
        }
        if request.source.len() > self.config.request.source_max_bytes {
            return Err(database::Error::SourceTooLarge.into());
        }
        let existing = self.database.submission_by_key(request.request_key).await?;
        if let Some(existing) = &existing {
            if existing.info.problem_id != request.problem_id
                || existing.info.language_id != request.language_id
                || existing.source != request.source
            {
                return Err(database::Error::IdempotencyConflict.into());
            }
            if existing.info.state != SubmissionState::Queued
                || self.scheduler.contains(&existing.info.id)
            {
                tracing::debug!(submission = %existing.info.id, "Duplicate creation request");
                return Ok(CreatedSubmission {
                    info: existing.info.clone(),
                    created: false,
                    task: None,
                });
            }
        }
        let permit = self
            .scheduler
            .reserve()
            .map_err(|cause| CreationError::Admission {
                cause,
                submission_id: existing.as_ref().map(|existing| existing.info.id.clone()),
            })?;
        let mut created = self.database.create_submission(request).await?;
        if let Some(task) = created.task.take() {
            self.scheduler
                .enqueue(task, permit)
                .map_err(|cause| CreationError::Admission {
                    cause,
                    submission_id: Some(created.info.id.clone()),
                })?;
        }
        Ok(created)
    }
}
