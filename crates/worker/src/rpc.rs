//! Short unary RPCs with admission independent of execution and submit connections.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use acorn_protocol::worker::v1::{
    AcknowledgeResultReply, AcknowledgeResultRequest, GetJobReply, GetJobRequest,
    GetWorkerStatusReply, GetWorkerStatusRequest, JobPhase, JobState, SubmitJobReply,
    SubmitJobRequest, WorkerExecutionState, get_job_reply::Terminal,
};
use anyhow::{Context, Result, ensure};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    time::Instant,
};
use tonic::{Request, Response, Status};
use tracing::Instrument;

use crate::{
    config::Config,
    executor::{ExecutionProgress, Executor},
    jobs::{Execution, JobRecord, Jobs},
    sandbox::{StageControl, StopReason},
};

pub use acorn_protocol::worker::v1::judge_worker_server::{JudgeWorker, JudgeWorkerServer};

/// Shared handles; the result lock never spans asynchronous execution or I/O.
struct Shared {
    /// Startup configuration and trust-boundary limits.
    config: Config,
    /// Atomic admission, progress, terminal and ACK transitions.
    jobs: Mutex<Jobs>,
    /// Exactly one executor, retained after an unconfirmed stop.
    executor: AsyncMutex<Executor>,
}

/// Cloneable RPC service with process-local deduplication and result retention.
#[derive(Clone)]
pub struct WorkerService {
    /// Shared state owned by the listener and admitted execution tasks.
    shared: Arc<Shared>,
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

    /// Apply configured request and response transport limits to the generated server.
    pub fn into_server(self) -> JudgeWorkerServer<Self> {
        let request = self.shared.config.server.max_request_bytes;
        let response = self.shared.config.server.max_response_bytes;
        JudgeWorkerServer::new(self)
            .max_decoding_message_size(request)
            .max_encoding_message_size(response)
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
    fn validate_id(&self, job_id: &str) -> Result<(), Status> {
        if job_id.is_empty() || job_id.len() > self.shared.config.request.job_id_max_bytes {
            return Err(Status::invalid_argument("InvalidJob"));
        }
        Ok(())
    }

    /// Check only new-request language, byte budgets, case count and resource ceilings.
    fn validate_job(&self, request: &SubmitJobRequest) -> Result<(), Status> {
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
            state: record.reply.state,
            duplicate: true,
        })
    }
}

#[tonic::async_trait]
impl JudgeWorker for WorkerService {
    /// Admit a new Job atomically, then return while execution continues in the background.
    async fn submit_job(
        &self,
        request: Request<SubmitJobRequest>,
    ) -> Result<Response<SubmitJobReply>, Status> {
        let request = request.into_inner();
        self.validate_id(&request.job_id)?;
        {
            let jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
            if let Some(reply) = Self::duplicate(&jobs, &request.job_id) {
                tracing::debug!(job = %request.job_id, "Duplicate Job submission");
                return Ok(Response::new(reply));
            }
        }
        self.validate_job(&request)?;
        let mut jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        if let Some(reply) = Self::duplicate(&jobs, &request.job_id) {
            return Ok(Response::new(reply));
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
        let (stop, receiver) = watch::channel(None);
        jobs.records.insert(
            job_id.clone(),
            JobRecord {
                reply: GetJobReply {
                    job_id: job_id.clone(),
                    state: JobState::Running as i32,
                    phase: JobPhase::Preparing as i32,
                    completed_cases: 0,
                    total_cases: request.cases.len() as u32,
                    terminal: None,
                },
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
                        stop: receiver,
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
        Ok(Response::new(SubmitJobReply {
            job_id,
            state: JobState::Running as i32,
            duplicate: false,
        }))
    }

    /// Clone current progress or terminal content without changing retention state.
    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<GetJobReply>, Status> {
        let job_id = request.into_inner().job_id;
        self.validate_id(&job_id)?;
        let jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        let record = jobs
            .records
            .get(&job_id)
            .ok_or_else(|| Status::not_found("JobNotFound"))?;
        tracing::debug!(job = %job_id, "Job queried");
        Ok(Response::new(record.reply.clone()))
    }

    /// Permit FIFO eviction only after explicit terminal-result confirmation.
    async fn acknowledge_result(
        &self,
        request: Request<AcknowledgeResultRequest>,
    ) -> Result<Response<AcknowledgeResultReply>, Status> {
        let job_id = request.into_inner().job_id;
        self.validate_id(&job_id)?;
        self.shared
            .jobs
            .lock()
            .expect("Job state lock poisoned")
            .acknowledge(
                &job_id,
                self.shared.config.results.acknowledged_queue_length,
            )?;
        tracing::debug!(job = %job_id, "Result acknowledged");
        Ok(Response::new(AcknowledgeResultReply {}))
    }

    /// Snapshot occupancy and retained-result capacity without waiting for the executor.
    async fn get_worker_status(
        &self,
        _request: Request<GetWorkerStatusRequest>,
    ) -> Result<Response<GetWorkerStatusReply>, Status> {
        let jobs = self.shared.jobs.lock().expect("Job state lock poisoned");
        let execution_state = match &jobs.execution {
            None => WorkerExecutionState::Idle,
            Some(execution) if execution.stop_unconfirmed => WorkerExecutionState::Blocked,
            Some(_) => WorkerExecutionState::Running,
        };
        tracing::debug!("Worker status queried");
        Ok(Response::new(GetWorkerStatusReply {
            stopping: jobs.stopping,
            execution_state: execution_state as i32,
            unacknowledged_results: jobs.unacknowledged_count() as u64,
            unacknowledged_capacity: self.shared.config.results.max_unacknowledged_results as u64,
        }))
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
                record.reply.phase = progress.phase as i32;
                record.reply.completed_cases = progress.completed_cases;
            }
        })
        .await;
    let release = executor.is_available();
    drop(executor);
    let mut jobs = shared.jobs.lock().expect("Job state lock poisoned");
    let record = jobs.records.get_mut(&job_id).expect("running Job record");
    record.reply.state = match outcome.terminal {
        Terminal::Result(_) => JobState::Finished as i32,
        Terminal::Failure(_) => JobState::Failed as i32,
    };
    record.reply.phase = JobPhase::Done as i32;
    record.reply.completed_cases = outcome.completed_cases;
    record.reply.terminal = Some(outcome.terminal);
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
