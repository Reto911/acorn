//! Read-only local snapshots and one shared, bounded database, Kafka and Worker collector.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use tokio::{
    sync::watch,
    task::{JoinHandle, JoinSet},
    time::Instant,
};

use crate::{
    config::HealthConfig,
    database::Database,
    scheduler::{
        Scheduler, SchedulerState, Snapshot as SchedulerSnapshot, worker_client::WorkerClient,
    },
    submissions::CreationService,
};

/// Current local admission conditions; actual creation still performs its own reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmissionAdmission {
    /// New submissions may attempt validation and capacity reservation.
    Open,
    /// All waiting-admission permits are currently owned.
    Full,
    /// Creation or dispatch has stopped accepting new work.
    Stopping,
    /// The dispatcher has ended.
    Unavailable,
}

/// Local component observations, read separately without a cross-component transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalSnapshot {
    /// Admission derived from creation, dispatcher lifecycle and remaining permits.
    pub submission_admission: SubmissionAdmission,
    /// Actual dispatcher lifecycle and semaphore usage.
    pub scheduler: SchedulerSnapshot,
}

/// Read local admission and scheduling without reserving or changing any state.
pub fn local_snapshot(creation: &CreationService, scheduler: &Scheduler) -> LocalSnapshot {
    let scheduler = scheduler.snapshot();
    let submission_admission = match scheduler.state {
        SchedulerState::Stopped => SubmissionAdmission::Unavailable,
        SchedulerState::Stopping => SubmissionAdmission::Stopping,
        SchedulerState::Running if creation.is_stopping() => SubmissionAdmission::Stopping,
        SchedulerState::Running if scheduler.pending_used == scheduler.pending_capacity => {
            SubmissionAdmission::Full
        }
        SchedulerState::Running => SubmissionAdmission::Open,
    };
    LocalSnapshot {
        submission_admission,
        scheduler,
    }
}

/// Validated remote execution occupancy, independent of result retention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionState {
    /// No execution position is occupied.
    Idle,
    /// An admitted Job still occupies the execution position.
    Running,
    /// Execution returned without confirming a safe stop.
    Blocked,
}

/// Worker-reported state that passed the status RPC contract checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerStatus {
    /// Whether the remote Worker is stopping new admission.
    pub stopping: bool,
    /// Occupancy of the remote execution position.
    pub execution_state: ExecutionState,
    /// Terminal entries still awaiting Kafka handoff.
    pub unacknowledged_results: u64,
    /// Positive configured ceiling for those results.
    pub unacknowledged_capacity: u64,
}

/// Stable observation errors, separate from persisted execution failure codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeErrorCode {
    /// No result was obtained within the per-check budget.
    ProbeTimeout,
    /// The database returned an actual read error.
    DatabaseReadFailed,
    /// The Worker RPC returned a communication or service error.
    WorkerRpcFailed,
    /// The remote service does not implement the status RPC.
    WorkerStatusUnsupported,
    /// Kafka metadata could not be read or lacks usable configured-topic partitions.
    KafkaMetadataFailed,
    /// A status reply violates the defined enum or result-capacity conditions.
    InvalidWorkerStatus,
}

/// Original error context retained for logs, never included in a cached public observation.
#[derive(Clone, Debug, thiserror::Error)]
#[error("{diagnostic}")]
pub(crate) struct ProbeError {
    /// Stable observation classification.
    pub code: ProbeErrorCode,
    /// Original database, Kafka, RPC or protocol error detail.
    diagnostic: String,
}

impl ProbeError {
    /// Preserve the actual error alongside its observation-only classification.
    pub fn new(code: ProbeErrorCode, diagnostic: impl ToString) -> Self {
        Self {
            code,
            diagnostic: diagnostic.to_string(),
        }
    }
}

/// Freshness and outcome of the latest completed check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    /// No check has completed yet.
    Unknown,
    /// The latest check obtained a valid result within budget.
    Ok,
    /// The latest check failed; older successes are discarded.
    Failed,
    /// The latest completion has reached its maximum display age.
    Stale,
}

