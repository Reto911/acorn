//! Atomic creation, fixed-version task assembly and conditional business-state writes.

use acorn_protocol::worker::v1::{JobFailure, SubmitJobRequest, TestCase};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use uuid::Uuid;

use crate::{
    config::Config,
    submissions::{
        CreateSubmission, CreatedSubmission, JudgingTask, Submission, SubmissionFilter,
        SubmissionInfo, SubmissionState, TerminalOutcome, Verdict,
    },
};

use super::{Database, Error, Result, now_ms, problems::run_limits};

/// Common list columns; queries using this projection never read source or diagnostics.
const INFO_COLUMNS: &str =
    "id, problem_id, version_no, language_id, state, verdict, created_at_ms, terminal_at_ms";

impl Database {
    /// Look up a creation key scoped to the deployment-configured test user.
    pub async fn submission_by_key(&self, key: Uuid) -> Result<Option<Submission>> {
        let mut connection = self.pool.acquire().await?;
        by_key(
            &mut connection,
            self.config.test_user.user_id,
            &key.to_string(),
        )
        .await
    }

    /// Read one stored submission and its parsed terminal details.
    pub async fn submission(&self, id: &str) -> Result<Option<Submission>> {
        let row = sqlx::query(&format!("SELECT {INFO_COLUMNS}, user_id, source, create_request_key, terminal_details_json FROM submissions WHERE id = ?"))
            .bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(stored_submission).transpose()
    }

    /// Read configured-user history without source or terminal diagnostic payloads.
    pub async fn list_submissions(
        &self,
        filter: &SubmissionFilter,
        limit: u32,
        offset: u64,
    ) -> Result<Vec<SubmissionInfo>> {
        let (limit, offset) = self.page(limit, offset)?;
        let state = filter.state.map(SubmissionState::as_str);
        let verdict = filter.verdict.map(Verdict::as_str);
        let rows = sqlx::query(&format!("SELECT {INFO_COLUMNS} FROM submissions WHERE user_id = ? AND (? IS NULL OR problem_id = ?) AND (? IS NULL OR state = ?) AND (? IS NULL OR verdict = ?) ORDER BY created_at_ms DESC, id DESC LIMIT ? OFFSET ?"))
            .bind(self.config.test_user.user_id).bind(filter.problem_id).bind(filter.problem_id)
            .bind(state).bind(state).bind(verdict).bind(verdict).bind(limit).bind(offset)
            .fetch_all(&self.pool).await?;
        rows.iter().map(info).collect()
    }

    /// Fix the latest version and create at most one record per owner/key transaction.
    ///
    /// The caller supplies the later Scheduler reservation; this method never waits
    /// for queue capacity or holds its transaction across task handoff.
    pub async fn create_submission(&self, request: CreateSubmission) -> Result<CreatedSubmission> {
        validate_source(&request.source, &self.config)?;
        let key = request.request_key.to_string();
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(existing) =
            by_key(&mut transaction, self.config.test_user.user_id, &key).await?
        {
            if existing.info.problem_id != request.problem_id
                || existing.info.language_id != request.language_id
                || existing.source != request.source
            {
                return Err(Error::IdempotencyConflict);
            }
            let task = if existing.info.state == SubmissionState::Queued {
                Some(
                    load_task(
                        &mut transaction,
                        &existing.info,
                        request.source,
                        &self.config,
                    )
                    .await?,
                )
            } else {
                None
            };
            transaction.commit().await?;
            return Ok(CreatedSubmission {
                info: existing.info,
                created: false,
                task,
            });
        }
        if !matches!(request.language_id.as_str(), "c" | "python") {
            return Err(Error::UnsupportedLanguage);
        }
        let version: Option<i64> = sqlx::query_scalar("SELECT current_version_no FROM problems WHERE id = ? AND current_version_no IS NOT NULL")
            .bind(request.problem_id).fetch_optional(&mut *transaction).await?;
        let version = version.ok_or(Error::ProblemNotFound)?;
        let info = SubmissionInfo {
            id: Uuid::new_v4().to_string(),
            problem_id: request.problem_id,
            version_no: version,
            language_id: request.language_id,
            state: SubmissionState::Queued,
            verdict: None,
            created_at_ms: now_ms()?,
            terminal_at_ms: None,
        };
        let task = load_task(&mut transaction, &info, request.source, &self.config).await?;
        sqlx::query("INSERT INTO submissions (id, user_id, problem_id, version_no, language_id, source, create_request_key, state, created_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, 'queued', ?)")
            .bind(&info.id).bind(self.config.test_user.user_id).bind(info.problem_id).bind(info.version_no)
            .bind(&info.language_id).bind(&task.request.source).bind(key).bind(info.created_at_ms)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        tracing::info!(submission = %info.id, problem = info.problem_id, version = info.version_no, language = %info.language_id, "Submission saved");
        Ok(CreatedSubmission {
            info,
            created: true,
            task: Some(task),
        })
    }

