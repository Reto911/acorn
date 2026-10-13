//! Public JSON shapes; private source, test data and Worker progress never appear.

use crate::{
    health,
    llm_review::{
        ManualReview, ReviewAttempt, ReviewError, ReviewMode, ReviewResult, ReviewStatus,
        ReviewTrigger,
    },
    problems, scheduler,
    submissions::{self, SubmissionState, TerminalDetails, Verdict},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Manual start or rerun input; the nullable expected identity must be explicitly present.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewRequest {
    /// Last observed review identity, normalized to the same UUID representation as storage.
    #[serde(deserialize_with = "expected_review_id")]
    pub expected_review_id: Option<String>,
}

/// Decode a required nullable UUID without treating an omitted field as null.
fn expected_review_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|id| {
            Uuid::parse_str(&id)
                .map(|id| id.to_string())
                .map_err(|_| serde::de::Error::custom("Expected a review UUID or null"))
        })
        .transpose()
}

/// Public latest-attempt fields, deliberately excluding internal policy and diagnostics.
#[derive(Serialize)]
struct Review {
    /// Identity used by the next manual request's expected_review_id.
    review_id: String,
    /// Automatic or manual origin of this attempt.
    trigger: ReviewTrigger,
    /// UTC Unix milliseconds at admission.
    requested_at_ms: i64,
    /// UTC Unix completion milliseconds, or null while pending.
    completed_at_ms: Option<i64>,
    /// Configured model for this attempt.
    model: String,
    /// Validated opinion, or null for pending and failed attempts.
    result: Option<ReviewResult>,
    /// Fixed stable error summary, or null when no failure exists.
    error: Option<ReviewError>,
}

impl From<ReviewAttempt> for Review {
    /// Select explicit public fields rather than serializing the persisted envelope.
    fn from(attempt: ReviewAttempt) -> Self {
        Self {
            review_id: attempt.id,
            trigger: attempt.details.trigger,
            requested_at_ms: attempt.details.requested_at_ms,
            completed_at_ms: attempt.details.completed_at_ms,
            model: attempt.details.model,
            result: attempt.details.result,
            error: attempt.details.error,
        }
    }
}

/// Snapshot acknowledging new manual admission or reuse of an existing pending attempt.
#[derive(Serialize)]
pub(super) struct ReviewResponse {
    /// Original submission, never a new Worker Job.
    submission_id: String,
    /// Independent review state at this operation's snapshot.
    review_status: ReviewStatus,
    /// Explicit public attempt metadata and opinion or error.
    review: Review,
}

impl From<ManualReview> for ReviewResponse {
    /// Preserve the service-owned snapshot without querying or starting another operation.
    fn from(value: ManualReview) -> Self {
        Self {
            submission_id: value.submission_id,
            review_status: value.review.status,
            review: value.review.into(),
        }
    }
}

/// Public component observations, with no internal endpoints, paths or diagnostics.
#[derive(Serialize)]
pub(super) struct HealthSnapshot {
    /// Backend UTC Unix milliseconds when HTTP assembled the snapshot.
    snapshot_at_ms: i64,
    /// Local admission conditions, independent of remote observations.
    submission_admission: &'static str,
    /// Actual local dispatcher lifecycle and semaphore usage.
    scheduler: SchedulerHealth,
    /// Latest database read observation, excluding its internal unit value.
    database: Probe,
    /// Worker observations in configuration order.
    workers: Vec<WorkerHealth>,
}