/// Projected latest observation; only a fresh success contains its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeSnapshot<T> {
    /// Latest outcome after applying the monotonic age limit.
    pub state: ProbeState,
    /// Backend UTC Unix milliseconds at check completion, if one exists.
    pub checked_at_ms: Option<i64>,
    /// Stable error for a fresh failed check only.
    pub error_code: Option<ProbeErrorCode>,
    /// Validated value for a fresh success only.
    pub value: Option<T>,
}

/// One configured Worker observation without its internal endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerSnapshot {
    /// Configuration-order label, such as worker-1; not a persistent identity.
    pub id: String,
    /// Latest validated status, failure or freshness state.
    pub probe: ProbeSnapshot<WorkerStatus>,
}

/// Cached database, Kafka and Worker observations; obtaining this snapshot performs no I/O.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Latest actual database table-read outcome.
    pub database: ProbeSnapshot<()>,
    /// Actual metadata observation for both configured Kafka topics.
    pub kafka: ProbeSnapshot<()>,
    /// Worker observations in configuration order.
    pub workers: Vec<WorkerSnapshot>,
}

/// One completed check; errors replace prior successful values.
struct Completed<T> {
    /// Monotonic check completion used only for freshness.
    checked_at: Instant,
    /// Backend wall-clock completion time exposed in snapshots.
    checked_at_ms: i64,
    /// Latest result and private diagnostic; public snapshots expose only the stable code.
    result: std::result::Result<T, ProbeError>,
}

impl<T> Completed<T> {
    /// Record completion with real clocks, reporting any timestamp failure explicitly.
    fn new(result: std::result::Result<T, ProbeError>) -> Result<Self> {
        let checked_at = Instant::now();
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Read health probe completion time")?;
        let checked_at_ms = i64::try_from(elapsed.as_millis())
            .context("Health probe completion time exceeds Unix millisecond range")?;
        Ok(Self {
            checked_at,
            checked_at_ms,
            result,
        })
    }
}

/// Latest observations only, guarded by a short synchronous lock.
struct Cache {
    /// Database check, initially unknown.
    database: Option<Completed<()>>,
    /// Configured Kafka topics, initially unknown.
    kafka: Option<Completed<()>>,
    /// One initially unknown check per configured Worker.
    workers: Vec<Option<Completed<WorkerStatus>>>,
}

/// Cloneable cached observations and explicit collector stop signal.
#[derive(Clone)]
pub struct Health {
    /// Shared latest results; no I/O occurs while its lock is held.
    cache: Arc<Mutex<Cache>>,
    /// Derived maximum age of a displayable result.
    stale_after: Duration,
    /// Retained sender makes stopping explicit even while a check is pending.
    stop: watch::Sender<bool>,
}

impl Health {
    /// Start one collector from validated configuration and existing status channels.
    pub fn start(
        config: &HealthConfig,
        database: Database,
        scheduler: &Scheduler,
    ) -> Result<(Self, JoinHandle<Result<()>>)> {
        let workers = scheduler.worker_clients();
        let (stop, stopped) = watch::channel(false);
        let health = Self {
            cache: Arc::new(Mutex::new(Cache {
                database: None,
                kafka: None,
                workers: workers.iter().map(|_| None).collect(),
            })),
            stale_after: config.stale_after()?,
            stop,
        };
        let task = tokio::spawn(collect(
            health.clone(),
            database,
            workers,
            config.clone(),
            scheduler.kafka_observer(),
            stopped,
        ));
        Ok((health, task))
    }

    /// Read cached outcomes with their current age, without initiating any probes.
    pub fn snapshot(&self) -> Snapshot {
        let cache = self.cache.lock().expect("Health cache lock poisoned");
        let now = Instant::now();
        Snapshot {
            database: project(cache.database.as_ref(), now, self.stale_after),
            kafka: project(cache.kafka.as_ref(), now, self.stale_after),
            workers: cache
                .workers
                .iter()
                .enumerate()
                .map(|(index, latest)| WorkerSnapshot {
                    id: format!("worker-{}", index + 1),
                    probe: project(latest.as_ref(), now, self.stale_after),
                })
                .collect(),
        }
    }

    /// Interrupt pending checks or the between-round delay; the owner must await the task.
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
}

