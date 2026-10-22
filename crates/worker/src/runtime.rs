//! Shared single-slot admission, execution notifications and safe stopping.

use crate::{
    config::Config,
    executor::{ExecutionProgress, Executor},
    jobs::{Execution, JobRecord, Jobs},
    sandbox::{StageControl, StopReason},
};
use acorn_protocol::worker::v1::{
    GetJobReply, JobPhase, JobState, SubmitJobReply, SubmitJobRequest, get_job_reply::Terminal,
};
use anyhow::{Context, Result, ensure};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    time::Instant,
};
use tonic::Status;
use tracing::Instrument;

/// Shared handles; the result lock never spans asynchronous execution or I/O.
pub(crate) struct Shared {
    /// Startup configuration and trust-boundary limits.
    pub(crate) config: Config,
    /// Atomic admission, progress, terminal and ACK transitions.
    pub(crate) jobs: Mutex<Jobs>,
    /// Exactly one executor, retained after an unconfirmed stop.
    executor: AsyncMutex<Executor>,
}

/// Cloneable RPC service with process-local deduplication and result retention.
#[derive(Clone)]
pub struct WorkerService {
    /// Shared state owned by the listener and admitted execution tasks.
    pub(crate) shared: Arc<Shared>,
}

impl WorkerService {
    /// Construct a service from validated deployment configuration.
    pub fn new(config: Config) -> Self {
        Self {
            shared: Arc::new(Shared {
                executor: AsyncMutex::new(Executor::new(config.clone())),
                config,
                jobs: Mutex::new(Jobs::default()),
            }),
        }
    }

    /// Reject new IDs, request the common stop path, and await the admitted task.
    ///
    /// Timeout detaches the task without aborting its future or claiming a safe stop.
    pub async fn shutdown(&self) -> Result<()> {
        let task = {
            let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
            jobs.stopping = true;
            jobs.execution.as_mut().and_then(|execution| {
                tracing::info!(job = %execution.job_id, "Stopping current Job for Worker shutdown");
                // A retained slot's completed task may already have dropped its receiver.
                let _ = execution.stop.send(Some(StopReason::Shutdown));
                execution.task.take()
            })
        };
        if let Some(task) = task {
            tokio::time::timeout(
                Duration::from_millis(self.shared.config.sandbox.shutdown_timeout_ms),
                task,
            )
            .await
            .context("Worker shutdown timed out; execution stop is unconfirmed")?
            .context("Worker execution task failed")?;
        }
        ensure!(
            self.shared
                .jobs
                .lock()
                .expect("Job state lock poisoned")
                .execution
                .is_none(),
            "Worker execution stop remains unconfirmed; deployment intervention required"
        );
        Ok(())
    }

