//! FIFO terminal publication followed by confirmation of the original task position.

use super::ownership::{Context as ConsumerContext, Ownership};
use acorn_kafka::commit::OffsetCommit;
use acorn_protocol::{records, worker::v1::GetJobReply};
use anyhow::{Context, Result};
use rdkafka::{
    Offset, TopicPartitionList,
    consumer::StreamConsumer,
    error::KafkaError,
    producer::{FutureProducer, FutureRecord},
};
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::watch, time::Instant};

/// Task record identity and the original assignment under which execution began.
#[derive(Clone)]
pub(super) struct Origin {
    /// Configured task topic, retained without the task payload.
    pub topic: String,
    /// Original task partition.
    pub partition: i32,
    /// Original record offset, acknowledged only as offset+1.
    pub offset: i64,
    /// This execution's original assignment, permanently false after revocation.
    pub lease: Arc<AtomicBool>,
}

impl Origin {
    /// Whether this original assignment remains eligible for a task commit.
    pub fn owned(&self) -> bool {
        self.lease.load(Ordering::Acquire)
    }
}

/// One bounded terminal with optional runtime retention to release after handoff.
pub(super) struct Entry {
    /// Immutable bounded terminal; no source, cases or artifacts are retained.
    pub reply: GetJobReply,
    /// Task position and original assignment.
    pub origin: Origin,
    /// Runtime record identity, absent for rejected task content.
    pub updates: Option<watch::Receiver<GetJobReply>>,
}

/// Send one event using the existing payload-derived key and bounded client queue wait.
pub(super) async fn publish(
    producer: &FutureProducer,
    topic: &str,
    reply: &GetJobReply,
    limit: usize,
    queue_timeout: Duration,
) -> Result<()> {
    let encoded = records::encode_event(reply, limit)?;
    producer
        .send(
            FutureRecord::to(topic)
                .key(encoded.key())
                .payload(encoded.payload()),
            queue_timeout,
        )
        .await
        .map_err(|(error, _)| error)
        .context("KafkaWorkerEventPublishFailed")?;
    Ok(())
}

/// Enqueue a commit only while holding its original assignment guard, then wait off runtime.
pub(super) async fn commit(
    consumer: Weak<StreamConsumer<ConsumerContext>>,
    ownership: Ownership,
    origin: &Origin,
    timeout: Duration,
    shutdown: watch::Receiver<Option<Instant>>,
) -> Result<()> {
    let Some(consumer) = consumer.upgrade() else {
        return Ok(());
    };
    let mut offsets = TopicPartitionList::new();
    offsets.add_partition_offset(
        &origin.topic,
        origin.partition,
        Offset::Offset(origin.offset + 1),
    )?;
    let lease = origin.lease.clone();
    let deadline = Instant::now() + timeout;
    tokio::task::spawn_blocking(move || {
        let guard = ownership
            .current
            .lock()
            .expect("Kafka assignment lock poisoned");
        if !lease.load(Ordering::Acquire) {
            return Ok(());
        }
        let effective_deadline = shutdown
            .borrow()
            .map_or(deadline, |shutdown| shutdown.min(deadline));
        if Instant::now() >= effective_deadline {
            return Err(KafkaError::ConsumerCommit(
                rdkafka::types::RDKafkaErrorCode::OperationTimedOut,
            ))
            .context("KafkaWorkerTaskCommitTimedOut");
        }
        let commit = OffsetCommit::start(consumer.as_ref(), &offsets)?;
        drop(guard);
        loop {
            if !lease.load(Ordering::Acquire) {
                return Ok(());
            }
            let deadline = shutdown
                .borrow()
                .map_or(deadline, |shutdown| shutdown.min(deadline));
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(KafkaError::ConsumerCommit(
                    rdkafka::types::RDKafkaErrorCode::OperationTimedOut,
                ))
                .context("KafkaWorkerTaskCommitTimedOut");
            }
            // A short native poll keeps an already-started confirmation within a later
            // shutdown deadline; dropping an async timeout cannot cancel a blocking call.
            if commit.poll(remaining.min(Duration::from_millis(100)))? {
                return Ok(());
            }
        }
    })
    .await?
}

/// Preserve the handoff phase across transient failures; confirmed terminals are never resent.
pub(super) async fn terminal<P, C>(
    entry: Entry,
    mut publish: impl FnMut(GetJobReply) -> P,
    mut commit: impl FnMut(Origin) -> C,
    mut assignment: watch::Receiver<()>,
) -> Result<Entry>
where
    P: std::future::Future<Output = Result<()>>,
    C: std::future::Future<Output = Result<()>>,
{
    let mut published = false;
    loop {
        if published && !entry.origin.owned() {
            return Ok(entry);
        }
        let result = if published {
            commit(entry.origin.clone()).await
        } else {
            publish(entry.reply.clone()).await
        };
        if published && !entry.origin.owned() {
            return Ok(entry);
        }
        match result {
            Ok(()) if published => return Ok(entry),
            Ok(()) => published = true,
            Err(error) => {
                if !error
                    .downcast_ref::<KafkaError>()
                    .is_some_and(acorn_kafka::temporary)
                {
                    return Err(error);
                }
                tracing::warn!(job = %entry.reply.job_id, topic = %entry.origin.topic, partition = entry.origin.partition, offset = entry.origin.offset, published, %error, "Kafka Worker handoff will retry");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                    _ = assignment.changed() => {},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Regression coverage for confirmed publication and revoked original ownership.

    use super::*;
    use acorn_protocol::worker::v1::{JobPhase, JobState};
    use rdkafka::types::RDKafkaErrorCode;
    use std::sync::atomic::AtomicUsize;

    /// A bounded terminal whose origin can be revoked during a controlled handoff.
    fn entry(lease: Arc<AtomicBool>) -> Entry {
        Entry {
            reply: GetJobReply {
                job_id: "job".into(),
                state: JobState::Failed as i32,
                phase: JobPhase::Done as i32,
                ..Default::default()
            },
            origin: Origin {
                topic: "tasks".into(),
                partition: 0,
                offset: 7,
                lease,
            },
            updates: None,
        }
    }

    /// Commit failure retries only the position; losing ownership releases a published terminal.
    #[tokio::test]
    async fn confirmed_terminal_retries_only_commit_and_releases_revoked_origin() {
        let published = Arc::new(AtomicUsize::new(0));
        let committed = Arc::new(AtomicUsize::new(0));
        let (changed, changes) = watch::channel(());
        let sends = published.clone();
        let commits = committed.clone();
        terminal(
            entry(Arc::new(AtomicBool::new(true))),
            move |_| {
                sends.fetch_add(1, Ordering::Relaxed);
                async { Ok(()) }
            },
            move |origin| {
                assert_eq!(origin.offset + 1, 8);
                let first = commits.fetch_add(1, Ordering::Relaxed) == 0;
                changed.send_replace(());
                async move {
                    if first {
                        Err(KafkaError::ConsumerCommit(RDKafkaErrorCode::OperationTimedOut).into())
                    } else {
                        Ok(())
                    }
                }
            },
            changes,
        )
        .await
        .unwrap();
        assert_eq!(published.load(Ordering::Relaxed), 1);
        assert_eq!(committed.load(Ordering::Relaxed), 2);

        let lease = Arc::new(AtomicBool::new(true));
        let revoked = lease.clone();
        terminal(
            entry(lease),
            move |_| {
                revoked.store(false, Ordering::Release);
                async { Ok(()) }
            },
            |_| async { panic!("revoked execution must not enqueue a task commit") },
            watch::channel(()).1,
        )
        .await
        .unwrap();
    }
}
