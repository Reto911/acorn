//! Backend startup loading, service supervision and normal or fatal shutdown.

use std::process::ExitCode;

use acorn_backend::{
    config::{Cli, Config},
    database::Database,
    health::Health,
    http,
    llm_review::ReviewService,
    logging,
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

/// Supervise business services and health collection, draining owned work before shutdown.
async fn serve(config: &Config, database: Database) -> Result<()> {
    let interrupted = database.interrupt_in_flight().await?;
    let reviews_interrupted = database.interrupt_pending_reviews().await?;
    let startup = database.queued_tasks().await?;
    tracing::info!(
        interrupted,
        reviews_interrupted,
        queued = startup.len(),
        "Backend startup tasks loaded"
    );
    let listener = TcpListener::bind(config.http.listen)
        .await
        .context("Bind backend HTTP listener")?;
    let address = listener.local_addr()?;
    let (review, mut review_task) = ReviewService::start(&config.llm_review, database.clone())?;
    let (scheduler, mut scheduler_task) =
        Scheduler::start(config, database.clone(), startup, review.clone())?;
    let creation = CreationService::new(config, database.clone(), scheduler.clone());
    let (health, mut health_task) = Health::start(&config.health, database.clone(), &scheduler)?;
    let router = http::router(
        config,
        database.clone(),
        creation.clone(),
        scheduler.clone(),
        health.clone(),
        review.clone(),
    );
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
    let mut review_finished = false;
    let mut http_finished = false;
    let mut health_finished = false;
    let mut stopping = false;
    let mut creation_drained = false;
    let result = loop {
        tokio::select! {
            biased;
            error = database.failure() => break Err(error),
            result = &mut scheduler_task, if !scheduler_finished => {
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
                if review_finished {
                    let _ = stop.take().expect("HTTP stop signal is available").send(());
                }
            },
            result = &mut review_task, if !review_finished => {
                review_finished = true;
                if !stopping {
                    break match result {
                        Ok(Err(error)) => Err(error),
                        other => Err(anyhow::anyhow!("Review service stopped unexpectedly: {other:?}")),
                    };
                }
                if let Err(error) = result.context("Review service task failed").and_then(|result| result) {
                    break Err(error);
                }
                tracing::info!("Review service stopped");
                if scheduler_finished {
                    let _ = stop.take().expect("HTTP stop signal is available").send(());
                }
            },
            result = &mut http_task => {
                http_finished = true;
                if stopping && scheduler_finished && review_finished {
                    break result.context("HTTP task failed")
                        .and_then(|result| result.context("HTTP listener failed"));
                }
                break match result {
                    Ok(Err(error)) => Err(error.into()),
                    other => Err(anyhow::anyhow!("HTTP service stopped unexpectedly: {other:?}")),
                };
            },
            result = &mut health_task, if !health_finished => {
                health_finished = true;
                observe_health_completion(result, false);
            },
            signal = stop_signal(), if !stopping => {
                if let Err(error) = signal {
                    break Err(error);
                }
                tracing::info!("Backend stopping admission and dispatch");
                creation.stop();
                scheduler.pause();
                review.stop();
                stopping = true;
            },
            _ = creation.wait_idle(), if stopping && !creation_drained => {
                creation_drained = true;
                scheduler.shutdown();
            },
        }
    };
    if let Err(error) = &result {
        tracing::error!(error = %error, "Backend fault; stopping admission and dispatch");
        creation.stop();
        scheduler.pause();
        review.stop();
        health.stop();
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
        if !review_finished {
            match review_task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::error!(error = %error, "Review service failed during fault shutdown")
                }
                Err(error) => {
                    tracing::error!(error = %error, "Review service task failed during fault shutdown")
                }
            }
        }
    }
    health.stop();
    if !health_finished {
        observe_health_completion(health_task.await, true);
    }
    if result.is_ok() {
        tracing::info!("Backend stopped");
    }
    result
}

/// Log collector completion without making an observation-task failure a business fault.
fn observe_health_completion(
    result: std::result::Result<Result<()>, tokio::task::JoinError>,
    stopping: bool,
) {
    match result {
        Ok(Ok(())) if stopping => tracing::info!("Health collector stopped"),
        Ok(Ok(())) => tracing::error!("Health collector stopped unexpectedly"),
        Ok(Err(error)) => tracing::error!(error = %error, "Health collector failed"),
        Err(error) => tracing::error!(error = %error, "Health collector task failed"),
    }
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
