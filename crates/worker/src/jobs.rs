//! In-memory admission, immutable terminal records and acknowledged-result FIFO.

use std::collections::{HashMap, VecDeque};

use acorn_protocol::worker::v1::{GetJobReply, JobState};
use tokio::{sync::watch, task::JoinHandle};
use tonic::Status;

use crate::sandbox::StopReason;

/// Running progress or immutable terminal content with its ACK membership.
pub(crate) struct JobRecord {
    /// Bounded query response; never contains source or test data.
    pub reply: GetJobReply,
    /// Whether this terminal record has entered the acknowledged FIFO.
    pub acknowledged: bool,
}

/// Execution occupancy persists independently of result retention.
pub(crate) struct Execution {
    /// Current or unconfirmed-stop Job, even after its result is evicted.
    pub job_id: String,
    /// Internal stop sender, independent of the submit connection.
    pub stop: watch::Sender<Option<StopReason>>,
    /// Background execution handle taken only to await normal shutdown.
    pub task: Option<JoinHandle<()>>,
}

/// All admission and result transitions protected by one short synchronous lock.
#[derive(Default)]
pub(crate) struct Jobs {
    /// Running and retained terminal records keyed only by opaque Job ID.
    pub records: HashMap<String, JobRecord>,
    /// Ordered first acknowledgements; repeated ACK never changes this queue.
    acknowledged: VecDeque<String>,
    /// Single occupied execution position, including an unconfirmed stop.
    pub execution: Option<Execution>,
    /// New IDs are rejected once shutdown begins; existing records remain usable.
    pub stopping: bool,
}

impl Jobs {
    /// Count terminal results awaiting explicit confirmation, without duplicate counters.
    pub fn unacknowledged_count(&self) -> usize {
        self.records
            .values()
            .filter(|record| record.reply.state != JobState::Running as i32 && !record.acknowledged)
            .count()
    }

    /// ACK a terminal record once and evict only the oldest acknowledged results.
    pub fn acknowledge(&mut self, job_id: &str, capacity: usize) -> Result<(), Status> {
        let record = self
            .records
            .get_mut(job_id)
            .ok_or_else(|| Status::not_found("JobNotFound"))?;
        if record.reply.state == JobState::Running as i32 {
            return Err(Status::failed_precondition("ResultNotReady"));
        }
        if record.acknowledged {
            return Ok(());
        }
        record.acknowledged = true;
        self.acknowledged.push_back(job_id.to_owned());
        if self.acknowledged.len() > capacity {
            let oldest = self.acknowledged.pop_front().expect("nonempty ACK queue");
            self.records.remove(&oldest);
        }
        Ok(())
    }
}
