//! Shared single-slot admission, execution notifications and safe stopping.

use crate::{
    config::Config,
    executor::{ExecutionProgress, Executor},
    jobs::{Execution, Jobs},
    sandbox::{StageControl, StopReason},
};
use acorn_protocol::worker::v1::{
    GetJobReply, JobPhase, JobState, SubmitJobRequest, get_job_reply::Terminal,
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
    /// Atomic admission, progress and retained terminal notifications.
    pub(crate) jobs: Mutex<Jobs>,
    /// Exactly one executor, retained after an unconfirmed stop.
    executor: AsyncMutex<Executor>,
}

/// Shared execution service with process-local deduplication and bounded notifications.
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

    /// Validate an opaque task ID before looking up retained execution updates.
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

    /// Admit through the single-slot deadline and stop-channel flow, or reuse retained updates.
    pub(crate) fn admit(
        &self,
        request: SubmitJobRequest,
    ) -> Result<watch::Receiver<GetJobReply>, Status> {
        let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        if let Some(updates) = jobs.records.get(&request.job_id) {
            return Ok(updates.subscribe());
        }
        self.validate_job(&request)?;
        if jobs.stopping {
            tracing::warn!(job = %request.job_id, "Job rejected: WorkerUnavailable");
            return Err(Status::failed_precondition("WorkerUnavailable"));
        }
        if jobs.execution.is_some() {
            tracing::warn!(job = %request.job_id, "Job rejected: WorkerBusy");
            return Err(Status::resource_exhausted("WorkerBusy"));
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
        jobs.records.insert(job_id.clone(), updates);
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
        Ok(receiver)
    }

    /// Remove only the completed record belonging to this notification channel.
    pub(crate) fn forget(&self, updates: &watch::Receiver<GetJobReply>) {
        let id = updates.borrow().job_id.clone();
        let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        if jobs
            .records
            .get(&id)
            .is_some_and(|r| r.subscribe().same_channel(updates))
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

/// Execute outside admission, publish a terminal, then release only a safe position.
async fn execute(shared: Arc<Shared>, request: SubmitJobRequest, control: StageControl) {
    let job_id = request.job_id.clone();
    let mut executor = shared.executor.lock().await;
    let outcome = executor
        .run(request, control, |progress: ExecutionProgress| {
            if progress.phase != JobPhase::Done {
                let mut jobs = shared.jobs.lock().expect("Job state lock poisoned");
                let record = jobs.records.get_mut(&job_id).expect("running Job record");
                record.send_modify(|reply| {
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
    record.send_modify(|reply| {
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

#[cfg(test)]
mod tests {
    //! Admission validation retained after retiring the business RPC adapter.
    use super::*;
    use acorn_protocol::worker::v1::{RunLimits, TestCase};

    /// Invalid language, limits and payload never register work; empty case bytes remain valid.
    #[test]
    fn validation_rejects_without_registering() {
        let mut config =
            Config::parse(include_str!("../../../config/worker.example.toml")).unwrap();
        config.request.payload_max_bytes = 32;
        let worker = WorkerService::new(config);
        let valid = SubmitJobRequest {
            job_id: "job".into(),
            language_id: "python".into(),
            source: b"pass".to_vec(),
            cases: vec![TestCase::default()],
            run_limits: Some(RunLimits {
                cpu_time_ms: 1,
                wall_time_ms: 1,
                memory_kib: 1,
            }),
        };
        assert!(worker.validate_job(&valid).is_ok());
        let mut language = valid.clone();
        language.language_id = "unknown".into();
        let mut limits = valid.clone();
        limits.run_limits.as_mut().unwrap().memory_kib = 0;
        let mut payload = valid;
        payload.cases[0].input = vec![0; 32];
        for request in [language, limits, payload] {
            assert_eq!(
                worker.admit(request).unwrap_err().code(),
                tonic::Code::InvalidArgument
            );
        }
        assert!(worker.validate_id("").is_err());
        assert!(worker.shared.jobs.lock().unwrap().records.is_empty());
    }
}