/// Project freshness without retaining or exposing an older successful value.
fn project<T: Clone>(
    latest: Option<&Completed<T>>,
    now: Instant,
    stale_after: Duration,
) -> ProbeSnapshot<T> {
    let Some(latest) = latest else {
        return ProbeSnapshot {
            state: ProbeState::Unknown,
            checked_at_ms: None,
            error_code: None,
            value: None,
        };
    };
    let mut snapshot = ProbeSnapshot {
        state: ProbeState::Stale,
        checked_at_ms: Some(latest.checked_at_ms),
        error_code: None,
        value: None,
    };
    if now.duration_since(latest.checked_at) < stale_after {
        match &latest.result {
            Ok(value) => {
                snapshot.state = ProbeState::Ok;
                snapshot.value = Some(value.clone());
            }
            Err(error) => {
                snapshot.state = ProbeState::Failed;
                snapshot.error_code = Some(error.code);
            }
        }
    }
    snapshot
}

/// Replace every completion, returning a changed failure or a recovery for logging outside the lock.
fn record<T>(
    latest: &mut Option<Completed<T>>,
    completed: Completed<T>,
) -> (Option<ProbeError>, bool) {
    let previous = latest
        .as_ref()
        .and_then(|previous| previous.result.as_ref().err())
        .map(|error| error.code);
    let warning = completed
        .result
        .as_ref()
        .err()
        .filter(|error| Some(error.code) != previous)
        .cloned();
    let recovered = previous.is_some() && completed.result.is_ok();
    *latest = Some(completed);
    (warning, recovered)
}

/// Independently completed checks returned by the current round.
enum Check {
    /// Actual table read through the existing database boundary.
    Database(Completed<()>),
    /// Configured-topic metadata obtained through the shared Kafka producer.
    Kafka(Completed<()>),
    /// Validated status for the Worker at this configuration index.
    Worker(usize, Completed<WorkerStatus>),
}

/// Run non-overlapping rounds and drain cancelled checks before returning to the owner.
async fn collect(
    health: Health,
    database: Database,
    workers: Vec<WorkerClient>,
    config: HealthConfig,
    kafka: crate::scheduler::kafka::Observer,
    mut stopped: watch::Receiver<bool>,
) -> Result<()> {
    let timeout = Duration::from_millis(config.timeout_ms);
    loop {
        if *stopped.borrow() {
            return Ok(());
        }
        let mut checks = JoinSet::new();
        let observer = kafka.clone();
        checks.spawn_blocking(move || {
            let result = observer.metadata(timeout);
            Completed::new(result).map(Check::Kafka)
        });
        let database = database.clone();
        checks.spawn(async move {
            let result = match tokio::time::timeout(timeout, database.probe_read()).await {
                Ok(Ok(())) => {
                    tracing::debug!("Database health probe completed");
                    Ok(())
                }
                Ok(Err(error)) => Err(ProbeError::new(ProbeErrorCode::DatabaseReadFailed, error)),
                Err(error) => Err(ProbeError::new(ProbeErrorCode::ProbeTimeout, error)),
            };
            Completed::new(result).map(Check::Database)
        });
        for (index, worker) in workers.iter().cloned().enumerate() {
            checks.spawn(async move {
                let result = worker.status(timeout).await;
                if result.is_ok() {
                    tracing::debug!(worker = index + 1, "Worker health probe completed");
                }
                Completed::new(result).map(|completed| Check::Worker(index, completed))
            });
        }
        while !checks.is_empty() {
            let completed = tokio::select! {
                biased;
                _ = wait_for_stop(&mut stopped) => {
                    checks.shutdown().await;
                    return Ok(());
                }
                completed = checks.join_next() => completed.expect("Pending health checks"),
            };
            let completed = match completed
                .context("Health check task failed")
                .and_then(|result| result)
            {
                Ok(completed) => completed,
                Err(error) => {
                    tracing::error!(error = %error, "Health collector failed");
                    checks.shutdown().await;
                    return Err(error);
                }
            };
            let (component, worker, (warning, recovered)) = {
                let mut cache = health.cache.lock().expect("Health cache lock poisoned");
                match completed {
                    Check::Database(completed) => {
                        ("database", None, record(&mut cache.database, completed))
                    }
                    Check::Kafka(completed) => ("kafka", None, record(&mut cache.kafka, completed)),
                    Check::Worker(index, completed) => (
                        "worker",
                        Some(index + 1),
                        record(&mut cache.workers[index], completed),
                    ),
                }
            };
            if let Some(error) = warning {
                tracing::warn!(component, worker, code = ?error.code, %error, "Health probe failed");
            } else if recovered {
                tracing::info!(component, worker, "Health probe recovered");
            }
        }
        tokio::select! {
            biased;
            _ = wait_for_stop(&mut stopped) => return Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(config.interval_ms)) => {},
        }
    }
}

