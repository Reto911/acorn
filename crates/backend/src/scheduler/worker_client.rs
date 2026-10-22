//! Reused Worker channels, bounded unary calls and validation at the RPC boundary.

use std::{future::Future, time::Duration};

use acorn_protocol::worker::v1::{
    self as proto, AcknowledgeResultRequest, GetJobRequest, JobState, SubmitJobRequest,
};
use tonic::{Code, Request, Response, Status, transport::Channel};

use crate::{
    config::{GrpcConfig, ResultsConfig},
    health::{ExecutionState, ProbeError, ProbeErrorCode, WorkerStatus},
};

use super::validation::{Observation, validate};
pub use proto::judge_worker_client::JudgeWorkerClient;

/// Classification used to decide whether admission was explicitly refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    /// Busy, full result buffer or shutdown, with no remote registration.
    Deferred,
    /// The Worker explicitly rejected invalid task data.
    Rejected,
    /// A call failed without a trustworthy admission decision.
    Unavailable,
    /// The queried Job no longer exists at the selected address.
    NotFound,
    /// A successful response violated the agreed protocol shape.
    InvalidReply,
}

/// RPC context retained for logs and a bounded persisted execution failure.
#[derive(Debug, thiserror::Error)]
#[error("{diagnostic}")]
pub(crate) struct Error {
    /// Control-flow classification, distinct from judging verdicts.
    pub kind: ErrorKind,
    /// Stable execution failure or rejection reason.
    pub code: &'static str,
    /// Actual transport or protocol context, never exposed as an HTTP error.
    pub diagnostic: String,
}

impl Error {
    /// Convert only recognized status/reason pairs into explicit refusal.
    fn status(status: Status) -> Self {
        let (kind, code) = match (status.code(), status.message()) {
            (Code::ResourceExhausted, "WorkerBusy") => (ErrorKind::Deferred, "WorkerBusy"),
            (Code::ResourceExhausted, "ResultBufferFull") => {
                (ErrorKind::Deferred, "ResultBufferFull")
            }
            (Code::FailedPrecondition, "WorkerUnavailable") => {
                (ErrorKind::Deferred, "WorkerUnavailable")
            }
            (Code::InvalidArgument, "InvalidJob") => (ErrorKind::Rejected, "InvalidJob"),
            (Code::InvalidArgument, "UnsupportedLanguage") => {
                (ErrorKind::Rejected, "UnsupportedLanguage")
            }
            (Code::NotFound, "JobNotFound") => (ErrorKind::NotFound, "WorkerJobNotFound"),
            _ => (ErrorKind::Unavailable, "WorkerUnavailable"),
        };
        Self {
            kind,
            code,
            diagnostic: status.to_string(),
        }
    }

    /// Describe a response that cannot safely drive a business state transition.
    fn invalid(message: impl ToString) -> Self {
        Self {
            kind: ErrorKind::InvalidReply,
            code: "InvalidWorkerResponse",
            diagnostic: message.to_string(),
        }
    }
}

/// A cheap clone shares one lazy channel for its configured Worker address.
#[derive(Clone)]
pub(crate) struct WorkerClient {
    /// Generated client with deployment-aligned binary message limits.
    client: JudgeWorkerClient<Channel>,
    /// Independent per-operation deadlines.
    grpc: GrpcConfig,
    /// Raw result budgets, before Base64 serialization.
    results: ResultsConfig,
}

impl WorkerClient {
    /// Build a channel without requiring the Worker to be reachable at startup.
    pub fn new(address: &str, grpc: &GrpcConfig, results: &ResultsConfig) -> anyhow::Result<Self> {
        let channel = tonic::transport::Endpoint::from_shared(address.to_owned())?.connect_lazy();
        Ok(Self {
            client: JudgeWorkerClient::new(channel)
                .max_encoding_message_size(grpc.max_request_bytes)
                .max_decoding_message_size(grpc.max_response_bytes),
            grpc: grpc.clone(),
            results: results.clone(),
        })
    }

