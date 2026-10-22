//! Bounded confirmation through librdkafka's explicit offset-commit reply queue.

use std::{marker::PhantomData, ptr::NonNull, time::Duration};

use anyhow::{Context, Result, ensure};
use rdkafka::{
    TopicPartitionList, bindings,
    consumer::{Consumer, ConsumerContext},
    error::KafkaError,
};

/// Owned result queue; outstanding native operations retain their own queue references.
pub struct OffsetCommit<'a> {
    /// Keep the consumer borrowed until the reply queue has been released.
    consumer: PhantomData<&'a ()>,
    /// Non-null queue created for this one commit and destroyed once on every return path.
    pointer: NonNull<bindings::rd_kafka_queue_t>,
}

impl Drop for OffsetCommit<'_> {
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

impl<'a> OffsetCommit<'a> {
    /// Enqueue exactly these offsets while the caller still holds its assignment guard.
    ///
    /// This call only enqueues the native operation; wait inspects its actual broker result.
    pub fn start<C: ConsumerContext>(
        consumer: &'a impl Consumer<C>,
        offsets: &TopicPartitionList,
    ) -> Result<Self> {
        let client = consumer.client().native_ptr();
        // SAFETY: the borrowed consumer owns a live client throughout this function.
        let pointer = unsafe { bindings::rd_kafka_queue_new(client) };
        let queue = Self {
            consumer: PhantomData,
            pointer: NonNull::new(pointer).context("KafkaCommitQueueUnavailable")?,
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
            return Err(KafkaError::ConsumerCommit(submitted.into())).context("KafkaCommitFailed");
        }
        Ok(queue)
    }

    /// Inspect the actual commit result within the native timeout, off the async executor.
    pub fn wait(self, timeout: Duration) -> Result<()> {
        if !self.poll(timeout)? {
            return Err(KafkaError::ConsumerCommit(
                rdkafka::types::RDKafkaErrorCode::OperationTimedOut,
            ))
            .context("KafkaCommitTimedOut");
        }
        Ok(())
    }

    /// Poll this operation's result queue once, allowing a caller to shorten shutdown waits.
    pub fn poll(&self, timeout: Duration) -> Result<bool> {
        // SAFETY: self owns a live queue; callers supply a finite signed-millisecond budget.
        let pointer = unsafe {
            bindings::rd_kafka_queue_poll(self.pointer.as_ptr(), timeout.as_millis() as i32)
        };
        let Some(pointer) = NonNull::new(pointer) else {
            return Ok(false);
        };
        let event = CommitEvent { pointer };
        // SAFETY: the owned event is live and represents the reply to this queue's only operation.
        let error = unsafe { bindings::rd_kafka_event_error(event.pointer.as_ptr()) };
        if error as i32 != 0 {
            return Err(KafkaError::ConsumerCommit(error.into())).context("KafkaCommitFailed");
        }
        // SAFETY: the returned list is owned by event and remains valid until event is dropped.
        let pointer =
            unsafe { bindings::rd_kafka_event_topic_partition_list(event.pointer.as_ptr()) };
        ensure!(!pointer.is_null(), "KafkaCommitMissingOffsets");
        // SAFETY: event owns this list and its cnt initialized partition entries.
        let partitions = unsafe { &*pointer };
        for index in 0..partitions.cnt {
            // SAFETY: index stays within the event-owned list, which outlives this inspection.
            let error = unsafe { (*partitions.elems.add(index as usize)).err };
            if error as i32 != 0 {
                return Err(KafkaError::ConsumerCommit(error.into())).context("KafkaCommitFailed");
            }
        }
        Ok(true)
    }
}