impl HealthSnapshot {
    /// Map only the agreed public fields from local state and the shared cache.
    pub fn new(
        snapshot_at_ms: i64,
        local: health::LocalSnapshot,
        cached: health::Snapshot,
    ) -> Self {
        Self {
            snapshot_at_ms,
            submission_admission: match local.submission_admission {
                health::SubmissionAdmission::Open => "open",
                health::SubmissionAdmission::Full => "full",
                health::SubmissionAdmission::Stopping => "stopping",
                health::SubmissionAdmission::Unavailable => "unavailable",
            },
            scheduler: SchedulerHealth {
                state: match local.scheduler.state {
                    scheduler::SchedulerState::Running => "running",
                    scheduler::SchedulerState::Stopping => "stopping",
                    scheduler::SchedulerState::Stopped => "stopped",
                },
                pending_used: local.scheduler.pending_used,
                pending_capacity: local.scheduler.pending_capacity,
            },
            database: cached.database.into(),
            workers: cached.workers.into_iter().map(Into::into).collect(),
        }
    }
}

/// Current local lifecycle and admission permit counts.
#[derive(Serialize)]
struct SchedulerHealth {
    /// Running, stopping or stopped, independently of Worker status.
    state: &'static str,
    /// Permits held by reserved, queued or awaiting-admission submissions.
    pending_used: u32,
    /// Fixed configured permit ceiling.
    pending_capacity: u32,
}

/// Common latest-probe metadata, whose absent fields remain explicit JSON nulls.
#[derive(Serialize)]
struct Probe {
    /// Unknown, ok, failed or stale after the monotonic age projection.
    state: &'static str,
    /// Backend UTC check completion time, distinct from HTTP assembly time.
    checked_at_ms: Option<i64>,
    /// Stable code for a fresh failure, never its original diagnostic.
    error_code: Option<&'static str>,
}

impl<T> From<health::ProbeSnapshot<T>> for Probe {
    /// Drop the internal value and map the stable observation vocabulary explicitly.
    fn from(probe: health::ProbeSnapshot<T>) -> Self {
        Self {
            state: match probe.state {
                health::ProbeState::Unknown => "unknown",
                health::ProbeState::Ok => "ok",
                health::ProbeState::Failed => "failed",
                health::ProbeState::Stale => "stale",
            },
            checked_at_ms: probe.checked_at_ms,
            error_code: probe.error_code.map(|code| match code {
                health::ProbeErrorCode::ProbeTimeout => "ProbeTimeout",
                health::ProbeErrorCode::DatabaseReadFailed => "DatabaseReadFailed",
                health::ProbeErrorCode::WorkerRpcFailed => "WorkerRpcFailed",
                health::ProbeErrorCode::WorkerStatusUnsupported => "WorkerStatusUnsupported",
                health::ProbeErrorCode::InvalidWorkerStatus => "InvalidWorkerStatus",
            }),
        }
    }
}

/// One configured Worker with flat probe fields and optional validated status.
#[derive(Serialize)]
struct WorkerHealth {
    /// Configuration-order label such as worker-1, excluding the endpoint.
    id: String,
    /// Metadata shared with the database observation.
    #[serde(flatten)]
    probe: Probe,
    /// Fresh validated status, or explicit null for unknown, failed and stale observations.
    status: Option<WorkerStatus>,
}

impl From<health::WorkerSnapshot> for WorkerHealth {
    /// Preserve only the label, projected metadata and validated value.
    fn from(worker: health::WorkerSnapshot) -> Self {
        Self {
            id: worker.id,
            status: worker.probe.value.map(Into::into),
            probe: worker.probe.into(),
        }
    }
}

/// Remote execution and result retention, without a synthesized health conclusion.
#[derive(Serialize)]
struct WorkerStatus {
    /// Whether the Worker is stopping admission.
    stopping: bool,
    /// Idle, running or blocked execution occupancy.
    execution_state: &'static str,
    /// Terminal results awaiting explicit ACK.
    unacknowledged_results: u64,
    /// Positive configured result-retention ceiling.
    unacknowledged_capacity: u64,
}

impl From<health::WorkerStatus> for WorkerStatus {
    /// Translate occupancy vocabulary while preserving the reported counters and stop flag.
    fn from(status: health::WorkerStatus) -> Self {
        Self {
            stopping: status.stopping,
            execution_state: match status.execution_state {
                health::ExecutionState::Idle => "idle",
                health::ExecutionState::Running => "running",
                health::ExecutionState::Blocked => "blocked",
            },
            unacknowledged_results: status.unacknowledged_results,
            unacknowledged_capacity: status.unacknowledged_capacity,
        }
    }
}

