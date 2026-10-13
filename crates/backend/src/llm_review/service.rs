//! Independent bounded review queue, automatic reservations and cancellation persistence.

use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore, watch},
    task::{JoinHandle, JoinSet},
};

use crate::{
    config::LlmReviewConfig,
    database::{Database, Result, now_ms},
    submissions::TerminalOutcome,
};

use super::{
    ReviewAttempt, ReviewErrorCode, ReviewMode, ReviewTrigger, client::ReviewClient, policy,
};

/// Automatic initial state and ownership of capacity until commit and local handoff.
pub(crate) struct PreparedReview {
    /// Initial metadata atomically saved with the machine terminal.
    pub(crate) attempt: ReviewAttempt,
    /// Present only when a pending attempt has reserved independent queue capacity.
    permit: Option<OwnedSemaphorePermit>,
}

/// One committed attempt; source and statement are loaded only when execution begins.
struct Pending {
    /// Persisted submission identity.
    submission_id: String,
    /// Latest attempt identity and request metadata.
    attempt: ReviewAttempt,
    /// Waiting capacity, released when execution or interruption starts.
    permit: OwnedSemaphorePermit,
}

/// Short synchronous queue state, never held across storage or network waits.
#[derive(Default)]
struct State {
    /// Committed work in local handoff order.
    queue: VecDeque<Pending>,
    /// Submission identities already queued or executing.
    known: HashSet<String>,
    /// Shared boundary for automatic reservations and all handoffs.
    stopping: bool,
    /// The supervised queue task has ended and cannot receive more work.
    closed: bool,
}

/// Shared capacity, queue and cancellation notifications.
struct Shared {
    /// Queue ownership and lifecycle flags.
    state: Mutex<State>,
    /// Sole count of reserved and queued attempts, separate from running work.
    capacity: Arc<Semaphore>,
    /// Wake the queue after handoff or stop.
    changed: Notify,
    /// Cancel input reads and external HTTP waits without canceling short writes.
    stopped: watch::Sender<bool>,
}

/// Cloneable review service, sharing the existing database and one HTTP client.
#[derive(Clone)]
pub struct ReviewService {
    /// Existing single-connection pool, released before model calls.
    database: Database,
    /// Immutable startup settings, never logged as a whole.
    config: Arc<LlmReviewConfig>,
    /// Absent for disabled deployments; no startup model request is sent.
    client: Option<ReviewClient>,
    /// Independent local queue and stop boundary.
    shared: Arc<Shared>,
}

/// Mark an unexpectedly ended service unavailable, without attempting asynchronous repair.
struct CloseOnDrop(Arc<Shared>);

impl Drop for CloseOnDrop {
    /// Release queued permits and cancel any local request waits on service exit.
    fn drop(&mut self) {
        let mut state = self.0.state.lock().expect("Review state lock poisoned");
        state.closed = true;
        state.queue.clear();
        state.known.clear();
        self.0.stopped.send_replace(true);
    }
}

