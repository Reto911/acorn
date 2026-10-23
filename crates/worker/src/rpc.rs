//! Read-only status RPC over the shared execution runtime.

pub use crate::runtime::WorkerService;
pub use acorn_protocol::worker::v1::judge_worker_server::{JudgeWorker, JudgeWorkerServer};
use acorn_protocol::worker::v1::{
    GetWorkerStatusReply, GetWorkerStatusRequest, WorkerExecutionState,
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
            unacknowledged_results: jobs.mq_results as u64,
            unacknowledged_capacity: self.shared.config.results.max_unacknowledged_results as u64,
        }))
    }
}