/// Allowed create fields only; execution data and owner are server-controlled.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRequest {
    /// Problem whose latest published version is selected.
    pub problem_id: i64,
    /// Built-in language identifier.
    pub language_id: String,
    /// Original UTF-8 source, with whitespace and newlines preserved.
    pub source: String,
}

/// Minimal response acknowledging persistence and local handoff.
#[derive(Serialize)]
pub(super) struct CreateResponse {
    /// API-generated UUID, also used as the Worker Job ID.
    pub submission_id: String,
    /// Persisted business state observed during creation.
    pub state: SubmissionState,
}

/// Current-version identity for a problem list.
#[derive(Serialize)]
pub(super) struct ProblemSummary {
    /// Positive display and lookup number.
    problem_id: i64,
    /// Latest published version number.
    version_no: i64,
    /// Published title.
    title: String,
}

impl From<problems::ProblemSummary> for ProblemSummary {
    /// Map list fields explicitly, excluding private database content.
    fn from(value: problems::ProblemSummary) -> Self {
        Self {
            problem_id: value.problem_id,
            version_no: value.version_no,
            title: value.title,
        }
    }
}

/// One permitted language with per-case budgets in milliseconds and KiB.
#[derive(Serialize)]
struct Language {
    /// Built-in c or python identifier.
    language_id: String,
    /// CPU limit per case in milliseconds.
    cpu_time_ms: u64,
    /// Wall-clock limit per case in milliseconds.
    wall_time_ms: u64,
    /// Memory limit per case in KiB.
    memory_kib: u64,
}

/// Latest or explicit-version public problem detail.
#[derive(Serialize)]
pub(super) struct Problem {
    /// Flat identity and title, shared with the list.
    #[serde(flatten)]
    summary: ProblemSummary,
    /// Immutable statement belonging to this version.
    statement: String,
    /// Allowed languages and their limits, excluding all tests and answers.
    languages: Vec<Language>,
    /// Automatic policy fixed in this published version.
    review_mode: ReviewMode,
}

impl From<problems::ProblemVersion> for Problem {
    /// Convert only the documented public fields.
    fn from(value: problems::ProblemVersion) -> Self {
        Self {
            summary: value.summary.into(),
            statement: value.statement,
            review_mode: value.review_mode,
            languages: value
                .languages
                .into_iter()
                .map(|language| Language {
                    language_id: language.language_id,
                    cpu_time_ms: language.limits.cpu_time_ms,
                    wall_time_ms: language.limits.wall_time_ms,
                    memory_kib: language.limits.memory_kib,
                })
                .collect(),
        }
    }
}

/// Submission history fields omit source and terminal diagnostics.
#[derive(Serialize)]
pub(super) struct SubmissionSummary {
    /// UUID exposed with the HTTP contract's field name.
    submission_id: String,
    /// Fixed problem number.
    problem_id: i64,
    /// Version fixed at creation, unaffected by later publication.
    version_no: i64,
    /// Fixed language.
    language_id: String,
    /// Persisted business state, never a transient Worker phase.
    state: SubmissionState,
    /// Independent verdict, or null before a judging terminal.
    verdict: Option<Verdict>,
    /// Latest review state, independent of the machine verdict.
    review_status: ReviewStatus,
    /// UTC Unix creation milliseconds.
    created_at_ms: i64,
    /// UTC Unix terminal-save milliseconds, or null until terminal.
    terminal_at_ms: Option<i64>,
}

impl From<submissions::SubmissionInfo> for SubmissionSummary {
    /// Rename the database identity without returning stored source or ownership data.
    fn from(value: submissions::SubmissionInfo) -> Self {
        Self {
            submission_id: value.id,
            problem_id: value.problem_id,
            version_no: value.version_no,
            language_id: value.language_id,
            state: value.state,
            verdict: value.verdict,
            review_status: value.review_status,
            created_at_ms: value.created_at_ms,
            terminal_at_ms: value.terminal_at_ms,
        }
    }
}

