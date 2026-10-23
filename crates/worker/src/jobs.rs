//! Execution occupancy and bounded notifications retained until Kafka handoff.

use std::collections::HashMap;

use acorn_protocol::worker::v1::GetJobReply;
use tokio::{sync::watch, task::JoinHandle};

use crate::sandbox::StopReason;

/// Execution occupancy persists independently of result retention.
pub(crate) struct Execution {
    /// Current or unconfirmed-stop Job, even after its result is evicted.
    pub job_id: String,
    /// Whether execution returned without confirming a safe stop of its process tree.
    pub stop_unconfirmed: bool,
    /// Internal stop sender, independent of the submit connection.
    pub stop: watch::Sender<Option<StopReason>>,
    /// Background execution handle taken only to await normal shutdown.
    pub task: Option<JoinHandle<()>>,
}

/// Execution state, runtime snapshots and MQ-count projection under one short lock.
#[derive(Default)]
pub(crate) struct Jobs {
    /// Running and retained terminal records keyed only by opaque Job ID.
    pub records: HashMap<String, watch::Sender<GetJobReply>>,
    /// Single occupied execution position, including an unconfirmed stop.
    pub execution: Option<Execution>,
    /// New IDs are rejected once shutdown begins; existing records remain usable.
    pub stopping: bool,
    /// Terminal FIFO entries, including the in-flight MQ handoff; excludes execution reservations.
    pub mq_results: usize,
}