impl ReviewService {
    /// Construct one reusable client and start the supervised queue, including when disabled.
    pub fn start(
        config: &LlmReviewConfig,
        database: Database,
    ) -> anyhow::Result<(Self, JoinHandle<anyhow::Result<()>>)> {
        let client = ReviewClient::new(config)?;
        let (stopped, _) = watch::channel(false);
        let service = Self {
            database,
            config: Arc::new(config.clone()),
            client,
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                capacity: Arc::new(Semaphore::new(config.pending_max_count as usize)),
                changed: Notify::new(),
                stopped,
            }),
        };
        let task = tokio::spawn(run(service.clone()));
        Ok((service, task))
    }

    /// Reserve without waiting, selecting the automatic initial state before terminal commit.
    pub(crate) fn prepare_automatic(
        &self,
        mode: ReviewMode,
        terminal: &TerminalOutcome,
    ) -> Result<Option<PreparedReview>> {
        if !policy::automatic(mode, terminal) {
            return Ok(None);
        }
        let mut attempt = ReviewAttempt::pending(
            ReviewTrigger::Auto,
            now_ms()?,
            self.config.model.clone(),
            self.config.reasoning_effort.clone(),
            policy::PROMPT_VERSION.into(),
        )?;
        let state = self
            .shared
            .state
            .lock()
            .expect("Review state lock poisoned");
        let reservation = if state.stopping {
            Err(ReviewErrorCode::ReviewInterrupted)
        } else if state.closed || self.client.is_none() {
            Err(ReviewErrorCode::ReviewUnavailable)
        } else {
            self.shared
                .capacity
                .clone()
                .try_acquire_owned()
                .map_err(|_| ReviewErrorCode::ReviewQueueFull)
        };
        drop(state);
        let permit = match reservation {
            Ok(permit) => Some(permit),
            Err(code) => {
                attempt = attempt.failed(now_ms()?, code, None, None, None)?;
                None
            }
        };
        Ok(Some(PreparedReview { attempt, permit }))
    }

    /// Hand off only after machine persistence, ACK and Scheduler node release have finished.
    pub(crate) async fn handoff(&self, id: String, prepared: PreparedReview) -> Result<()> {
        let Some(permit) = prepared.permit else {
            return Ok(());
        };
        let pending = Pending {
            submission_id: id,
            attempt: prepared.attempt,
            permit,
        };
        let failure = {
            let mut state = self
                .shared
                .state
                .lock()
                .expect("Review state lock poisoned");
            if state.stopping {
                ReviewErrorCode::ReviewInterrupted
            } else if state.closed {
                ReviewErrorCode::ReviewUnavailable
            } else {
                if state.known.insert(pending.submission_id.clone()) {
                    state.queue.push_back(pending);
                }
                self.shared.changed.notify_one();
                return Ok(());
            }
        };
        drop(pending.permit);
        self.fail(&pending.submission_id, pending.attempt, failure)
            .await
    }

    /// Immediately reject reservations and handoffs and interrupt queued or active attempts.
    pub fn stop(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("Review state lock poisoned");
        state.stopping = true;
        self.shared.stopped.send_replace(true);
        drop(state);
        self.shared.changed.notify_one();
    }

    /// Persist one fixed failure using the latest-attempt conditional write.
    async fn fail(&self, id: &str, attempt: ReviewAttempt, code: ReviewErrorCode) -> Result<()> {
        let attempt = attempt.failed(now_ms()?, code, None, None, None)?;
        self.database.finish_review(id, &attempt).await?;
        tracing::info!(submission = id, review = %attempt.id, error_code = ?code, "Review ended without an opinion");
        Ok(())
    }

    /// Cancel local reads and HTTP waits on stop, while allowing a started completion write to end.
    async fn execute(&self, id: &str, pending: ReviewAttempt) -> Result<()> {
        let mut stopped = self.shared.stopped.subscribe();
        let completed = tokio::select! {
            biased;
            _ = async {
                if !*stopped.borrow_and_update() {
                    let _ = stopped.changed().await;
                }
            } => pending.failed(now_ms()?, ReviewErrorCode::ReviewInterrupted, None, None, None)?,
            result = async {
                match self.database.review_input(id).await {
                    Ok(Some(input)) => self.client.as_ref().expect("Enabled review client")
                        .review(id, &input, pending.clone()).await,
                    other => {
                        if let Err(error) = other {
                            tracing::warn!(submission = id, review = %pending.id, error = %error, "Review input unavailable");
                        }
                        pending.clone().failed(now_ms()?, ReviewErrorCode::ReviewInputUnavailable, None, None, None)
                    }
                }
            } => result?,
        };
        let saved = self.database.finish_review(id, &completed).await?;
        tracing::info!(submission = id, review = %completed.id, status = completed.status.as_str(), saved, "Review terminal committed");
        Ok(())
    }
}

/// Dispatch FIFO work within the running limit and drain local interruption writes on stop.
async fn run(service: ReviewService) -> anyhow::Result<()> {
    let _close = CloseOnDrop(service.shared.clone());
    let mut active = JoinSet::new();
    loop {
        let changed = service.shared.changed.notified();
        {
            let mut state = service
                .shared
                .state
                .lock()
                .expect("Review state lock poisoned");
            while active.len() < service.config.max_concurrent as usize {
                let Some(pending) = state.queue.pop_front() else {
                    break;
                };
                drop(pending.permit);
                let service = service.clone();
                active.spawn(async move {
                    let result = service
                        .execute(&pending.submission_id, pending.attempt)
                        .await;
                    (pending.submission_id, result)
                });
            }
            if state.stopping && active.is_empty() && state.queue.is_empty() {
                return Ok(());
            }
        }
        tokio::select! {
            _ = changed => {},
            completed = active.join_next(), if !active.is_empty() => {
                let (id, result) = completed.expect("Active review task")?;
                result?;
                service.shared.state.lock().expect("Review state lock poisoned").known.remove(&id);
            },
        }
    }
}
