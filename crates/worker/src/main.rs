//! Worker Kafka execution, read-only status RPCs and coordinated process shutdown.

use std::process::ExitCode;

use acorn_worker::{
    config::{Cli, Config},
    kafka::{self, KafkaWorker},
    language, logging,
    rpc::WorkerService,
};
use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::{oneshot, watch};

/// Start the Worker and report startup, transport or shutdown failures.
#[tokio::main]
async fn main() -> ExitCode {
    match serve(&Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Worker failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Supervise Kafka and status transport, keeping status available through execution cleanup.
async fn serve(cli: &Cli) -> Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(&config.logging)?;
    config.check_tools()?;
    language::plan("c", &config)?;
    language::plan("python", &config)?;
    let listen = config.server.listen;
    let producer = kafka::producer_config(&config.kafka).create()?;
    let worker = WorkerService::new(config);
    let kafka = KafkaWorker::new(worker.clone(), producer)?;
    let (stop, stopping) = watch::channel(false);
    let mut kafka_task = tokio::spawn(kafka.run(stopping));
    let (rpc_stop, stopped) = oneshot::channel();
    let mut rpc_task = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(worker.into_server())
            .serve_with_shutdown(listen, async {
                let _ = stopped.await;
            })
            .await
            .context("Worker RPC server failed")
    });
    tracing::info!(%listen, "Starting Worker status RPC listener and Kafka service");
    let mut kafka_finished = false;
    let mut rpc_finished = false;
    let result = tokio::select! {
        signal = shutdown_signal() => signal,
        result = &mut kafka_task => {
            kafka_finished = true;
            result.context("Worker Kafka task failed").and_then(|result| result)
                .and_then(|()| Err(anyhow::anyhow!("Worker Kafka service stopped unexpectedly")))
        },
        result = &mut rpc_task => {
            rpc_finished = true;
            result.context("Worker RPC task failed").and_then(|result| result)
                .and_then(|()| Err(anyhow::anyhow!("Worker RPC service stopped unexpectedly")))
        },
    };
    tracing::info!("Worker shutdown requested");
    stop.send_replace(true);
    let execution = if kafka_finished {
        Ok(())
    } else {
        kafka_task
            .await
            .context("Worker Kafka task failed")
            .and_then(|result| result)
    };
    let _ = rpc_stop.send(());
    let transport = if rpc_finished {
        Ok(())
    } else {
        rpc_task
            .await
            .context("Worker RPC task failed")
            .and_then(|result| result)
    };
    for error in [&execution, &transport]
        .into_iter()
        .filter_map(|result| result.as_ref().err())
    {
        tracing::error!(%error, "Worker service failed during shutdown");
    }
    result.and(execution).and(transport)?;
    tracing::info!("Worker stopped");
    Ok(())
}

/// Accept Ctrl-C and the deployment service manager's SIGTERM.
async fn shutdown_signal() -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("Install Worker SIGTERM handler")?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("Wait for Worker Ctrl-C"),
        _ = terminate.recv() => Ok(()),
    }
}
