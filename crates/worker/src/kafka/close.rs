//! Release the last consumer owner off the runtime while the result producer remains available.

use super::ownership::Context as ConsumerContext;
use anyhow::{Context, Result};
use rdkafka::consumer::StreamConsumer;
use std::sync::Arc;

/// Let SDK RAII release the final consumer owner and leave the group on a blocking thread.
///
/// The service bounds its wait for this operation; an already running SDK destructor is not canceled.
pub(super) async fn close(consumer: Arc<StreamConsumer<ConsumerContext>>) -> Result<()> {
    tokio::task::spawn_blocking(move || drop(consumer))
        .await
        .context("KafkaWorkerConsumerDropFailed")?;
    Ok(())
}
