//! Latest review reads, conditional persistence and explicit startup interruption.

use sqlx::{Row, mysql::MySqlRow};

use crate::{
    llm_review::{ReviewAttempt, ReviewDetails, ReviewErrorCode, ReviewStatus},
    submissions::Verdict,
};

use super::{Database, Error, Result, now_ms};

/// One transactional manual-admission decision, without ownership of queue resources.
pub(crate) enum ReviewAdmission {
    /// No submission with the requested identity exists.
    NotFound,
    /// Only a finished machine result permits review.
    NotFinished,
    /// Reuse the latest pending attempt before checking the expected identity or capacity.
    Existing(Box<ReviewAttempt>),
    /// The latest review differs from the caller's expected identity.
    Changed,
    /// No capacity was reserved and the previous opinion remains intact.
    QueueFull,
    /// The supplied fresh pending attempt was committed.
    Started,
}

/// Public fixed-version inputs, deliberately excluding private cases and user identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewInput {
    /// Original UTF-8 source, with no preprocessing or newline normalization.
    pub source: String,
    /// Original language identifier.
    pub language_id: String,
    /// Title from the submission's immutable problem version.
    pub title: String,
    /// Public statement from that same immutable version.
    pub statement: String,
    /// Persisted machine verdict, without case progress or failure details.
    pub verdict: Verdict,
}

impl Database {
    /// Read finished-review input and release the connection before returning to the caller.
    pub async fn review_input(&self, id: &str) -> Result<Option<ReviewInput>> {
        self.observe("Read review input", false, async {
            let row = sqlx::query("SELECT s.source, s.language_id, s.verdict, v.title, v.statement FROM submissions s JOIN problem_versions v USING (problem_id, version_no) WHERE s.id = ? AND s.state = 'finished'")
                .bind(id).fetch_optional(&self.pool).await?;
            row.map(|row| {
                let source: Vec<u8> = row.try_get("source")?;
                Ok(ReviewInput {
                    source: String::from_utf8(source).map_err(|error| Error::Data(error.into()))?,
                    language_id: row.try_get("language_id")?,
                    title: row.try_get("title")?,
                    statement: row.try_get("statement")?,
                    verdict: Verdict::parse(&row.try_get::<String, _>("verdict")?)?,
                })
            }).transpose()
        }).await
    }

    /// Replace the latest finished submission's review only when its expected identity matches.
    ///
    /// This storage operation owns no queue capacity; its caller must reserve capacity
    /// and own the operation's lifetime before calling it.
    pub async fn start_review(
        &self,
        id: &str,
        expected_review_id: Option<&str>,
        review: &ReviewAttempt,
    ) -> Result<bool> {
        Ok(matches!(
            self.admit_review(id, expected_review_id, review, true)
                .await?,
            ReviewAdmission::Started
        ))
    }

    /// Check and replace one latest review in the same short transaction, releasing before handoff.
    pub(crate) async fn admit_review(
        &self,
        id: &str,
        expected_review_id: Option<&str>,
        review: &ReviewAttempt,
        has_capacity: bool,
    ) -> Result<ReviewAdmission> {
        self.observe("Start review", true, async {
            if review.status != ReviewStatus::Pending || expected_review_id == Some(&review.id) {
                return Err(Error::InvalidReview("Admission requires a fresh pending review"));
            }
            let json = review.encode(0)?;
            let mut transaction = self.pool.begin().await?;
            let row = sqlx::query("SELECT state, review_status, review_id, review_details_json FROM submissions WHERE id = ?")
                .bind(id).fetch_optional(&mut *transaction).await?;
            let admission = match row {
                None => ReviewAdmission::NotFound,
                Some(row) if row.try_get::<String, _>("state")? != "finished" => ReviewAdmission::NotFinished,
                Some(row) => {
                    let current = stored_review(&row)?;
                    if let Some(pending) = current.as_ref().filter(|attempt| attempt.status == ReviewStatus::Pending) {
                        ReviewAdmission::Existing(Box::new(pending.clone()))
                    } else if current.as_ref().map(|attempt| attempt.id.as_str()) != expected_review_id {
                        ReviewAdmission::Changed
                    } else if !has_capacity {
                        ReviewAdmission::QueueFull
                    } else {
                        sqlx::query("UPDATE submissions SET review_status = 'PENDING', review_id = ?, review_details_json = ? WHERE id = ?")
                            .bind(&review.id).bind(json).bind(id).execute(&mut *transaction).await?;
                        ReviewAdmission::Started
                    }
                }
            };
            transaction.commit().await?;
            Ok(admission)
        }).await
    }

