//! Unary RPC adapters over the shared admission and execution runtime.

pub use crate::runtime::WorkerService;
pub use acorn_protocol::worker::v1::judge_worker_server::{JudgeWorker, JudgeWorkerServer};
use acorn_protocol::worker::v1::{
    AcknowledgeResultReply, AcknowledgeResultRequest, GetJobReply, GetJobRequest,
    GetWorkerStatusReply, GetWorkerStatusRequest, SubmitJobReply, SubmitJobRequest,
    WorkerExecutionState,
};
use tonic::{Request, Response, Status};

impl WorkerService {
    /// Apply configured request and response transport limits to the generated server.
    pub fn into_server(self) -> JudgeWorkerServer<Self> {
        let request = self.shared.config.server.max_request_bytes;
        let response = self.shared.config.server.max_response_bytes;
        JudgeWorkerServer::new(self)
            .max_decoding_message_size(request)
            .max_encoding_message_size(response)
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
        Ok(Response::new(self.admit(request)?.reply))
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
        Ok(Response::new(record.updates.borrow().clone()))
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
