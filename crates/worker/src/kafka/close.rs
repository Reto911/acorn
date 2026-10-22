//! Release the last consumer owner off the runtime while the result producer remains available.

use super::ownership::Context as ConsumerContext;
use anyhow::{Context, Result};
use rdkafka::consumer::StreamConsumer;
use std::{sync::Arc, time::Duration};

/// Wait for in-flight commit owners, then let SDK RAII leave the group on a blocking thread.
///
/// The service bounds its wait for this operation; an already running SDK destructor is not canceled.
pub(super) async fn close(consumer: Arc<StreamConsumer<ConsumerContext>>) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut shared = consumer;
        let consumer = loop {
            match Arc::try_unwrap(shared) {
                Ok(consumer) => break consumer,
                Err(consumer) => {
                    shared = consumer;
                    // Revocation stops the existing commit wait at its next finite poll.
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        drop(consumer);
    })
    .await
    .context("KafkaWorkerConsumerDropFailed")?;
    Ok(())
}
