//! Backend startup loading, service supervision and normal or fatal shutdown.

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
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

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

/// Validate storage and finish startup processing before opening HTTP admission.
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
    let interrupted = database.interrupt_in_flight().await?;
    let startup = database.queued_tasks().await?;
    tracing::info!(
        interrupted,
        queued = startup.len(),
        "Backend startup tasks loaded"
    );
    let listener = TcpListener::bind(config.http.listen)
        .await
        .context("Bind backend HTTP listener")?;
    let address = listener.local_addr()?;
    let (scheduler, mut scheduler_task) = Scheduler::start(config, database.clone(), startup)?;
    let creation = CreationService::new(config, database.clone(), scheduler.clone());
    let router = http::router(config, database.clone(), creation.clone());
    let (stop, stopped) = oneshot::channel();
    let mut http_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    tracing::info!(listen = %address, "Backend HTTP listening");
    let mut stop = Some(stop);
    let mut scheduler_finished = false;
    let mut http_finished = false;
    let mut stopping = false;
    let mut creation_drained = false;
    let result = loop {
        tokio::select! {
            biased;
            error = database.failure() => break Err(error),
            result = &mut scheduler_task => {
                scheduler_finished = true;
                if !stopping || !creation_drained {
                    break match result {
                        Ok(Err(error)) => Err(error),
                        other => Err(anyhow::anyhow!("Scheduler stopped unexpectedly: {other:?}")),
                    };
                }
                let completed = result.context("Scheduler task failed").and_then(|result| result);
                if let Err(error) = completed {
                    break Err(error);
                }
                let _ = stop.take().expect("HTTP stop signal is available").send(());
                break tokio::select! {
                    biased;
                    error = database.failure() => Err(error),
                    result = &mut http_task => {
                        http_finished = true;
                        result.context("HTTP task failed")
                            .and_then(|result| result.context("HTTP listener failed"))
                    },
                };
            },
            result = &mut http_task => {
                http_finished = true;
                break match result {
                    Ok(Err(error)) => Err(error.into()),
                    other => Err(anyhow::anyhow!("HTTP service stopped unexpectedly: {other:?}")),
                };
            },
            signal = stop_signal(), if !stopping => {
                if let Err(error) = signal {
                    break Err(error);
                }
                tracing::info!("Backend stopping admission and dispatch");
                creation.stop();
                scheduler.pause();
                stopping = true;
            },
            _ = creation.wait_idle(), if stopping && !creation_drained => {
                creation_drained = true;
                scheduler.shutdown();
            },
        }
    };
    if let Err(error) = result {
        tracing::error!(error = %error, "Backend fault; stopping admission and dispatch");
        creation.stop();
        scheduler.pause();
        if let Some(stop) = stop.take() {
            let _ = stop.send(());
        }
        creation.wait_idle().await;
        if !scheduler_finished {
            abort_task(&mut scheduler_task, "Scheduler").await;
        }
        if !http_finished {
            abort_task(&mut http_task, "HTTP").await;
        }
        return Err(error);
    }
    tracing::info!("Backend stopped");
    Ok(())
}

/// End local service tracking after a fatal fault, preserving secondary failure logs.
async fn abort_task<E: std::fmt::Display>(
    task: &mut JoinHandle<std::result::Result<(), E>>,
    service: &str,
) {
    task.abort();
    match task.await {
        Ok(Err(error)) => {
            tracing::error!(service, error = %error, "Service failed during fault shutdown")
        }
        Err(error) if !error.is_cancelled() => {
            tracing::error!(service, error = %error, "Service task failed during fault shutdown");
        }
        _ => {}
    }
}

/// Wait for either normal Unix termination signal without cancelling owned operations.
async fn stop_signal() -> Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("Wait for Ctrl-C"),
        _ = terminate.recv() => Ok(()),
    }
}