    /// Offer a task exactly once, accepting only a matching, valid reply.
    pub async fn submit(&self, task: SubmitJobRequest) -> Result<(), Error> {
        let id = task.job_id.clone();
        let duration = Duration::from_millis(self.grpc.submit_timeout_ms);
        let reply = bounded(
            duration,
            self.client.clone().submit_job(request(task, duration)),
        )
        .await?;
        let state = JobState::try_from(reply.state).ok();
        if reply.job_id != id
            || !matches!(
                state,
                Some(JobState::Running | JobState::Finished | JobState::Failed)
            )
            || (!reply.duplicate && state != Some(JobState::Running))
        {
            return Err(Error::invalid(
                "SubmitJob returned an inconsistent identity or state",
            ));
        }
        Ok(())
    }

    /// Query the original address and ID, checking shape against the fixed task.
    pub async fn get(&self, id: &str, language: &str, total: u32) -> Result<Observation, Error> {
        let duration = Duration::from_millis(self.grpc.get_timeout_ms);
        let reply = bounded(
            duration,
            self.client
                .clone()
                .get_job(request(GetJobRequest { job_id: id.into() }, duration)),
        )
        .await?;
        validate(reply, id, language, total, &self.results).map_err(Error::invalid)
    }

    /// Confirm only after the caller has committed its business terminal.
    pub async fn acknowledge(&self, id: &str) -> Result<(), Error> {
        let duration = Duration::from_millis(self.grpc.ack_timeout_ms);
        bounded(
            duration,
            self.client.clone().acknowledge_result(request(
                AcknowledgeResultRequest { job_id: id.into() },
                duration,
            )),
        )
        .await?;
        Ok(())
    }

    /// Query status on the dispatch channel with an independent health budget and deadline.
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

/// Bound connection establishment and response waiting without resending the call.
async fn bounded<T>(
    duration: Duration,
    call: impl Future<Output = Result<Response<T>, Status>>,
) -> Result<T, Error> {
    match tokio::time::timeout(duration, call).await {
        Ok(response) => response.map(Response::into_inner).map_err(Error::status),
        Err(_) => Err(Error::status(Status::deadline_exceeded(
            "Worker RPC deadline exceeded",
        ))),
    }
}

#[cfg(test)]
mod tests {
    //! Contract checks for transient progress and terminal correspondence.

    use super::*;
    use acorn_protocol::worker::v1::{GetJobReply, JobPhase, get_job_reply::Terminal};

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

    /// Valid progress stays transient; mismatched phases and completed counts fail validation.
    #[test]
    fn progress_and_terminal_correspondence() {
        let config =
            crate::config::Config::parse(include_str!("../../../../config/backend.example.toml"))
                .unwrap();
        let mut reply = GetJobReply {
            job_id: "job".into(),
            state: JobState::Running as i32,
            phase: JobPhase::RunningCase as i32,
            total_cases: 2,
            ..Default::default()
        };
        assert!(matches!(
            validate(reply.clone(), "job", "python", 2, &config.results),
            Ok(Observation::Running)
        ));
        for (phase, completed) in [
            (JobPhase::Preparing, 0),
            (JobPhase::Compiling, 0),
            (JobPhase::RunningCase, 1),
            (JobPhase::Checking, 2),
        ] {
            let mut snapshot = reply.clone();
            snapshot.phase = phase.into();
            snapshot.completed_cases = completed;
            assert!(matches!(
                validate(snapshot, "job", "c", 2, &config.results),
                Ok(Observation::Running)
            ));
        }
        reply.phase = JobPhase::Compiling as i32;
        assert!(validate(reply.clone(), "job", "python", 2, &config.results).is_err());
        reply.phase = JobPhase::Done as i32;
        reply.state = JobState::Finished as i32;
        reply.terminal = Some(Terminal::Result(proto::JudgeResult {
            verdict: proto::Verdict::Accepted as i32,
            ..Default::default()
        }));
        reply.completed_cases = 1;
        assert!(validate(reply.clone(), "job", "c", 2, &config.results).is_err());
        reply.completed_cases = 2;
        assert!(matches!(
            validate(reply.clone(), "job", "c", 2, &config.results),
            Ok(Observation::Terminal(_))
        ));
        reply.state = JobState::Failed as i32;
        assert!(validate(reply, "job", "c", 2, &config.results).is_err());
    }
}
