//! Independent Worker-group observations used only for new-submission admission.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

use crate::{config::WorkerMembershipConfig, scheduler::kafka::Observer};

/// Latest completed membership observation; freshness controls only the usable count.
struct Completed {
    /// Monotonic completion for admission freshness.
    at: Instant,
    /// Completion timestamp retained independently of health collection.
    at_ms: i64,
    /// Query error replaces any earlier successful count.
    result: Result<usize, String>,
}

/// Read-only observation without an available count when unknown, failed or stale.
#[derive(Debug)]
pub struct Snapshot {
    /// Fresh observed members; a positive number does not indicate idle capacity.
    pub members: Option<usize>,
    /// Latest completion, including a failed or expired observation.
    pub checked_at_ms: Option<i64>,
    /// Latest query error, retained for diagnostics rather than exposed by health HTTP.
    pub error: Option<String>,
}

/// Dedicated observation cache and stop signal, independent of health lifecycle.
#[derive(Clone)]
pub struct Membership {
    /// Only this collector writes the latest observation.
    latest: Arc<Mutex<Option<Completed>>>,
    /// Maximum age derived from the membership collection configuration.
    stale_after: Duration,
    /// Explicitly stop collection without affecting business services.
    stop: watch::Sender<bool>,
}

impl Membership {
    /// Construct an unknown cache whose owner may supply completed observations.
    pub fn new(config: &WorkerMembershipConfig) -> Result<Self> {
        Ok(Self {
            latest: Arc::new(Mutex::new(None)),
            stale_after: config.stale_after()?,
            stop: watch::channel(false).0,
        })
    }

    /// Start the independent collector, returning its cache and supervised task.
    pub fn start(
        config: &WorkerMembershipConfig,
        observer: Observer,
    ) -> Result<(Self, JoinHandle<Result<()>>)> {
        let membership = Self::new(config)?;
        let task = tokio::spawn(collect(membership.clone(), config.clone(), observer));
        Ok((membership, task))
    }

    /// Replace the latest sample after a completed query; failures discard older counts.
    pub fn record(&self, result: Result<usize, String>) -> Result<()> {
        let at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
            .context("Worker membership completion time exceeds Unix millisecond range")?;
        *self.latest.lock().expect("Membership cache lock poisoned") = Some(Completed {
            at: Instant::now(),
            at_ms,
            result,
        });
        Ok(())
    }

    /// Read only the dedicated cache, applying the monotonic freshness boundary.
    pub fn snapshot(&self) -> Snapshot {
        let latest = self.latest.lock().expect("Membership cache lock poisoned");
        Snapshot {
            members: latest
                .as_ref()
                .filter(|sample| sample.at.elapsed() < self.stale_after)
                .and_then(|sample| sample.result.as_ref().ok().copied()),
            checked_at_ms: latest.as_ref().map(|sample| sample.at_ms),
            error: latest
                .as_ref()
                .and_then(|sample| sample.result.as_ref().err().cloned()),
        }
    }

    /// Stop before another query; an already running SDK call finishes within its budget.
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
}

/// Query on a blocking thread, never overlapping rounds or detaching an active SDK call.
async fn collect(
    membership: Membership,
    config: WorkerMembershipConfig,
    observer: Observer,
) -> Result<()> {
    let mut stopped = membership.stop.subscribe();
    while !*stopped.borrow() {
        let client = observer.clone();
        let group = config.tasks_group_id.clone();
        let timeout = Duration::from_millis(config.timeout_ms);
        let result = tokio::task::spawn_blocking(move || client.members(&group, timeout))
            .await
            .context("Worker membership query task failed")?;
        let result = result.map_err(|error| {
            tracing::warn!(%error, "Worker membership query failed");
            error.to_string()
        });
        let members = result.as_ref().ok().copied();
        membership.record(result)?;
        if let Some(members) = members {
            tracing::debug!(members, "Worker membership probe completed");
        }
        tokio::select! {
            _ = stopped.changed() => {},
            _ = tokio::time::sleep(Duration::from_millis(config.interval_ms)) => {},
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Freshness boundaries do not require sleeping or a broker.
    use super::*;

    /// Failed and expired samples cannot retain a previous positive count.
    #[test]
    fn unknown_failure_and_expiration() {
        let config = WorkerMembershipConfig::default();
        let membership = Membership::new(&config).unwrap();
        assert_eq!(membership.snapshot().members, None);
        for members in [0, 2] {
            membership.record(Ok(members)).unwrap();
            assert_eq!(membership.snapshot().members, Some(members));
        }
        membership.record(Err("query failed".into())).unwrap();
        assert_eq!(membership.snapshot().members, None);
        assert_eq!(membership.snapshot().error.as_deref(), Some("query failed"));
        membership.record(Ok(2)).unwrap();
        membership.latest.lock().unwrap().as_mut().unwrap().at -= membership.stale_after;
        assert_eq!(membership.snapshot().members, None);
        assert!(membership.snapshot().checked_at_ms.is_some());
    }
}
