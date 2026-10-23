//! Bounded local admission and supervised Kafka publication and event consumption.

pub mod kafka;
mod validation;
pub mod worker_client;

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore, watch},
    task::{JoinHandle, JoinSet},
};

use crate::{
    config::Config,
    database::{self, Database},
    llm_review::{PreparedReview, ReviewAttempt, ReviewMode, ReviewService},
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

/// Complete payload retained until Kafka delivery or committed Worker progress.
struct Pending {
    /// Fixed-version payload retained until its publication attempt.
    task: JudgingTask,
}

/// Short synchronous queue state; no lock is held across database or Kafka waits.
#[derive(Default)]
struct State {
    /// Waiting-admission tasks in FIFO order.
    queue: VecDeque<Pending>,
    /// One owner per waiting job; admitted jobs retain an ID with no permit.
    known: HashMap<String, Option<OwnedSemaphorePermit>>,
    /// Prevent new reservations and dispatch while allowing existing handoffs.
    stopping: bool,
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
    /// Status-only clients in configuration order; observation never selects work.
    workers: Vec<WorkerClient>,
    /// Wake the publisher for handoff, stop or completion.
    changed: Notify,
    /// Stop message intake after creation handoffs, independently of publication wakeups.
    finish: watch::Sender<bool>,
}

/// Cloneable process-local interface used by the submission creation service.
#[derive(Clone)]
pub struct Scheduler {
    /// Shared state also owned by the Kafka lifecycle task.
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
            .map(|address| WorkerClient::new(address, &config.grpc))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let scheduler = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                capacity: Arc::new(Semaphore::new(config.scheduler.pending_max_count as usize)),
                pending_max_count: config.scheduler.pending_max_count,
                workers,
                changed: Notify::new(),
                finish: watch::channel(false).0,
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

    /// Restore ownership and start the two supervised Kafka services before HTTP admission.
    pub fn start(
        config: &Config,
        database: Database,
        startup: JudgingRecovery,
        review: ReviewService,
    ) -> anyhow::Result<(Self, JoinHandle<anyhow::Result<()>>)> {
        let scheduler = Self::restore(config, startup)?;
        let publisher = kafka::Publisher::new(config, scheduler.clone(), database.clone())?;
        let consumer = kafka::EventConsumer::new(config, scheduler.clone(), database, review)?;
        let task = tokio::spawn(run(
            scheduler.clone(),
            publisher,
            consumer,
            Duration::from_millis(config.kafka.shutdown_timeout_ms),
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

    /// Share status channels with independent health observation.
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
    /// completes ID ownership when terminal handling ends.
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

    /// Prepare automatic review intent and atomically persist the machine terminal.
    pub(crate) async fn persist_terminal(
        &self,
        database: &Database,
        review: &ReviewService,
        id: &str,
        mode: ReviewMode,
        terminal: &TerminalOutcome,
    ) -> database::Result<Option<PreparedReview>> {
        let prepared = review.prepare_automatic(mode, terminal)?;
        let saved = self
            .save_terminal(
                database,
                id,
                terminal,
                prepared.as_ref().map(|review| &review.attempt),
            )
            .await?;
        tracing::info!(submission = id, saved, "Submission terminal committed");
        Ok(if saved { prepared } else { None })
    }

    /// Observe whether an ID still owns the admission permit required for publication retries.
    fn awaiting_admission(&self, id: &str) -> bool {
        self.shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned")
            .known
            .get(id)
            .is_some_and(Option::is_some)
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

    /// End ID ownership after terminal processing and review handoff.
    pub fn complete(&self, id: &str) {
        self.shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned")
            .known
            .remove(id);
        self.shared.changed.notify_one();
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

    /// Stop receiving new events and drain started message work after creation handoffs.
    pub fn shutdown(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("Scheduler state lock poisoned");
        state.stopping = true;
        drop(state);
        self.shared.finish.send_replace(true);
        self.shared.changed.notify_one();
    }
}

/// Supervise both transports and give all started message work one shared shutdown budget.
async fn run(
    scheduler: Scheduler,
    publisher: kafka::Publisher,
    consumer: kafka::EventConsumer,
    budget: Duration,
) -> anyhow::Result<()> {
    let _close = CloseOnDrop(scheduler.shared.clone());
    let (stop, stopping) = watch::channel(false);
    let mut services = JoinSet::new();
    services.spawn(async move { publisher.run(stopping).await });
    services.spawn(consumer.run(stop.subscribe()));
    let mut finish = scheduler.shared.finish.subscribe();
    let result = loop {
        if *finish.borrow() {
            break Ok(());
        }
        tokio::select! {
            _ = finish.changed() => {},
            result = services.join_next() => {
                let completed = match result {
                    Some(result) => result.map_err(anyhow::Error::from).and_then(|result| result),
                    None => Err(anyhow::anyhow!("Kafka services stopped unexpectedly")),
                };
                if let Err(error) = completed { break Err(error); }
                if !scheduler.shared.state.lock().expect("Scheduler state lock poisoned").stopping {
                    break Err(anyhow::anyhow!("Kafka service stopped unexpectedly"));
                }
            }
        }
    };
    scheduler.pause();
    stop.send_replace(true);
    let drained = tokio::time::timeout(budget, async {
        let mut failure = None;
        while let Some(service) = services.join_next().await {
            if let Err(error) = service
                .map_err(anyhow::Error::from)
                .and_then(|result| result)
            {
                tracing::error!(%error, "Kafka service failed during shutdown");
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    })
    .await;
    let drained = match drained {
        Ok(result) => result,
        Err(_) => {
            services.abort_all();
            // Observe cancellation before the lifecycle owner closes the database.
            while services.join_next().await.is_some() {}
            Err(anyhow::anyhow!(
                "KafkaBackendShutdownTimedOut: message handoff or consumer release incomplete"
            ))
        }
    };
    result.and(drained)
}