/// Release the watch borrow before cancellation starts awaiting the pending check tasks.
async fn wait_for_stop(stopped: &mut watch::Receiver<bool>) {
    stopped
        .wait_for(|stop| *stop)
        .await
        .expect("Health collector retains its stop sender");
}

#[cfg(test)]
mod tests {
    //! Freshness checks use supplied monotonic times without waiting for real expiration.

    use super::*;

    /// Repeated failures still replace timestamps; only error-category changes and recovery are logged.
    #[test]
    fn completion_logging_tracks_failure_transitions() {
        let mut latest = None;
        for (index, (code, warn, recovered)) in [
            (Some(ProbeErrorCode::WorkerRpcFailed), true, false),
            (Some(ProbeErrorCode::WorkerRpcFailed), false, false),
            (Some(ProbeErrorCode::ProbeTimeout), true, false),
            (None, false, true),
            (None, false, false),
            (Some(ProbeErrorCode::WorkerRpcFailed), true, false),
        ]
        .into_iter()
        .enumerate()
        {
            let completed = Completed {
                checked_at: Instant::now(),
                checked_at_ms: index as i64,
                result: code.map_or(Ok(()), |code| {
                    Err(ProbeError::new(code, format!("attempt {index}")))
                }),
            };
            let (warning, recovery) = record(&mut latest, completed);
            assert_eq!(warning.is_some(), warn);
            assert_eq!(recovery, recovered);
            assert_eq!(latest.as_ref().unwrap().checked_at_ms, index as i64);
        }
    }

    /// Each result replaces the last; exactly reaching the age limit hides value and error.
    #[test]
    fn latest_result_and_stale_boundary() {
        let now = Instant::now();
        let age_limit = Duration::from_secs(12);
        let mut latest = None;
        assert_eq!(
            project::<WorkerStatus>(latest.as_ref(), now, age_limit),
            ProbeSnapshot {
                state: ProbeState::Unknown,
                checked_at_ms: None,
                error_code: None,
                value: None,
            }
        );
        let status = WorkerStatus {
            stopping: false,
            execution_state: ExecutionState::Idle,
            unacknowledged_results: 0,
            unacknowledged_capacity: 128,
        };
        for result in [Ok(status), Err(ProbeErrorCode::WorkerRpcFailed), Ok(status)] {
            latest = Some(Completed {
                checked_at: now,
                checked_at_ms: 42,
                result: result.map_err(|code| ProbeError::new(code, "Controlled probe failure")),
            });
            let fresh = project(
                latest.as_ref(),
                now + age_limit - Duration::from_nanos(1),
                age_limit,
            );
            match result {
                Ok(status) => {
                    assert_eq!(fresh.state, ProbeState::Ok);
                    assert_eq!(fresh.value, Some(status));
                    assert_eq!(fresh.error_code, None);
                }
                Err(code) => {
                    assert_eq!(fresh.state, ProbeState::Failed);
                    assert_eq!(fresh.value, None);
                    assert_eq!(fresh.error_code, Some(code));
                }
            }
            assert_eq!(fresh.checked_at_ms, Some(42));
            for time in [now + age_limit, now + age_limit + Duration::from_secs(1)] {
                assert_eq!(
                    project(latest.as_ref(), time, age_limit),
                    ProbeSnapshot {
                        state: ProbeState::Stale,
                        checked_at_ms: Some(42),
                        error_code: None,
                        value: None,
                    }
                );
            }
        }
    }
}
