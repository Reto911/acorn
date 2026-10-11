//! Worker listener startup and bounded shutdown through the common stop path.

use std::process::ExitCode;

use acorn_worker::{
    config::{Cli, Config},
    language, logging,
    rpc::WorkerService,
};
use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::oneshot;

/// Start the Worker and report startup, transport or shutdown failures.
#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match serve(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Worker failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Validate tools, listen with bounded messages, and detach admitted work from RPCs.
async fn serve(cli: &Cli) -> Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(&config.logging)?;
    config.check_tools()?;
    language::plan("c", &config)?;
    language::plan("python", &config)?;
    let listen = config.server.listen;
    let worker = WorkerService::new(config);
    let service = worker.clone().into_server();
    let (finished, shutdown_result) = oneshot::channel();
    tracing::info!(%listen, "Starting Worker RPC listener");
    tonic::transport::Server::builder()
        .add_service(service)
        .serve_with_shutdown(listen, async move {
            let signal_result = shutdown_signal().await;
            tracing::info!("Worker shutdown requested");
            let execution_result = worker.shutdown().await;
            let _ = finished.send(signal_result.and(execution_result));
        })
        .await
        .context("Worker RPC server failed")?;
    shutdown_result
        .await
        .context("Worker shutdown result missing")??;
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
