//! Bounded in-memory admission, FIFO dispatch and independent Worker tracking tasks.

pub mod kafka;
mod tracking;
pub mod worker_client;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
    task::{JoinHandle, JoinSet},
    time::Instant,
};

use crate::{
    config::Config,
    database::{self, Database},
    llm_review::{ReviewAttempt, ReviewService},
    submissions::{JudgingRecovery, JudgingTask, TerminalOutcome},
};
use worker_client::WorkerClient;

/// Expected local admission errors, independent of Worker availability.
#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum AdmissionError {
    /// All waiting-admission permits are currently owned.
    #[error("The submission queue is full")]
    QueueFull,
    /// The backend has stopped accepting new submissions.
    #[error("The backend is stopping")]
    Stopping,
}

/// Complete payload retained until delivery or explicit Worker admission.
struct Pending {
    /// Complete fixed-version task, retained while a Worker can refuse admission.
    task: JudgingTask,
}

/// Short synchronous queue state; no lock is held across database or RPC waits.
#[derive(Default)]
struct State {
    /// Waiting-admission tasks in FIFO order.
    queue: VecDeque<Pending>,
    /// One owner per waiting job; admitted jobs retain an ID with no permit.
    known: HashMap<String, Option<OwnedSemaphorePermit>>,
    /// Prevent new reservations and dispatch while allowing existing handoffs.
    stopping: bool,
    /// Finish once all in-flight tracking tasks have ended.
    finish: bool,
    /// The scheduler task has returned or failed and can no longer accept handoffs.
    closed: bool,
}

/// Shared handle state and wakeup, with one semaphore as the capacity counter.
struct Shared {
    /// Payload queue, ID ownership and lifecycle flags.
    state: Mutex<State>,
    /// Creating, queued and dispatching tasks each own one permit.
    capacity: Arc<Semaphore>,
    /// Fixed configured ceiling; live usage remains solely in the semaphore.
    pending_max_count: u32,
    /// Cheap clones of the same clients used by dispatch, in configuration order.
    workers: Vec<WorkerClient>,
    /// Wake the dispatcher for handoff, stop or completion.
    changed: Notify,
}

/// Cloneable process-local interface used by the submission creation service.
#[derive(Clone)]
pub struct Scheduler {
    /// Shared state also owned by the dispatch loop.
    shared: Arc<Shared>,
}

/// Local dispatcher lifecycle, independent of remote Worker observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerState {
    /// The dispatcher has not stopped or requested shutdown.
    Running,
    /// Admission or dispatch has stopped while existing operations drain.
    Stopping,
    /// The dispatcher has ended and can no longer accept handoffs.
    Stopped,
}

/// Read-only admission capacity and lifecycle at one local observation point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Lifecycle derived from the existing closed and stopping flags.
    pub state: SchedulerState,
    /// Permits held by creating, queued and awaiting-admission tasks.
    pub pending_used: u32,
    /// Configured waiting-admission ceiling.
    pub pending_capacity: u32,
}

/// Local availability cache for one configured endpoint.
struct Node {
    /// Stable routing address, never persisted to SQLite.
    address: String,
    /// Reused channel and RPC budgets for this address.
    client: WorkerClient,
    /// The selected Job ID while dispatch or tracking is in flight.
    job: Option<String>,
    /// Earliest time this address may receive a later task.
    ready_at: Instant,
}

/// Marks the shared handle closed even when the dispatcher fails or panics.
struct CloseOnDrop(Arc<Shared>);

impl Drop for CloseOnDrop {
    /// Release queued memory and permits while leaving persisted queued records intact.
    fn drop(&mut self) {
        let mut state = self.0.state.lock().expect("Scheduler state lock poisoned");
        state.closed = true;
        state.stopping = true;
        state.queue.clear();
        state.known.clear();
    }
}

