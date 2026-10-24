//! One shared SQLx MySQL pool, explicit queries and short conditional writes.

mod problems;
mod reviews;
pub(crate) use reviews::ReviewAdmission;
pub use reviews::ReviewInput;
mod submissions;

use std::{
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::{
    MySqlConnection, MySqlPool,
    mysql::{MySqlDatabaseError, MySqlPoolOptions},
};
use tokio::sync::watch;

use crate::config::{Config, DatabaseConfig};

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
    /// SQLx preserves the actual MySQL failure and its context.
    #[error("Database operation failed: {0}")]
    Sql(#[from] sqlx::Error),
    /// Startup configuration references or pre-existing task data are inconsistent.
    #[error("Invalid database data: {0:#}")]
    Data(#[source] anyhow::Error),
}

/// Typed database operation result, preserving business errors and storage failures.
pub type Result<T> = std::result::Result<T, Error>;

/// Cloneable backend database handle; clones share one bounded connection pool.
#[derive(Clone)]
pub struct Database {
    /// Shared MySQL pool; connections are never held across RPC or queue handoff.
    pool: MySqlPool,
    /// Fixed owner and deployment resource ceilings.
    config: Arc<Config>,
    /// First fatal storage error, retained for process supervision beyond HTTP disconnects.
    failure: watch::Sender<Option<String>>,
}

impl Database {
    /// Open an existing database, checking schema version and configured test user.
    pub async fn open(config: &Config) -> Result<Self> {
        let pool = connect_pool(&config.database).await?;
        let checked = async {
            let version: Option<i64> =
                sqlx::query_scalar("SELECT version FROM schema_version WHERE id = 1")
                    .fetch_optional(&pool)
                    .await?;
            let version = version
                .ok_or_else(|| Error::Data(anyhow::anyhow!("Schema version row is missing")))?;
            if version != 4 {
                return Err(Error::Data(anyhow::anyhow!(
                    "Expected schema version 4, got {version}"
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
        tracing::info!(
            user_id = config.test_user.user_id,
            "Backend database validated"
        );
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
        let result = sqlx::query_scalar::<_, i64>("SELECT id FROM users LIMIT 1")
            .fetch_optional(&self.pool)
            .await;
        if let Err(error) = &result {
            // Periodic ordinary failures are logged on health-cache transitions.
            self.report_sql_error("Probe database read", false, false, error);
        }
        result.map(|_| ()).map_err(Error::Sql)
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
            self.report_sql_error(operation, critical_write, true, error);
        }
        result
    }

    /// Keep fatal supervision common while allowing periodic nonfatal errors to use cached logging.
    fn report_sql_error(
        &self,
        operation: &'static str,
        critical_write: bool,
        log_nonfatal: bool,
        error: &sqlx::Error,
    ) {
        let fatal = critical_write || severe_storage_error(error);
        if fatal || log_nonfatal {
            tracing::error!(operation, fatal, error = %error, "Database operation failed");
        }
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

    /// Await release and closure of all shared connections at process shutdown.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Enforce configured list page bounds and the signed offset range.
    fn page(&self, limit: u32, offset: u64) -> Result<(i64, i64)> {
        if limit == 0 || limit > self.config.http.max_page_size {
            return Err(Error::InvalidRequest(
                "Page size is outside configured bounds",
            ));
        }
        Ok((
            i64::from(limit),
            i64::try_from(offset).map_err(|_| {
                Error::InvalidRequest("Page offset exceeds the signed integer range")
            })?,
        ))
    }
}

/// Open the bounded application pool and initialize each new session once.
pub async fn connect_pool(config: &DatabaseConfig) -> Result<MySqlPool> {
    let options = config
        .connect_options()
        .map_err(Error::Data)?
        .database(&config.name);
    let lock_wait = config.lock_wait_timeout_secs;
    Ok(MySqlPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_millis(config.acquire_timeout_ms))
        .after_connect(move |connection, _| {
            Box::pin(async move { initialize_session(connection, lock_wait).await })
        })
        .connect_with(options)
        .await?)
}

/// Apply application session settings to a new pool or independent control connection.
pub async fn initialize_session(
    connection: &mut MySqlConnection,
    lock_wait: u32,
) -> sqlx::Result<()> {
    sqlx::query("SET SESSION TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *connection)
        .await?;
    sqlx::query("SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION', innodb_lock_wait_timeout = ?, lock_wait_timeout = ?")
        .bind(lock_wait).bind(lock_wait).execute(connection).await?;
    Ok(())
}

/// Extract the MySQL server number without interpreting SQLSTATE or transport errors.
fn mysql_error_number(error: &sqlx::Error) -> Option<u16> {
    error
        .as_database_error()
        .and_then(|error| error.try_downcast_ref::<MySqlDatabaseError>())
        .map(MySqlDatabaseError::number)
}

/// Recognize explicit server storage faults without treating network I/O as disk damage.
fn severe_storage_error(error: &sqlx::Error) -> bool {
    matches!(
        mysql_error_number(error),
        Some(1114 | 1712 | 1810 | 1811 | 1812 | 1817)
    )
}

/// Current UTC Unix milliseconds, with no fabricated fallback timestamp.
pub(crate) fn now_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Data(error.into()))?;
    i64::try_from(elapsed.as_millis()).map_err(|error| Error::Data(error.into()))
}

#[cfg(test)]
mod tests {
    //! Network transport failures retain the ordinary-read error boundary.

    use super::*;

    /// A lost network connection is not an explicit MySQL server storage fault.
    #[test]
    fn network_failure_is_not_storage_damage() {
        let error = sqlx::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        assert!(!severe_storage_error(&error));
    }
}