    /// Assemble the original fixed-version task only if the record is still queued.
    pub async fn queued_task(&self, id: &str) -> Result<Option<JudgingTask>> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query(&format!(
            "SELECT {INFO_COLUMNS}, source FROM submissions WHERE id = ? AND state = 'queued'"
        ))
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(
            load_task(
                &mut connection,
                &info(&row)?,
                row.try_get("source")?,
                &self.config,
            )
            .await?,
        ))
    }

    /// Read startup queued tasks in stable creation order, bounded by admission capacity.
    pub async fn queued_tasks(&self) -> Result<Vec<JudgingTask>> {
        let mut connection = self.pool.acquire().await?;
        let rows = sqlx::query(&format!("SELECT {INFO_COLUMNS}, source FROM submissions WHERE state = 'queued' ORDER BY created_at_ms, id LIMIT ?"))
            .bind(i64::from(self.config.scheduler.pending_max_count) + 1).fetch_all(&mut *connection).await?;
        if rows.len() > self.config.scheduler.pending_max_count as usize {
            return Err(Error::Data(anyhow::anyhow!(
                "Queued submissions exceed scheduler.pending_max_count; increase the configured limit before restarting"
            )));
        }
        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            tasks.push(
                load_task(
                    &mut connection,
                    &info(&row)?,
                    row.try_get("source")?,
                    &self.config,
                )
                .await?,
            );
        }
        Ok(tasks)
    }

    /// Claim a queued submission before making any Worker submission RPC.
    pub async fn mark_dispatching(&self, id: &str) -> Result<bool> {
        self.transition(id, SubmissionState::Queued, SubmissionState::Dispatching)
            .await
    }

    /// Record admission only if the submission is still dispatching.
    pub async fn mark_running(&self, id: &str) -> Result<bool> {
        self.transition(id, SubmissionState::Dispatching, SubmissionState::Running)
            .await
    }

    /// Return a clearly unadmitted task to queued without changing terminal records.
    pub async fn requeue(&self, id: &str) -> Result<bool> {
        self.transition(id, SubmissionState::Dispatching, SubmissionState::Queued)
            .await
    }

    /// Commit all terminal columns atomically; false means the previous state disallowed it.
    pub async fn save_terminal(&self, id: &str, terminal: &TerminalOutcome) -> Result<bool> {
        let details = terminal
            .details
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| Error::Data(error.into()))?;
        let changed = sqlx::query("UPDATE submissions SET state = ?, verdict = ?, terminal_details_json = ?, terminal_at_ms = ? WHERE id = ? AND state IN ('dispatching', 'running')")
            .bind(terminal.state.as_str()).bind(terminal.verdict.map(Verdict::as_str)).bind(details)
            .bind(now_ms()?).bind(id).execute(&self.pool).await?;
        Ok(changed.rows_affected() == 1)
    }

    /// Provide the later startup flow's single atomic interruption write, without contacting Workers.
    pub async fn interrupt_in_flight(&self) -> Result<u64> {
        let terminal = TerminalOutcome::failed(
            &JobFailure {
                code: "SchedulerInterrupted".into(),
                diagnostic: "Backend restarted while submission was in flight".into(),
            },
            &self.config.results,
        )?;
        let details =
            serde_json::to_string(&terminal.details).map_err(|error| Error::Data(error.into()))?;
        Ok(sqlx::query("UPDATE submissions SET state = 'failed', verdict = NULL, terminal_details_json = ?, terminal_at_ms = ? WHERE state IN ('dispatching', 'running')")
            .bind(details).bind(now_ms()?).execute(&self.pool).await?.rows_affected())
    }

    /// Share the three allowed nonterminal transitions using one conditional atomic UPDATE.
    async fn transition(
        &self,
        id: &str,
        from: SubmissionState,
        to: SubmissionState,
    ) -> Result<bool> {
        Ok(
            sqlx::query("UPDATE submissions SET state = ? WHERE id = ? AND state = ?")
                .bind(to.as_str())
                .bind(id)
                .bind(from.as_str())
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1,
        )
    }
}

/// Read an owner-scoped creation key on the caller's existing connection or transaction.
async fn by_key(
    connection: &mut SqliteConnection,
    user_id: i64,
    key: &str,
) -> Result<Option<Submission>> {
    let row = sqlx::query(&format!("SELECT {INFO_COLUMNS}, user_id, source, create_request_key, terminal_details_json FROM submissions WHERE user_id = ? AND create_request_key = ?"))
        .bind(user_id).bind(key).fetch_optional(connection).await?;
    row.as_ref().map(stored_submission).transpose()
}