impl Scheduler {
    /// Restore local ownership before starting any dispatch or event processing.
    ///
    /// Only queued jobs receive payloads; confirmed deliveries and running jobs
    /// wait by ID. This constructor does not start a background service.
    pub fn restore(config: &Config, startup: JudgingRecovery) -> anyhow::Result<Self> {
        let workers = config
            .scheduler
            .workers
            .iter()
            .map(|address| WorkerClient::new(address, &config.grpc, &config.results))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let scheduler = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                capacity: Arc::new(Semaphore::new(config.scheduler.pending_max_count as usize)),
                pending_max_count: config.scheduler.pending_max_count,
                workers,
                changed: Notify::new(),
            }),
        };
        for task in startup.queued {
            let permit = scheduler.reserve()?;
            scheduler.enqueue(task, permit)?;
        }
        {
            let mut state = scheduler
                .shared
                .state
                .lock()
                .expect("Scheduler state lock poisoned");
            for id in startup.dispatching {
                let permit = scheduler
                    .shared
                    .capacity
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| AdmissionError::QueueFull)?;
                state.known.insert(id, Some(permit));
            }
            for id in startup.running {
                state.known.insert(id, None);
            }
        }
        Ok(scheduler)
    }

    /// Load ordered startup tasks and their permits before starting the dispatcher.
    pub fn start(
        config: &Config,
        database: Database,
        startup: Vec<JudgingTask>,
        review: ReviewService,
    ) -> anyhow::Result<(Self, JoinHandle<anyhow::Result<()>>)> {
        let scheduler = Self::restore(
            config,
            JudgingRecovery {
                queued: startup,
                ..Default::default()
            },
        )?;
        let nodes = config
            .scheduler
            .workers
            .iter()
            .zip(scheduler.worker_clients())
            .map(|(address, client)| Node {
                address: address.clone(),
                client,
                job: None,
                ready_at: Instant::now(),
            })
            .collect();
        let task = tokio::spawn(run(
            scheduler.clone(),
            nodes,
            Arc::new(config.clone()),
            database,
            review,
        ));
        Ok((scheduler, task))
    }

    /// Observe local lifecycle and semaphore usage without reserving any capacity.
    pub fn snapshot(&self) -> Snapshot {
        let state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        Snapshot {
            state: if state.closed {
                SchedulerState::Stopped
            } else if state.stopping {
                SchedulerState::Stopping
            } else {
                SchedulerState::Running
            },
            pending_used: self.shared.pending_max_count
                - self.shared.capacity.available_permits() as u32,
            pending_capacity: self.shared.pending_max_count,
        }
    }

    /// Share dispatch channels with in-process observation without exposing routing state.
    pub(crate) fn worker_clients(&self) -> Vec<WorkerClient> {
        self.shared.workers.clone()
    }

    /// Reserve capacity before starting a creation transaction, without waiting.
    pub fn reserve(&self) -> Result<OwnedSemaphorePermit, AdmissionError> {
        self.reserve_inner(false)
    }

    /// Let a creation operation admitted before shutdown finish reserving and handing off.
    pub(crate) fn reserve_started(&self) -> Result<OwnedSemaphorePermit, AdmissionError> {
        self.reserve_inner(true)
    }

    /// Share capacity accounting while distinguishing new and already admitted operations.
    fn reserve_inner(&self, started: bool) -> Result<OwnedSemaphorePermit, AdmissionError> {
        let state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        if state.closed || (state.stopping && !started) {
            return Err(AdmissionError::Stopping);
        }
        self.shared
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| AdmissionError::QueueFull)
    }

    /// Check whether a queued or in-flight ID already has a local owner.
    pub fn contains(&self, id: &str) -> bool {
        self.shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned")
            .known
            .contains_key(id)
    }

    /// Transfer a committed task and its permit, merging duplicate handoffs by ID.
    pub fn enqueue(
        &self,
        task: JudgingTask,
        permit: OwnedSemaphorePermit,
    ) -> Result<bool, AdmissionError> {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        if state.closed {
            return Err(AdmissionError::Stopping);
        }
        if state.known.contains_key(&task.request.job_id) {
            return Ok(false);
        }
        state
            .known
            .insert(task.request.job_id.clone(), Some(permit));
        state.queue.push_back(Pending { task });
        drop(state);
        self.shared.changed.notify_one();
        Ok(true)
    }

    /// Persist first admission, then release its permit while retaining the waiting ID.
    pub async fn mark_running(&self, database: &Database, id: &str) -> database::Result<bool> {
        let changed = database.mark_running(id).await?;
        if changed {
            self.release_pending(id);
        }
        Ok(changed)
    }

    /// Atomically save machine terminal and review intent before releasing admission capacity.
    ///
    /// The caller hands off a prepared review only when this returns true, and
    /// completes ID ownership when terminal tracking ends.
    pub async fn save_terminal(
        &self,
        database: &Database,
        id: &str,
        terminal: &TerminalOutcome,
        review: Option<&ReviewAttempt>,
    ) -> database::Result<bool> {
        let changed = database.save_terminal(id, terminal, review).await?;
        if changed {
            self.release_pending(id);
        }
        Ok(changed)
    }

    /// Remove unsent payloads and take a permit at most once after a committed state change.
    fn release_pending(&self, id: &str) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        if let Some(permit) = state.known.get_mut(id) {
            permit.take();
        }
        state
            .queue
            .retain(|pending| pending.task.request.job_id != id);
        drop(state);
        self.shared.changed.notify_one();
    }

    /// End ID ownership after terminal handling or a stale legacy dispatch has finished.
    pub fn complete(&self, id: &str) {
        self.shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned")
            .known
            .remove(id);
    }

    /// Stop reservations and new dispatch; already reserved handoffs may still finish.
    pub fn pause(&self) {
        self.shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned")
            .stopping = true;
        self.shared.changed.notify_one();
    }

    /// Request exit after creation handoffs have completed and tracking has drained.
    pub fn shutdown(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        state.stopping = true;
        state.finish = true;
        drop(state);
        self.shared.changed.notify_one();
    }
}

