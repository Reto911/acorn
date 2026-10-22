//! Explicit asynchronous consumer close while the result producer remains available.

use super::ownership::Context as ConsumerContext;
use anyhow::{Context, Result, ensure};
use rdkafka::{
    bindings,
    consumer::{Consumer, StreamConsumer},
    error::KafkaError,
};
use std::{sync::Arc, time::Duration};

/// Initiate native close, drive its callbacks, and observe closed before releasing the SDK handle.
pub(super) async fn close(consumer: Arc<StreamConsumer<ConsumerContext>>) -> Result<()> {
    let client = consumer.client().native_ptr();
    // SAFETY: the Arc owns the live client. get_consumer returns one owned queue reference.
    let queue = unsafe { bindings::rd_kafka_queue_get_consumer(client) };
    ensure!(!queue.is_null(), "KafkaWorkerConsumerQueueUnavailable");
    // SAFETY: client and queue remain live; close_queue retains what it needs for asynchronous close.
    let error = unsafe { bindings::rd_kafka_consumer_close_queue(client, queue) };
    // SAFETY: release exactly the reference returned by get_consumer, on both success and error.
    unsafe {
        bindings::rd_kafka_queue_destroy(queue);
    }
    if !error.is_null() {
        // SAFETY: close_queue transfers this error object to the caller.
        let code = unsafe { bindings::rd_kafka_error_code(error) };
        // SAFETY: the code was copied and the owned error is no longer accessed.
        unsafe {
            bindings::rd_kafka_error_destroy(error);
        }
        return Err(KafkaError::ConsumerQueueClose(code.into()))
            .context("KafkaWorkerConsumerCloseFailed");
    }
    loop {
        // SAFETY: consumer keeps the native client live throughout close and callback polling.
        if unsafe { bindings::rd_kafka_consumer_closed(consumer.client().native_ptr()) } == 1 {
            break;
        }
        tokio::select! {
            _ = consumer.recv() => {},
            _ = tokio::time::sleep(Duration::from_millis(10)) => {},
        }
    }
    // Drop sees an already closed consumer, even if a finite commit wait still has an Arc.
    tokio::task::spawn_blocking(move || drop(consumer))
        .await
        .context("KafkaWorkerConsumerDropFailed")?;
    Ok(())
}
