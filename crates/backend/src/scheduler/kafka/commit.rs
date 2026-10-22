//! Bounded confirmation through librdkafka's explicit offset-commit reply queue.

use std::{ptr::NonNull, time::Duration};

use anyhow::{Context, Result, ensure};
use rdkafka::{
    TopicPartitionList, bindings,
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
};

/// Owned result queue; outstanding native operations retain their own queue references.
struct ReplyQueue {
    /// Non-null queue created for this one commit and destroyed once on every return path.
    pointer: NonNull<bindings::rd_kafka_queue_t>,
}

impl Drop for ReplyQueue {
    /// Release the owned queue without waiting indefinitely for a coordinator or callback.
    fn drop(&mut self) {
        // SAFETY: this wrapper exclusively owns the reference returned by queue_new.
        unsafe {
            bindings::rd_kafka_queue_destroy(self.pointer.as_ptr());
        }
    }
}

/// Owned native event; the partition list is borrowed only while this event remains alive.
struct CommitEvent {
    /// Non-null event returned by polling this commit's dedicated reply queue.
    pointer: NonNull<bindings::rd_kafka_event_t>,
}

impl Drop for CommitEvent {
    /// Release the event and the partition list it owns after result inspection.
    fn drop(&mut self) {
        // SAFETY: queue_poll transfers this event reference to this wrapper exactly once.
        unsafe {
            bindings::rd_kafka_event_destroy(self.pointer.as_ptr());
        }
    }
}

/// Submit exactly these offsets and inspect the actual reply within the supplied native budget.
///
/// The caller keeps the consumer alive and invokes this function on a blocking thread.
/// An explicit reply queue avoids depending on manual Async commits emitting a context callback.
/// Its poll accepts a native timeout; the synchronous commit API has no timeout argument.
pub(super) fn confirm(
    consumer: &StreamConsumer,
    offsets: &TopicPartitionList,
    timeout: Duration,
) -> Result<()> {
    let client = consumer.client().native_ptr();
    // SAFETY: the borrowed consumer owns a live client throughout this function.
    let pointer = unsafe { bindings::rd_kafka_queue_new(client) };
    let queue = ReplyQueue {
        pointer: NonNull::new(pointer).context("KafkaEventCommitQueueUnavailable")?,
    };
    // SAFETY: client, offsets and queue are live; librdkafka copies offsets into the operation.
    // No callback or opaque pointer is supplied: the reply is delivered to our owned queue.
    let submitted = unsafe {
        bindings::rd_kafka_commit_queue(
            client,
            offsets.ptr(),
            queue.pointer.as_ptr(),
            None,
            std::ptr::null_mut(),
        )
    };
    if submitted as i32 != 0 {
        return Err(KafkaError::ConsumerCommit(submitted.into())).context("KafkaEventCommitFailed");
    }
    // SAFETY: queue stays alive while polling; validated configuration fits signed milliseconds.
    let pointer = unsafe {
        bindings::rd_kafka_queue_poll(queue.pointer.as_ptr(), timeout.as_millis() as i32)
    };
    let event = CommitEvent {
        pointer: NonNull::new(pointer).context("KafkaEventCommitTimedOut")?,
    };
    // SAFETY: the owned event is live and represents the reply to this queue's only operation.
    let error = unsafe { bindings::rd_kafka_event_error(event.pointer.as_ptr()) };
    if error as i32 != 0 {
        return Err(KafkaError::ConsumerCommit(error.into())).context("KafkaEventCommitFailed");
    }
    // SAFETY: the returned list is owned by event and remains valid until event is dropped.
    let pointer = unsafe { bindings::rd_kafka_event_topic_partition_list(event.pointer.as_ptr()) };
    ensure!(!pointer.is_null(), "KafkaEventCommitMissingOffsets");
    // SAFETY: event owns this list and its cnt initialized partition entries.
    let partitions = unsafe { &*pointer };
    for index in 0..partitions.cnt {
        // SAFETY: index stays within the event-owned list, which outlives this inspection.
        let error = unsafe { (*partitions.elems.add(index as usize)).err };
        if error as i32 != 0 {
            return Err(KafkaError::ConsumerCommit(error.into())).context("KafkaEventCommitFailed");
        }
    }
    Ok(())
}
