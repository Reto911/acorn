//! Reused status-only Worker channels and bounded health queries.

use crate::{
    config::GrpcConfig,
    health::{ExecutionState, ProbeError, ProbeErrorCode, WorkerStatus},
};
use acorn_protocol::worker::v1 as proto;
use std::time::Duration;
use tonic::{Code, Request, transport::Channel};

pub use proto::judge_worker_client::JudgeWorkerClient;

/// A cheap clone shares one lazy channel for its configured Worker address.
#[derive(Clone)]
pub(crate) struct WorkerClient {
    /// Generated client with deployment-aligned binary message limits.
    client: JudgeWorkerClient<Channel>,
}

impl WorkerClient {
    /// Build a channel without requiring the Worker to be reachable at startup.
    pub fn new(address: &str, grpc: &GrpcConfig) -> anyhow::Result<Self> {
        let channel = tonic::transport::Endpoint::from_shared(address.to_owned())?.connect_lazy();
        Ok(Self {
            client: JudgeWorkerClient::new(channel)
                .max_encoding_message_size(grpc.max_request_bytes)
                .max_decoding_message_size(grpc.max_response_bytes),
        })
    }

    /// Query status on the shared observation channel with an independent health budget and deadline.
    pub async fn status(&self, duration: Duration) -> Result<WorkerStatus, ProbeError> {
        let mut client = self.client.clone();
        let call = client.get_worker_status(request(proto::GetWorkerStatusRequest {}, duration));
        let reply = match tokio::time::timeout(duration, call).await {
            Ok(Ok(response)) => response.into_inner(),
            Ok(Err(status)) => {
                let code = match status.code() {
                    Code::DeadlineExceeded => ProbeErrorCode::ProbeTimeout,
                    // Tonic reports its typed transport timeout as Cancelled. A remote
                    // cancellation without that source remains an ordinary RPC failure.
                    Code::Cancelled
                        if std::iter::successors(std::error::Error::source(&status), |error| {
                            error.source()
                        })
                        .any(|error| error.is::<tonic::TimeoutExpired>()) =>
                    {
                        ProbeErrorCode::ProbeTimeout
                    }
                    Code::Unimplemented => ProbeErrorCode::WorkerStatusUnsupported,
                    _ => ProbeErrorCode::WorkerRpcFailed,
                };
                return Err(ProbeError::new(code, status));
            }
            Err(error) => return Err(ProbeError::new(ProbeErrorCode::ProbeTimeout, error)),
        };
        validate_status(reply)
    }
}

/// Accept only defined occupancy, positive capacity and counts within that capacity.
fn validate_status(reply: proto::GetWorkerStatusReply) -> Result<WorkerStatus, ProbeError> {
    let execution_state = match proto::WorkerExecutionState::try_from(reply.execution_state) {
        Ok(proto::WorkerExecutionState::Idle) => ExecutionState::Idle,
        Ok(proto::WorkerExecutionState::Running) => ExecutionState::Running,
        Ok(proto::WorkerExecutionState::Blocked) => ExecutionState::Blocked,
        _ => {
            return Err(ProbeError::new(
                ProbeErrorCode::InvalidWorkerStatus,
                "GetWorkerStatus returned an undefined execution state",
            ));
        }
    };
    if reply.unacknowledged_capacity == 0
        || reply.unacknowledged_results > reply.unacknowledged_capacity
    {
        return Err(ProbeError::new(
            ProbeErrorCode::InvalidWorkerStatus,
            "GetWorkerStatus returned an invalid result count or capacity",
        ));
    }
    Ok(WorkerStatus {
        stopping: reply.stopping,
        execution_state,
        unacknowledged_results: reply.unacknowledged_results,
        unacknowledged_capacity: reply.unacknowledged_capacity,
    })
}

/// Attach the server-visible deadline as well as the local bounded wait.
fn request<T>(message: T, duration: Duration) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(duration);
    request
}

#[cfg(test)]
mod tests {
    //! Contract checks for transient progress and terminal correspondence.

    use super::*;

    /// Occupancy and counts accept only the three status conditions agreed at the boundary.
    #[test]
    fn worker_status_contract() {
        let valid = proto::GetWorkerStatusReply {
            stopping: true,
            execution_state: proto::WorkerExecutionState::Idle as i32,
            unacknowledged_results: 1,
            unacknowledged_capacity: 1,
        };
        for (state, capacity, count) in [(0, 1, 0), (99, 1, 0), (-1, 1, 0), (1, 0, 0), (1, 1, 2)] {
            let error = validate_status(proto::GetWorkerStatusReply {
                execution_state: state,
                unacknowledged_capacity: capacity,
                unacknowledged_results: count,
                ..valid
            })
            .unwrap_err();
            assert_eq!(error.code, ProbeErrorCode::InvalidWorkerStatus);
        }
        for (state, expected) in [
            (proto::WorkerExecutionState::Idle, ExecutionState::Idle),
            (
                proto::WorkerExecutionState::Running,
                ExecutionState::Running,
            ),
            (
                proto::WorkerExecutionState::Blocked,
                ExecutionState::Blocked,
            ),
        ] {
            let status = validate_status(proto::GetWorkerStatusReply {
                execution_state: state as i32,
                ..valid
            })
            .unwrap();
            assert_eq!(status.execution_state, expected);
            assert!(status.stopping);
            assert_eq!(status.unacknowledged_results, 1);
            assert_eq!(status.unacknowledged_capacity, 1);
        }
    }
}
