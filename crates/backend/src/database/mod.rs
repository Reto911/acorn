//! One shared SQLx SQLite pool, explicit queries and short conditional writes.

mod problems;
mod reviews;
pub use reviews::ReviewInput;
mod submissions;

use std::{
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tokio::sync::watch;

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
    /// Review data violates the fixed shape or persistence budget.
    #[error("Invalid review: {0}")]
    InvalidReview(&'static str),
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
    /// First fatal storage error, retained for process supervision beyond HTTP disconnects.
    failure: watch::Sender<Option<String>>,
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
            if version != 3 {
                return Err(Error::Data(anyhow::anyhow!(
                    "Expected schema version 3, got {version}"
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
            failure: watch::channel(None).0,
        })
    }

    /// Wait for the first fatal operation, including one reported before subscribing.
    pub async fn failure(&self) -> anyhow::Error {
        let mut receiver = self.failure.subscribe();
        let error = receiver
            .wait_for(Option::is_some)
            .await
            .expect("Database retains the failure sender")
            .as_ref()
            .expect("Failure notification contains an error")
            .clone();
        anyhow::anyhow!(error)
    }

    /// Read an actual table through the shared pool and ordinary read-error boundary.
    pub async fn probe_read(&self) -> Result<()> {
        self.observe("Probe database read", false, async {
            sqlx::query_scalar::<_, i64>("SELECT id FROM users LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
            Ok(())
        })
        .await
    }

    /// Report storage failures at operation boundaries while preserving the original result.
    async fn observe<T>(
        &self,
        operation: &'static str,
        critical_write: bool,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let result = future.await;
        if let Err(Error::Sql(error)) = &result {
            let fatal = critical_write || severe_storage_error(error);
            tracing::error!(operation, fatal, error = %error, "Database operation failed");
            if fatal {
                self.failure.send_if_modified(|failure| {
                    if failure.is_some() {
                        return false;
                    }
                    *failure = Some(format!("{operation}: {error}"));
                    true
                });
            }
        }
        result
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

/// Recognize direct SQLx I/O errors and SQLite I/O, corruption, disk-full or invalid-file codes.
///
/// Extended SQLite codes retain the primary code in their least significant byte.
fn severe_storage_error(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Io(_))
        || error
            .as_database_error()
            .and_then(|error| error.code())
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| matches!(code & 0xff, 10 | 11 | 13 | 26))
}

/// Current UTC Unix milliseconds, with no fabricated fallback timestamp.
fn now_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Data(error.into()))?;
    i64::try_from(elapsed.as_millis()).map_err(|error| Error::Data(error.into()))
}

#[cfg(test)]
mod tests {
    //! Fatal storage classification uses a real SQLite invalid-file error.

    use super::*;

    /// A read encountering a corrupt database notifies supervision and preserves its error.
    #[tokio::test]
    async fn severe_read_failure_notifies_supervision() {
        let directory = tempfile::tempdir().unwrap();
        let corrupt = directory.path().join("corrupt.db");
        std::fs::write(&corrupt, [0xff; 4096]).unwrap();
        let database = Database {
            pool: SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(directory.path().join("healthy.db"))
                        .create_if_missing(true),
                )
                .await
                .unwrap(),
            config: Arc::new(
                Config::parse(include_str!("../../../../config/backend.example.toml")).unwrap(),
            ),
            failure: watch::channel(None).0,
        };
        let result = database
            .observe("Read corrupt database", false, async {
                sqlx::query("ATTACH DATABASE ? AS corrupt")
                    .bind(corrupt.to_str().unwrap())
                    .execute(&database.pool)
                    .await?;
                sqlx::query("SELECT * FROM corrupt.sqlite_schema")
                    .fetch_all(&database.pool)
                    .await?;
                Ok(())
            })
            .await;
        assert!(
            matches!(&result, Err(Error::Sql(error)) if severe_storage_error(error)),
            "{result:?}"
        );
        let error = tokio::time::timeout(Duration::from_secs(1), database.failure())
            .await
            .unwrap();
        assert!(error.to_string().contains("not a database"), "{error}");
        database.close().await;
    }
}