    /// Validate an opaque ID for Job-scoped RPCs before looking up a record.
    pub(crate) fn validate_id(&self, job_id: &str) -> Result<(), Status> {
        if job_id.is_empty() || job_id.len() > self.shared.config.request.job_id_max_bytes {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        Ok(())
    }

    /// Check only new-request language, byte budgets, case count and resource ceilings.
    pub(crate) fn validate_job(&self, request: &SubmitJobRequest) -> Result<(), Status> {
        if !matches!(request.language_id.as_str(), "c" | "python") {
            return Err(Status::invalid_argument("UnsupportedLanguage"));
        }
        let limits = &self.shared.config.request;
        if request.source.is_empty()
            || request.source.len() > limits.source_max_bytes
            || request.cases.is_empty()
            || request.cases.len() > limits.case_max_count
        {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        let mut payload = request.source.len();
        for case in &request.cases {
            if case.input.len() > limits.input_max_bytes
                || case.expected_output.len() > limits.expected_output_max_bytes
            {
                return Err(Status::invalid_argument("InvalidJob"));
            }
            payload = payload
                .checked_add(case.input.len())
                .and_then(|size| size.checked_add(case.expected_output.len()))
                .ok_or_else(|| Status::invalid_argument("InvalidJob"))?;
        }
        if payload > limits.payload_max_bytes {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        let Some(run) = &request.run_limits else {
            return Err(Status::invalid_argument("InvalidJob"));
        };
        let execution = &self.shared.config.execution;
        if run.cpu_time_ms == 0
            || run.cpu_time_ms > execution.max_run_cpu_time_ms
            || run.wall_time_ms == 0
            || run.wall_time_ms > execution.max_run_wall_time_ms
            || run.memory_kib == 0
            || run.memory_kib > execution.max_run_memory_kib
        {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        Ok(())
    }

    /// Return an existing record without validating or comparing repeated Job content.
    fn duplicate(jobs: &Jobs, job_id: &str) -> Option<SubmitJobReply> {
        jobs.records.get(job_id).map(|record| SubmitJobReply {
            job_id: job_id.into(),
            state: record.updates.borrow().state,
            duplicate: true,
        })
    }
    /// Admit an identified request through the common single-slot, deadline and stop-channel flow.
    pub(crate) fn admit(&self, request: SubmitJobRequest) -> Result<Admission, Status> {
        {
            let jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
            if let Some(reply) = Self::duplicate(&jobs, &request.job_id) {
                tracing::debug!(job = %request.job_id, "Duplicate Job submission");
                return Ok(Admission {
                    reply,
                    updates: jobs.records[&request.job_id].updates.subscribe(),
                });
            }
        }
        self.validate_job(&request)?;
        let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        if let Some(reply) = Self::duplicate(&jobs, &request.job_id) {
            return Ok(Admission {
                reply,
                updates: jobs.records[&request.job_id].updates.subscribe(),
            });
        }
        if jobs.stopping {
            tracing::warn!(job = %request.job_id, "Job rejected: WorkerUnavailable");
            return Err(Status::failed_precondition("WorkerUnavailable"));
        }
        if jobs.execution.is_some() {
            tracing::warn!(job = %request.job_id, "Job rejected: WorkerBusy");
            return Err(Status::resource_exhausted("WorkerBusy"));
        }
        if jobs.unacknowledged_count() >= self.shared.config.results.max_unacknowledged_results {
            tracing::warn!(job = %request.job_id, "Job rejected: ResultBufferFull");
            return Err(Status::resource_exhausted("ResultBufferFull"));
        }
        let job_id = request.job_id.clone();
        let deadline =
            Instant::now() + Duration::from_millis(self.shared.config.execution.job_wall_time_ms);
        let (stop, stop_receiver) = watch::channel(None);
        let (updates, receiver) = watch::channel(GetJobReply {
            job_id: job_id.clone(),
            state: JobState::Running as i32,
            phase: JobPhase::Preparing as i32,
            completed_cases: 0,
            total_cases: request.cases.len() as u32,
            terminal: None,
        });
        jobs.records.insert(
            job_id.clone(),
            JobRecord {
                updates,
                acknowledged: false,
            },
        );
        tracing::info!(job = %job_id, language = %request.language_id, phase = "preparing", cases = request.cases.len(), "Job accepted");
        let shared = self.shared.clone();
        let execution_id = job_id.clone();
        let task = tokio::spawn(
            async move {
                execute(
                    shared,
                    request,
                    StageControl {
                        deadline,
                        stop: stop_receiver,
                    },
                )
                .await;
            }
            .instrument(tracing::info_span!("execution", job = %execution_id)),
        );
        jobs.execution = Some(Execution {
            job_id: job_id.clone(),
            stop_unconfirmed: false,
            stop,
            task: Some(task),
        });
        Ok(Admission {
            reply: SubmitJobReply {
                job_id,
                state: JobState::Running as i32,
                duplicate: false,
            },
            updates: receiver,
        })
    }

    /// Remove only the completed record belonging to this notification channel.
    pub(crate) fn forget(&self, updates: &watch::Receiver<GetJobReply>) {
        let id = updates.borrow().job_id.clone();
        let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        if jobs
            .records
            .get(&id)
            .is_some_and(|r| r.updates.subscribe().same_channel(updates))
        {
            jobs.records.remove(&id);
        }
    }

    /// Observe the retained execution position without waiting for its asynchronous lock.
    pub(crate) fn blocked(&self) -> bool {
        self.shared
            .jobs
            .lock()
            .expect("Job state lock poisoned")
            .execution
            .as_ref()
            .is_some_and(|execution| execution.stop_unconfirmed)
    }
}

/// Admission metadata and the latest bounded execution snapshot.
pub(crate) struct Admission {
    /// Existing RPC acknowledgement, also identifying process-local duplicate admissions.
    pub reply: SubmitJobReply,
    /// Progress and immutable terminal updates, independent of the caller's connection.
    pub updates: watch::Receiver<GetJobReply>,
}

/// Execute outside admission, publish a terminal, then release only a safe position.
async fn execute(shared: Arc<Shared>, request: SubmitJobRequest, control: StageControl) {
    let job_id = request.job_id.clone();
    let mut executor = shared.executor.lock().await;
    let outcome = executor
        .run(request, control, |progress: ExecutionProgress| {
            if progress.phase != JobPhase::Done {
                let mut jobs = shared.jobs.lock().expect("Job state lock poisoned");
                let record = jobs.records.get_mut(&job_id).expect("running Job record");
                record.updates.send_modify(|reply| {
                    reply.phase = progress.phase as i32;
                    reply.completed_cases = progress.completed_cases;
                });
            }
        })
        .await;
    let release = executor.is_available();
    drop(executor);
    let mut jobs = shared.jobs.lock().expect("Job state lock poisoned");
    let record = jobs.records.get_mut(&job_id).expect("running Job record");
    record.updates.send_modify(|reply| {
        reply.state = match outcome.terminal {
            Terminal::Result(_) => JobState::Finished as i32,
            Terminal::Failure(_) => JobState::Failed as i32,
        };
        reply.phase = JobPhase::Done as i32;
        reply.completed_cases = outcome.completed_cases;
        reply.terminal = Some(outcome.terminal);
    });
    if release {
        jobs.execution = None;
    } else {
        jobs.execution
            .as_mut()
            .expect("occupied execution slot")
            .stop_unconfirmed = true;
        tracing::error!(job = %job_id, "Execution slot retained after unconfirmed stop; deployment intervention required");
    }
}