/// Parsed terminal details remain an object rather than a JSON-encoded string.
#[derive(Serialize)]
pub(super) struct Submission {
    /// Flat identity, fixed target, status and timestamps.
    #[serde(flatten)]
    summary: SubmissionSummary,
    /// Typed Base64-bearing details; null for AC and nonterminal records.
    terminal_details: Option<TerminalDetails>,
    /// Latest public attempt snapshot, or null when no review has been admitted.
    review: Option<Review>,
}

impl From<submissions::Submission> for Submission {
    /// Preserve typed terminal shape and optional resource statistics.
    fn from(value: submissions::Submission) -> Self {
        Self {
            summary: value.info.into(),
            terminal_details: value.terminal_details,
            review: value.review.map(Into::into),
        }
    }
}

/// Shared pagination envelope, deliberately without an extra total-count query.
#[derive(Serialize)]
pub(super) struct Page<T> {
    /// Items in the documented deterministic order.
    pub items: Vec<T>,
    /// Effective configured or explicitly requested page length.
    pub limit: u32,
    /// Requested zero-based item offset.
    pub offset: u64,
}

/// Problem-list pagination parameters; unrecognized fields are rejected.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    /// Optional page length, defaulting to the deployment configuration.
    pub limit: Option<u32>,
    /// Optional nonnegative item offset, defaulting to zero.
    pub offset: Option<u64>,
}

/// Submission history pagination and the documented machine and review filters.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HistoryQuery {
    /// Optional page length.
    pub limit: Option<u32>,
    /// Optional nonnegative offset.
    pub offset: Option<u64>,
    /// Optional positive problem number.
    pub problem_id: Option<i64>,
    /// Optional persisted business state.
    pub state: Option<SubmissionState>,
    /// Optional judging conclusion.
    pub verdict: Option<Verdict>,
    /// Optional independent review state, combined with all other filters using AND.
    pub review_status: Option<ReviewStatus>,
}

#[cfg(test)]
mod tests {
    //! Public health JSON contract, including fixed null fields and vocabulary.

    use super::*;
    use serde_json::json;

    /// Manual requests distinguish an explicit null from omission and reject unallowed input.
    #[test]
    fn manual_review_request_requires_nullable_uuid() {
        assert!(
            serde_json::from_value::<ReviewRequest>(json!({"expected_review_id":null}))
                .unwrap()
                .expected_review_id
                .is_none()
        );
        let id = Uuid::new_v4();
        assert_eq!(
            serde_json::from_value::<ReviewRequest>(
                json!({"expected_review_id":id.to_string().to_uppercase()})
            )
            .unwrap()
            .expected_review_id,
            Some(id.to_string())
        );
        for invalid in [
            json!({}),
            json!({"expected_review_id":3}),
            json!({"expected_review_id":"bad-id"}),
            json!({"expected_review_id":null,"user_id":1}),
        ] {
            assert!(serde_json::from_value::<ReviewRequest>(invalid).is_err());
        }
    }