/// Dispatch without awaiting a particular Worker; completions feed the same queue loop.
async fn run(
    scheduler: Scheduler,
    mut nodes: Vec<Node>,
    config: Arc<Config>,
    database: Database,
    review: ReviewService,
) -> anyhow::Result<()> {
    let _close = CloseOnDrop(scheduler.shared.clone());
    let mut active = JoinSet::new();
    let mut next_node = 0;
    loop {
        loop {
            let mut state = scheduler
                .shared
                .state
                .lock()
                .expect("Scheduler state lock poisoned");
            if state.stopping || state.queue.is_empty() {
                break;
            }
            let selected = (0..nodes.len())
                .map(|offset| (next_node + offset) % nodes.len())
                .find(|&index| {
                    nodes[index].job.is_none() && nodes[index].ready_at <= Instant::now()
                });
            let Some(index) = selected else {
                break;
            };
            let pending = state.queue.pop_front().expect("Nonempty queue");
            let id = pending.task.request.job_id.clone();
            nodes[index].job = Some(id.clone());
            next_node = (index + 1) % nodes.len();
            let client = nodes[index].client.clone();
            let address = nodes[index].address.clone();
            let database = database.clone();
            let config = config.clone();
            let review = review.clone();
            let scheduler = scheduler.clone();
            active.spawn(async move {
                let result = tracking::track(
                    pending, &client, &address, &database, &config, &review, &scheduler,
                )
                .await;
                (index, id, result)
            });
        }
        let wake_at = {
            let state = scheduler
                .shared
                .state
                .lock()
                .expect("Scheduler state lock poisoned");
            if state.finish && active.is_empty() {
                return Ok(());
            }
            if state.stopping || state.queue.is_empty() {
                None
            } else {
                nodes
                    .iter()
                    .filter(|node| node.job.is_none())
                    .map(|node| node.ready_at)
                    .min()
            }
        };
        tokio::select! {
            _ = scheduler.shared.changed.notified() => {},
            _ = async {
                match wake_at {
                    Some(when) => tokio::time::sleep_until(when).await,
                    None => std::future::pending::<()>().await,
                }
            } => {},
            completed = active.join_next(), if !active.is_empty() => {
                let (index, id, result) = completed.expect("Active tracking task")?;
                let result = result?;
                nodes[index].job = None;
                nodes[index].ready_at = Instant::now() + if result.cooldown {
                    std::time::Duration::from_millis(config.scheduler.retry_interval_ms)
                } else { std::time::Duration::ZERO };
                if let Some(pending) = result.deferred {
                    let mut state = scheduler.shared.state.lock().expect("Scheduler state lock poisoned");
                    state.queue.push_front(pending);
                } else {
                    scheduler.complete(&id);
                }
                if let Some(prepared) = result.review {
                    review.handoff(id, prepared).await?;
                }
            },
        }
    }
}
