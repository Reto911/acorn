//! Queries for published current and explicitly selected immutable problem versions.

use acorn_protocol::worker::v1::RunLimits;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};

use crate::problems::{LanguageLimits, ProblemSummary, ProblemVersion};

use super::{Database, Error, Result};

impl Database {
    /// List only published current versions in problem-number order.
    pub async fn list_problems(&self, limit: u32, offset: u64) -> Result<Vec<ProblemSummary>> {
        let (limit, offset) = self.page(limit, offset)?;
        let rows = sqlx::query("SELECT p.id AS problem_id, v.version_no, v.title FROM problems p JOIN problem_versions v ON v.problem_id = p.id AND v.version_no = p.current_version_no ORDER BY p.id LIMIT ? OFFSET ?")
            .bind(limit).bind(offset).fetch_all(&self.pool).await?;
        rows.iter().map(summary).collect()
    }

    /// Read the current published version without exposing its private test data.
    pub async fn problem(&self, problem_id: i64) -> Result<Option<ProblemVersion>> {
        self.read_problem(problem_id, None).await
    }

    /// Read a particular published version belonging to the specified problem.
    pub async fn problem_version(
        &self,
        problem_id: i64,
        version_no: i64,
    ) -> Result<Option<ProblemVersion>> {
        self.read_problem(problem_id, Some(version_no)).await
    }

    /// Share current and explicit-version assembly while holding just one connection.
    async fn read_problem(
        &self,
        problem_id: i64,
        version: Option<i64>,
    ) -> Result<Option<ProblemVersion>> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query("SELECT v.problem_id, v.version_no, v.title, v.statement, v.comparison_mode, v.published_at_ms FROM problems p JOIN problem_versions v ON v.problem_id = p.id AND v.version_no = COALESCE(?, p.current_version_no) WHERE p.id = ?")
            .bind(version).bind(problem_id).fetch_optional(&mut *connection).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let summary = summary(&row)?;
        let languages = language_limits(&mut connection, problem_id, summary.version_no).await?;
        Ok(Some(ProblemVersion {
            summary,
            statement: row.try_get("statement")?,
            comparison_mode: row.try_get("comparison_mode")?,
            published_at_ms: row.try_get("published_at_ms")?,
            languages,
        }))
    }
}

/// Decode list identity without loading statements or test data.
fn summary(row: &SqliteRow) -> Result<ProblemSummary> {
    Ok(ProblemSummary {
        problem_id: row.try_get("problem_id")?,
        version_no: row.try_get("version_no")?,
        title: row.try_get("title")?,
    })
}

/// Read all allowed language budgets in deterministic identifier order.
async fn language_limits(
    connection: &mut SqliteConnection,
    problem_id: i64,
    version: i64,
) -> Result<Vec<LanguageLimits>> {
    let rows = sqlx::query("SELECT language_id, cpu_time_ms, wall_time_ms, memory_bytes FROM problem_version_languages WHERE problem_id = ? AND version_no = ? ORDER BY language_id")
        .bind(problem_id).bind(version).fetch_all(connection).await?;
    rows.iter()
        .map(|row| {
            Ok(LanguageLimits {
                language_id: row.try_get("language_id")?,
                limits: run_limits(row)?,
            })
        })
        .collect()
}

/// Convert signed SQLite budgets without truncating or wrapping negative data.
pub(super) fn run_limits(row: &SqliteRow) -> Result<RunLimits> {
    Ok(RunLimits {
        cpu_time_ms: unsigned(row.try_get("cpu_time_ms")?)?,
        wall_time_ms: unsigned(row.try_get("wall_time_ms")?)?,
        memory_bytes: unsigned(row.try_get("memory_bytes")?)?,
    })
}

/// Preserve integer conversion errors as invalid stored data.
fn unsigned(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|error| Error::Data(error.into()))
}
