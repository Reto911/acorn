//! Backend process assembly for the P08 HTTP-to-Worker business workflow.

use std::process::ExitCode;

use acorn_backend::{
    config::{Cli, Config},
    database::Database,
    http, logging,
    scheduler::Scheduler,
    submissions::CreationService,
};
use anyhow::{Context, Result};
use clap::Parser;
use tokio::{net::TcpListener, sync::oneshot};

/// Run the configured backend and report startup or background failures explicitly.
#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Backend failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Start an empty Scheduler and HTTP service; restart task loading is implemented in P09.
async fn run(cli: Cli) -> Result<()> {
    let config = Config::load(&cli.config)?;
    logging::init(&config.logging)?;
    let database = Database::open(&config).await?;
    let result = serve(&config, database.clone()).await;
    database.close().await;
    result
}

/// Supervise both services and drain owned creation operations before stopping tracking.
async fn serve(config: &Config, database: Database) -> Result<()> {
    let listener = TcpListener::bind(config.http.listen)
        .await
        .context("Bind backend HTTP listener")?;
    let address = listener.local_addr()?;
    let (scheduler, mut scheduler_task) = Scheduler::start(config, database.clone())?;
    let creation = CreationService::new(config, database.clone(), scheduler.clone());
    let router = http::router(config, database, creation.clone());
    let (stop, stopped) = oneshot::channel();
    let mut http_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    tracing::info!(listen = %address, "Backend HTTP listening");
    let mut scheduler_finished = false;
    let mut http_finished = false;
    let result = tokio::select! {
        signal = stop_signal() => signal,
        result = &mut scheduler_task => {
            scheduler_finished = true;
            match result { Ok(Err(error)) => Err(error), other => Err(anyhow::anyhow!("Scheduler stopped unexpectedly: {other:?}")) }
        },
        result = &mut http_task => {
            http_finished = true;
            match result { Ok(Err(error)) => Err(error.into()), other => Err(anyhow::anyhow!("HTTP service stopped unexpectedly: {other:?}")) }
        },
    };
    tracing::info!("Backend stopping admission and dispatch");
    creation.stop();
    scheduler.pause();
    let _ = stop.send(());
    creation.wait_idle().await;
    scheduler.shutdown();
    let scheduler_result = if scheduler_finished {
        Ok(())
    } else {
        scheduler_task
            .await
            .context("Scheduler task failed")
            .and_then(|result| result)
    };
    let http_result = if http_finished {
        Ok(())
    } else {
        http_task
            .await
            .context("HTTP task failed")
            .and_then(|result| result.context("HTTP listener failed"))
    };
    result?;
    scheduler_result?;
    http_result?;
    tracing::info!("Backend stopped");
    Ok(())
}

/// Wait for either normal Unix termination signal without cancelling owned operations.
async fn stop_signal() -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("Wait for Ctrl-C"),
        _ = terminate.recv() => Ok(()),
    }
}
