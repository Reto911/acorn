//! Bounded in-memory admission, FIFO dispatch and independent Worker tracking tasks.

mod tracking;
pub mod worker_client;

use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
    task::{JoinHandle, JoinSet},
    time::Instant,
};

use crate::{config::Config, database::Database, submissions::JudgingTask};
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

/// A task owns its capacity until explicitly admitted or locally finished.
struct Pending {
    /// Complete fixed-version task, retained while a Worker can refuse admission.
    task: JudgingTask,
    /// Sole capacity accounting for this queued or awaiting-admission task.
    permit: OwnedSemaphorePermit,
}

/// Short synchronous queue state; no lock is held across database or RPC waits.
#[derive(Default)]
struct State {
    /// Waiting-admission tasks in FIFO order.
    queue: VecDeque<Pending>,
    /// IDs present in the queue or an in-flight tracking task.
    known: HashSet<String>,
    /// Prevent new reservations and dispatch while allowing existing handoffs.
    stopping: bool,
    /// Finish once all in-flight tracking tasks have ended.
    finish: bool,
    /// The scheduler task has returned or failed and can no longer accept handoffs.
    closed: bool,
}

/// Shared handle state and wakeup, with one semaphore as the capacity counter.
struct Shared {
    /// Queue and lifecycle flags.
    state: Mutex<State>,
    /// Creating, queued and awaiting-admission tasks each own one permit.
    capacity: Arc<Semaphore>,
    /// Wake the dispatcher for handoff, stop or completion.
    changed: Notify,
}

/// Cloneable process-local interface used by the submission creation service.
#[derive(Clone)]
pub struct Scheduler {
    /// Shared state also owned by the dispatch loop.
    shared: Arc<Shared>,
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
    /// Start an initially empty dispatcher; startup database loading belongs to P09.
    pub fn start(
        config: &Config,
        database: Database,
    ) -> anyhow::Result<(Self, JoinHandle<anyhow::Result<()>>)> {
        let nodes = config
            .scheduler
            .workers
            .iter()
            .map(|address| {
                Ok(Node {
                    address: address.clone(),
                    client: WorkerClient::new(address, &config.grpc, &config.results)?,
                    job: None,
                    ready_at: Instant::now(),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let scheduler = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                capacity: Arc::new(Semaphore::new(config.scheduler.pending_max_count as usize)),
                changed: Notify::new(),
            }),
        };
        let task = tokio::spawn(run(
            scheduler.clone(),
            nodes,
            Arc::new(config.clone()),
            database,
        ));
        Ok((scheduler, task))
    }

    /// Reserve capacity before starting a creation transaction, without waiting.
    pub fn reserve(&self) -> Result<OwnedSemaphorePermit, AdmissionError> {
        let state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        if state.stopping || state.closed {
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
            .contains(id)
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
        if !state.known.insert(task.request.job_id.clone()) {
            return Ok(false);
        }
        state.queue.push_back(Pending { task, permit });
        drop(state);
        self.shared.changed.notify_one();
        Ok(true)
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
            active.spawn(async move {
                let result = tracking::track(pending, &client, &address, &database, &config).await;
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
                let mut state = scheduler.shared.state.lock().expect("Scheduler state lock poisoned");
                if let Some(pending) = result.deferred {
                    state.queue.push_front(pending);
                } else {
                    state.known.remove(&id);
                }
            },
        }
    }
}