    /// Save one terminal opinion or failure only for the matching still-pending attempt.
    pub async fn finish_review(&self, id: &str, review: &ReviewAttempt) -> Result<bool> {
        self.observe("Finish review", true, async {
            if !matches!(review.status, ReviewStatus::Pass | ReviewStatus::Doubtful | ReviewStatus::Failed) {
                return Err(Error::InvalidReview("Completion requires a terminal review"));
            }
            let mut transaction = self.pool.begin().await?;
            let source: Option<Vec<u8>> = sqlx::query_scalar("SELECT source FROM submissions WHERE id = ? AND review_id = ? AND review_status = 'PENDING'")
                .bind(id).bind(&review.id).fetch_optional(&mut *transaction).await?;
            let Some(source) = source else {
                transaction.commit().await?;
                return Ok(false);
            };
            let source_lines = source.split_inclusive(|byte| *byte == b'\n').count();
            let json = review.encode(source_lines)?;
            let changed = sqlx::query("UPDATE submissions SET review_status = ?, review_details_json = ? WHERE id = ? AND review_id = ? AND review_status = 'PENDING'")
                .bind(review.status.as_str()).bind(json).bind(id).bind(&review.id)
                .execute(&mut *transaction).await?;
            transaction.commit().await?;
            Ok(changed.rows_affected() == 1)
        }).await
    }

    /// End old pending attempts on explicit startup, retaining their IDs and request metadata.
    ///
    /// The program entry point invokes this before admission; it never re-enqueues work.
    pub async fn interrupt_pending_reviews(&self) -> Result<u64> {
        self.observe("Interrupt old pending reviews", true, async {
            let mut transaction = self.pool.begin().await?;
            let rows = sqlx::query("SELECT id, review_status, review_id, review_details_json FROM submissions WHERE review_status = 'PENDING'")
                .fetch_all(&mut *transaction).await?;
            let completed_at_ms = now_ms()?;
            let mut count = 0;
            for row in rows {
                let review = stored_review(&row)?.ok_or(Error::InvalidReview("Pending review is missing"))?
                    .failed(completed_at_ms, ReviewErrorCode::ReviewInterrupted, None, None, None)?;
                count += sqlx::query("UPDATE submissions SET review_status = 'FAILED', review_details_json = ? WHERE id = ? AND review_id = ? AND review_status = 'PENDING'")
                    .bind(review.encode(0)?).bind(row.try_get::<String, _>("id")?).bind(&review.id)
                    .execute(&mut *transaction).await?.rows_affected();
            }
            transaction.commit().await?;
            Ok(count)
        }).await
    }
}

/// Parse the latest attempt separately from machine terminal details.
pub(super) fn stored_review(row: &MySqlRow) -> Result<Option<ReviewAttempt>> {
    let status = ReviewStatus::parse(&row.try_get::<String, _>("review_status")?)?;
    if status == ReviewStatus::Disabled {
        return Ok(None);
    }
    let json: String = row.try_get("review_details_json")?;
    let details: ReviewDetails =
        serde_json::from_str(&json).map_err(|error| Error::Data(error.into()))?;
    Ok(Some(ReviewAttempt {
        id: row.try_get("review_id")?,
        status,
        details,
    }))
}
