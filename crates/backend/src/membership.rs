//! Independent Worker-group observations used only for new-submission admission.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use rdkafka::{error::KafkaError, types::RDKafkaErrorCode};
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
    /// Stable SDK category used for transition logging; non-SDK failures use None.
    error_code: Option<RDKafkaErrorCode>,
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
        self.record_observation(result, None)
    }

    /// Update freshness on every result and emit warnings only when its failure category changes.
    fn record_observation(
        &self,
        result: Result<usize, String>,
        error_code: Option<RDKafkaErrorCode>,
    ) -> Result<()> {
        let at_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
            .context("Worker membership completion time exceeds Unix millisecond range")?;
        let members = result.as_ref().ok().copied();
        let (warning, recovered) = {
            let mut latest = self.latest.lock().expect("Membership cache lock poisoned");
            let changed = latest.as_ref().is_none_or(|previous| {
                previous.result.is_ok() || previous.error_code != error_code
            });
            let warning = result.as_ref().err().filter(|_| changed).cloned();
            let recovered = latest
                .as_ref()
                .is_some_and(|previous| previous.result.is_err())
                && result.is_ok();
            *latest = Some(Completed {
                at: Instant::now(),
                at_ms,
                result,
                error_code,
            });
            (warning, recovered)
        };
        if let Some(error) = warning {
            tracing::warn!(?error_code, %error, "Worker membership query failed");
        } else if recovered {
            tracing::info!(members, "Worker membership probe recovered");
        }
        if let Some(members) = members {
            tracing::debug!(members, "Worker membership probe completed");
        }
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
        let error_code = result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<KafkaError>())
            .and_then(KafkaError::rdkafka_error_code);
        membership.record_observation(result.map_err(|error| error.to_string()), error_code)?;
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
    use std::io::{Read, Seek};

    /// Failed samples replace counts without repeating warnings; expiry still hides recovered counts.
    #[test]
    fn unknown_failure_and_expiration() {
        let config = WorkerMembershipConfig::default();
        let membership = Membership::new(&config).unwrap();
        let mut logs = tempfile::tempfile().unwrap();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(logs.try_clone().unwrap())
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        assert_eq!(membership.snapshot().members, None);
        for members in [0, 2] {
            membership.record(Ok(members)).unwrap();
            assert_eq!(membership.snapshot().members, Some(members));
        }
        membership.record(Err("query failed".into())).unwrap();
        assert_eq!(membership.snapshot().members, None);
        assert_eq!(membership.snapshot().error.as_deref(), Some("query failed"));
        for (diagnostic, code) in [
            ("same category", None),
            (
                "transport failed",
                Some(RDKafkaErrorCode::BrokerTransportFailure),
            ),
            (
                "transport still failed",
                Some(RDKafkaErrorCode::BrokerTransportFailure),
            ),
        ] {
            membership
                .record_observation(Err(diagnostic.into()), code)
                .unwrap();
            assert_eq!(membership.snapshot().error.as_deref(), Some(diagnostic));
        }
        membership.record(Ok(2)).unwrap();
        logs.rewind().unwrap();
        let mut output = String::new();
        logs.read_to_string(&mut output).unwrap();
        assert_eq!(output.matches("Worker membership query failed").count(), 2);
        assert_eq!(
            output.matches("Worker membership probe recovered").count(),
            1
        );
        membership.latest.lock().unwrap().as_mut().unwrap().at -= membership.stale_after;
        assert_eq!(membership.snapshot().members, None);
        assert!(membership.snapshot().checked_at_ms.is_some());
    }
}
