//! One shared SQLx SQLite pool, explicit queries and short conditional writes.

mod problems;
mod submissions;

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use crate::config::Config;

/// Database and business errors remain distinguishable at the HTTP boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The requested problem has no current published version.
    #[error("Problem has no published version")]
    ProblemNotFound,
    /// The selected version does not allow the requested language.
    #[error("Language is not allowed for this problem version")]
    LanguageNotAllowed,
    /// The language is not one of the two built-in languages.
    #[error("Unsupported language")]
    UnsupportedLanguage,
    /// Basic creation or pagination input is invalid.
    #[error("Invalid request: {0}")]
    InvalidRequest(&'static str),
    /// UTF-8-encoded or internally supplied source exceeds its byte budget.
    #[error("Source exceeds configured byte limit")]
    SourceTooLarge,
    /// A creation key already belongs to different content.
    #[error("Idempotency key conflicts with existing submission content")]
    IdempotencyConflict,
    /// A Worker terminal violates the agreed shape or raw-byte budgets.
    #[error("Invalid Worker result: {0}")]
    InvalidResult(&'static str),
    /// SQLx preserves the actual SQLite failure and its context.
    #[error("Database operation failed: {0}")]
    Sql(#[from] sqlx::Error),
    /// Startup configuration references or pre-existing task data are inconsistent.
    #[error("Invalid database data: {0:#}")]
    Data(#[source] anyhow::Error),
}

/// Typed database operation result, preserving business errors and storage failures.
pub type Result<T> = std::result::Result<T, Error>;

/// Cloneable backend database handle; clones share the same single-connection pool.
#[derive(Clone)]
pub struct Database {
    /// The only backend SQLite pool; never held across RPC or Scheduler handoff.
    pool: SqlitePool,
    /// Fixed owner and deployment resource ceilings.
    config: Arc<Config>,
}

impl Database {
    /// Open an existing database, checking schema version and configured test user.
    pub async fn open(config: &Config) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(&config.database.path)
            .create_if_missing(false)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(config.database.busy_timeout_ms));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        let checked = async {
            let version: i64 = sqlx::query_scalar("PRAGMA user_version")
                .fetch_one(&pool)
                .await?;
            if version != 2 {
                return Err(Error::Data(anyhow::anyhow!(
                    "Expected schema version 2, got {version}"
                )));
            }
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
                    .bind(config.test_user.user_id)
                    .fetch_one(&pool)
                    .await?;
            if !exists {
                return Err(Error::Data(anyhow::anyhow!(
                    "Configured test user {} does not exist",
                    config.test_user.user_id
                )));
            }
            Ok(())
        }
        .await;
        if let Err(error) = checked {
            pool.close().await;
            return Err(error);
        }
        tracing::info!(path = %config.database.path.display(), user_id = config.test_user.user_id, "Backend database validated");
        Ok(Self {
            pool,
            config: Arc::new(config.clone()),
        })
    }

    /// Await release and closure of the shared connection at process shutdown.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Enforce configured list page bounds and SQLite's signed offset range.
    fn page(&self, limit: u32, offset: u64) -> Result<(i64, i64)> {
        if limit == 0 || limit > self.config.http.max_page_size {
            return Err(Error::InvalidRequest(
                "Page size is outside configured bounds",
            ));
        }
        Ok((
            i64::from(limit),
            i64::try_from(offset)
                .map_err(|_| Error::InvalidRequest("Page offset exceeds SQLite's integer range"))?,
        ))
    }
}

/// Current UTC Unix milliseconds, with no fabricated fallback timestamp.
fn now_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Data(error.into()))?;
    i64::try_from(elapsed.as_millis()).map_err(|error| Error::Data(error.into()))
}