    /// Unknown and stale values retain explicit nulls; failures expose only stable codes.
    #[test]
    fn health_probe_json_contract() {
        for (state, name, checked_at_ms, error_code) in [
            (health::ProbeState::Unknown, "unknown", None, None),
            (health::ProbeState::Ok, "ok", Some(41), None),
            (
                health::ProbeState::Failed,
                "failed",
                Some(41),
                Some(health::ProbeErrorCode::WorkerRpcFailed),
            ),
            (health::ProbeState::Stale, "stale", Some(41), None),
        ] {
            let probe = health::ProbeSnapshot::<health::WorkerStatus> {
                state,
                checked_at_ms,
                error_code,
                value: (state == health::ProbeState::Ok).then_some(health::WorkerStatus {
                    stopping: false,
                    execution_state: health::ExecutionState::Idle,
                    unacknowledged_results: 0,
                    unacknowledged_capacity: 128,
                }),
            };
            let worker = WorkerHealth::from(health::WorkerSnapshot {
                id: "worker-1".into(),
                probe,
            });
            assert_eq!(
                serde_json::to_value(worker).unwrap(),
                json!({
                    "id":"worker-1", "state":name, "checked_at_ms":checked_at_ms,
                    "error_code":if state == health::ProbeState::Failed {Some("WorkerRpcFailed")} else {None},
                "status":if state == health::ProbeState::Ok {json!({"stopping":false, "execution_state":"idle", "unacknowledged_results":0, "unacknowledged_capacity":128})} else {serde_json::Value::Null},
                })
            );
        }
        for (code, name) in [
            (health::ProbeErrorCode::ProbeTimeout, "ProbeTimeout"),
            (
                health::ProbeErrorCode::DatabaseReadFailed,
                "DatabaseReadFailed",
            ),
            (health::ProbeErrorCode::WorkerRpcFailed, "WorkerRpcFailed"),
            (
                health::ProbeErrorCode::WorkerStatusUnsupported,
                "WorkerStatusUnsupported",
            ),
            (
                health::ProbeErrorCode::InvalidWorkerStatus,
                "InvalidWorkerStatus",
            ),
        ] {
            let probe = Probe::from(health::ProbeSnapshot::<()> {
                state: health::ProbeState::Failed,
                checked_at_ms: Some(41),
                error_code: Some(code),
                value: None,
            });
            assert_eq!(
                serde_json::to_value(probe).unwrap(),
                json!({"state":"failed", "checked_at_ms":41, "error_code":name})
            );
        }
    }

    /// Local and execution enum values serialize to the fixed browser-facing vocabulary.
    #[test]
    fn health_state_json_contract() {
        for (admission, name) in [
            (health::SubmissionAdmission::Open, "open"),
            (health::SubmissionAdmission::Full, "full"),
            (health::SubmissionAdmission::Stopping, "stopping"),
            (health::SubmissionAdmission::Unavailable, "unavailable"),
        ] {
            for (lifecycle, lifecycle_name) in [
                (scheduler::SchedulerState::Running, "running"),
                (scheduler::SchedulerState::Stopping, "stopping"),
                (scheduler::SchedulerState::Stopped, "stopped"),
            ] {
                let body = HealthSnapshot::new(
                    42,
                    health::LocalSnapshot {
                        submission_admission: admission,
                        scheduler: scheduler::Snapshot {
                            state: lifecycle,
                            pending_used: 1,
                            pending_capacity: 2,
                        },
                    },
                    health::Snapshot {
                        database: health::ProbeSnapshot {
                            state: health::ProbeState::Ok,
                            checked_at_ms: Some(41),
                            error_code: None,
                            value: Some(()),
                        },
                        workers: Vec::new(),
                    },
                );
                assert_eq!(
                    serde_json::to_value(body).unwrap(),
                    json!({
                        "snapshot_at_ms":42, "submission_admission":name,
                        "scheduler":{"state":lifecycle_name, "pending_used":1, "pending_capacity":2},
                        "database":{"state":"ok", "checked_at_ms":41, "error_code":null}, "workers":[],
                    })
                );
            }
        }
        for (execution_state, name) in [
            (health::ExecutionState::Idle, "idle"),
            (health::ExecutionState::Running, "running"),
            (health::ExecutionState::Blocked, "blocked"),
        ] {
            let status = WorkerStatus::from(health::WorkerStatus {
                stopping: true,
                execution_state,
                unacknowledged_results: 2,
                unacknowledged_capacity: 2,
            });
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                json!({"stopping":true, "execution_state":name, "unacknowledged_results":2, "unacknowledged_capacity":2})
            );
        }
    }
}