/// Decode identity and summary independently of source and terminal detail payloads.
fn info(row: &SqliteRow) -> Result<SubmissionInfo> {
    let state: String = row.try_get("state")?;
    let verdict: Option<String> = row.try_get("verdict")?;
    Ok(SubmissionInfo {
        id: row.try_get("id")?,
        problem_id: row.try_get("problem_id")?,
        version_no: row.try_get("version_no")?,
        language_id: row.try_get("language_id")?,
        state: SubmissionState::parse(&state)?,
        verdict: verdict.as_deref().map(Verdict::parse).transpose()?,
        created_at_ms: row.try_get("created_at_ms")?,
        terminal_at_ms: row.try_get("terminal_at_ms")?,
    })
}

/// Deserialize typed JSON once, preserving raw source bytes and original identity.
fn stored_submission(row: &SqliteRow) -> Result<Submission> {
    let details: Option<String> = row.try_get("terminal_details_json")?;
    Ok(Submission {
        info: info(row)?,
        user_id: row.try_get("user_id")?,
        source: row.try_get("source")?,
        create_request_key: row.try_get("create_request_key")?,
        terminal_details: details
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|error| Error::Data(error.into()))?,
    })
}

/// Reject invalid source at creation and when loading pre-existing queued data.
fn validate_source(source: &[u8], config: &Config) -> Result<()> {
    if source.is_empty() {
        return Err(Error::InvalidRequest("Source must not be empty"));
    }
    if source.len() > config.request.source_max_bytes {
        return Err(Error::SourceTooLarge);
    }
    Ok(())
}

/// Assemble and validate fixed-version task data on the already held connection.
async fn load_task(
    connection: &mut SqliteConnection,
    info: &SubmissionInfo,
    source: Vec<u8>,
    config: &Config,
) -> Result<JudgingTask> {
    validate_source(&source, config)?;
    let language = sqlx::query("SELECT v.comparison_mode, l.cpu_time_ms, l.wall_time_ms, l.memory_kib FROM problem_versions v JOIN problem_version_languages l USING (problem_id, version_no) WHERE v.problem_id = ? AND v.version_no = ? AND l.language_id = ?")
        .bind(info.problem_id).bind(info.version_no).bind(&info.language_id).fetch_optional(&mut *connection).await?.ok_or(Error::LanguageNotAllowed)?;
    if language.try_get::<String, _>("comparison_mode")? != "trim_trailing_ascii_whitespace" {
        return Err(Error::Data(anyhow::anyhow!(
            "Unsupported comparison mode in stored task"
        )));
    }
    let limits = run_limits(&language)?;
    let policy = &config.request;
    if limits.cpu_time_ms == 0
        || limits.cpu_time_ms > policy.max_run_cpu_time_ms
        || limits.wall_time_ms == 0
        || limits.wall_time_ms > policy.max_run_wall_time_ms
        || limits.memory_kib == 0
        || limits.memory_kib > policy.max_run_memory_kib
    {
        return Err(Error::Data(anyhow::anyhow!(
            "Stored task limits exceed deployment ceilings"
        )));
    }
    let rows = sqlx::query("SELECT case_index, input, expected_output FROM test_cases WHERE problem_id = ? AND version_no = ? ORDER BY case_index LIMIT ?")
        .bind(info.problem_id).bind(info.version_no).bind(i64::from(policy.case_max_count) + 1).fetch_all(connection).await?;
    if rows.is_empty() || rows.len() > policy.case_max_count as usize {
        return Err(Error::Data(anyhow::anyhow!(
            "Stored task has invalid case count"
        )));
    }
    let mut cases = Vec::with_capacity(rows.len());
    let mut payload = source.len();
    for (index, row) in rows.iter().enumerate() {
        if row.try_get::<i64, _>("case_index")? != index as i64 + 1 {
            return Err(Error::Data(anyhow::anyhow!(
                "Stored case indices must be contiguous from one"
            )));
        }
        let input: Vec<u8> = row.try_get("input")?;
        let expected_output: Vec<u8> = row.try_get("expected_output")?;
        if input.len() > policy.input_max_bytes
            || expected_output.len() > policy.expected_output_max_bytes
        {
            return Err(Error::Data(anyhow::anyhow!(
                "Stored case bytes exceed deployment ceilings"
            )));
        }
        payload = payload
            .checked_add(input.len())
            .and_then(|size| size.checked_add(expected_output.len()))
            .ok_or_else(|| Error::Data(anyhow::anyhow!("Task payload size overflow")))?;
        cases.push(TestCase {
            input,
            expected_output,
        });
    }
    if payload > policy.payload_max_bytes {
        return Err(Error::Data(anyhow::anyhow!(
            "Task payload exceeds deployment ceiling"
        )));
    }
    Ok(JudgingTask {
        problem_id: info.problem_id,
        version_no: info.version_no,
        request: SubmitJobRequest {
            job_id: info.id.clone(),
            language_id: info.language_id.clone(),
            source,
            cases,
            run_limits: Some(limits),
        },
    })
}
