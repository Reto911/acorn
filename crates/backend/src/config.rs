//! Backend startup configuration and deployment-aligned input and result budgets.

use std::{
    collections::HashSet,
    fmt,
    net::{Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::Deserialize;
use sqlx::mysql::{MySqlConnectOptions, MySqlSslMode};

pub use acorn_kafka::KafkaConfig;

use crate::llm_review::{ReviewAttempt, ReviewTrigger, policy::PROMPT_VERSION};

/// Command-line options for the backend process.
#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    /// Path to the backend TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: PathBuf,
}

/// Complete backend configuration, loaded once at startup.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Kafka transport settings for the judging communication migration.
    pub kafka: KafkaConfig,
    /// Independent Worker consumer-group observation settings.
    #[serde(default)]
    pub worker_membership: WorkerMembershipConfig,
    /// HTTP listener, request body and pagination settings.
    pub http: HttpConfig,
    /// Existing MySQL database, bounded connection pool and finite lock waits.
    pub database: DatabaseConfig,
    /// Deployment-provided submission owner.
    pub test_user: TestUserConfig,
    /// In-memory admission and Worker selection settings.
    pub scheduler: SchedulerConfig,
    /// Per-call deadlines and binary message ceilings.
    pub grpc: GrpcConfig,
    /// Independent read-only observation cadence and per-check budget.
    #[serde(default)]
    pub health: HealthConfig,
    /// Optional independent LLM review settings; old configurations keep reviews disabled.
    #[serde(default)]
    pub llm_review: LlmReviewConfig,
    /// Source, case and per-case execution ceilings.
    pub request: RequestConfig,
    /// Bounded terminal payload settings, aligned with Workers.
    pub results: ResultsConfig,
    /// English stderr log filtering.
    pub logging: LoggingConfig,
}

/// HTTP listener, JSON extraction and list pagination settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    /// Listening address.
    pub listen: SocketAddr,
    /// Encoded JSON request body limit in bytes.
    pub max_request_bytes: usize,
    /// Default number of items per list page.
    pub default_page_size: u32,
    /// Maximum number of items per list page.
    pub max_page_size: u32,
}

/// MySQL startup settings; initialization belongs to the deployment account.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Host and TCP port, with IPv6 addresses enclosed in square brackets.
    pub endpoint: String,
    /// Existing database name; applications never create it at startup.
    pub name: String,
    /// MySQL account name; MYSQL_PASSWORD supplies its password separately.
    pub username: String,
    /// Shared application connection ceiling, defaulting to four.
    #[serde(default = "default_database_connections")]
    pub max_connections: u32,
    /// Positive budget for the entire connection acquisition phase, in milliseconds.
    pub acquire_timeout_ms: u64,
    /// Positive session InnoDB and metadata lock wait, in seconds.
    pub lock_wait_timeout_secs: u32,
}

/// Preserve the agreed four-connection application pool default.
fn default_database_connections() -> u32 {
    4
}

impl DatabaseConfig {
    /// Parse the configured host and positive TCP port without resolving DNS.
    pub fn address(&self) -> Result<(&str, u16)> {
        let (host, port) = if let Some(ipv6) = self.endpoint.strip_prefix('[') {
            let (host, port) = ipv6
                .split_once("]:")
                .context("database.endpoint must use [IPv6]:port")?;
            host.parse::<Ipv6Addr>()
                .context("database.endpoint contains an invalid IPv6 address")?;
            (host, port)
        } else {
            let (host, port) = self
                .endpoint
                .rsplit_once(':')
                .context("database.endpoint must use host:port")?;
            ensure!(
                !host.contains(':'),
                "database.endpoint IPv6 addresses require square brackets"
            );
            (host, port)
        };
        ensure!(
            !host.is_empty()
                && !host.chars().any(|character| {
                    character.is_whitespace()
                        || matches!(character, '/' | '@' | '[' | ']' | '?' | '#')
                }),
            "database.endpoint must contain only a host and port"
        );
        let port: u16 = port
            .parse()
            .context("database.endpoint port must fit a TCP port")?;
        ensure!(port > 0, "database.endpoint port must be positive");
        Ok((host, port))
    }

    /// Build credential-bearing options without selecting a database or logging them.
    ///
    /// Application and test callers select their own database after construction.
    /// Configuration parsing remains independent of the process password variable.
    pub fn connect_options(&self) -> Result<MySqlConnectOptions> {
        let (host, port) = self.address()?;
        let password = std::env::var("MYSQL_PASSWORD").map_err(|_| {
            anyhow::anyhow!(
                "MYSQL_PASSWORD must be present in the process environment as valid UTF-8"
            )
        })?;
        Ok(MySqlConnectOptions::new()
            .host(host)
            .port(port)
            .username(&self.username)
            .password(&password)
            .charset("utf8mb4")
            .collation("utf8mb4_0900_bin")
            .ssl_mode(MySqlSslMode::Disabled))
    }
}

/// The pre-existing user assigned to all new submissions.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestUserConfig {
    /// Positive users primary key; clients cannot override it.
    pub user_id: i64,
}

/// Waiting-admission capacity, status endpoints and publication retry settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    /// Capacity including reservations, queued and awaiting-admission tasks.
    pub pending_max_count: u32,
    /// Unique gRPC endpoint URIs, such as http://127.0.0.1:50051.
    pub workers: Vec<String>,
    /// Delay before retrying an uncertain Kafka task publication, in milliseconds.
    pub retry_interval_ms: u64,
}

/// Transport message budgets for read-only Worker status RPCs.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrpcConfig {
    /// Maximum outgoing encoded Worker request size.
    pub max_request_bytes: usize,
    /// Maximum incoming encoded Worker reply size.
    pub max_response_bytes: usize,
}

/// Shared health collection settings, with defaults for existing deployment files.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthConfig {
    /// Positive delay after a completed round, in milliseconds.
    pub interval_ms: u64,
    /// Positive budget for one database, Kafka or Worker check, in milliseconds.
    pub timeout_ms: u64,
}

impl Default for HealthConfig {
    /// Preserve the agreed five-second interval and one-second check budget.
    fn default() -> Self {
        Self {
            interval_ms: 5_000,
            timeout_ms: 1_000,
        }
    }
}

impl HealthConfig {
    /// Derive the observation age limit without overflowing the configured milliseconds.
    pub fn stale_after(&self) -> Result<Duration> {
        self.interval_ms
            .checked_add(self.timeout_ms)
            .and_then(|cycle| cycle.checked_mul(2))
            .map(Duration::from_millis)
            .context("Health stale threshold overflows milliseconds")
    }
}

/// Independent consumer-group observation, separate from health sampling.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerMembershipConfig {
    /// Worker task group to observe, matching the deployed Workers' Kafka group_id.
    pub tasks_group_id: String,
    /// Delay after a completed group observation, in milliseconds.
    pub interval_ms: u64,
    /// Maximum duration of one blocking group query, in milliseconds.
    pub timeout_ms: u64,
}

impl Default for WorkerMembershipConfig {
    /// Use a five-second interval and a one-second query budget.
    fn default() -> Self {
        Self {
            tasks_group_id: "acorn-judge-workers-v1".into(),
            interval_ms: 5_000,
            timeout_ms: 1_000,
        }
    }
}

impl WorkerMembershipConfig {
    /// Reject invalid SDK timeouts and overflowing observation ages.
    pub fn validate(&self) -> Result<()> {
        acorn_kafka::validate_group_id(&self.tasks_group_id, "worker_membership.tasks_group_id")?;
        ensure!(
            self.interval_ms > 0,
            "worker_membership.interval_ms must be positive"
        );
        ensure!(
            (1..=i32::MAX as u64).contains(&self.timeout_ms),
            "worker_membership.timeout_ms must fit positive client milliseconds"
        );
        self.stale_after()?;
        Ok(())
    }

    /// Consider a sample expired after two complete observation budgets.
    pub fn stale_after(&self) -> Result<Duration> {
        self.interval_ms
            .checked_add(self.timeout_ms)
            .and_then(|cycle| cycle.checked_mul(2))
            .map(Duration::from_millis)
            .context("Worker membership stale threshold overflows milliseconds")
    }
}

/// Raw task ceilings aligned by deployment with the Worker request contract.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestConfig {
    /// Maximum source length in bytes.
    pub source_max_bytes: usize,
    /// Maximum ordered case count.
    pub case_max_count: u32,
    /// Maximum input length per case.
    pub input_max_bytes: usize,
    /// Maximum expected output length per case.
    pub expected_output_max_bytes: usize,
    /// Combined source, input and expected-output byte limit.
    pub payload_max_bytes: usize,
    /// Per-case CPU ceiling in milliseconds.
    pub max_run_cpu_time_ms: u64,
    /// Per-case wall-clock ceiling in milliseconds.
    pub max_run_wall_time_ms: u64,
    /// Per-case memory ceiling in KiB.
    pub max_run_memory_kib: u64,
}

/// Raw result limits, distinct from Base64 JSON size and event encoding overhead.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultsConfig {
    /// Combined compiler stdout/stderr budget.
    pub compile_diagnostic_max_bytes: usize,
    /// Retained failed-case stdout budget.
    pub stdout_max_bytes: usize,
    /// Retained failed-case stderr budget.
    pub stderr_max_bytes: usize,
    /// UTF-8 execution-failure diagnostic budget.
    pub failure_diagnostic_max_bytes: usize,
}

/// Process-wide text log filtering.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Minimum enabled tracing level.
    pub level: String,
}

/// Single-provider Responses settings, read only from backend TOML in formal programs.
#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LlmReviewConfig {
    /// Whether the independent review service may admit work.
    pub enabled: bool,
    /// Complete POST URL, with no automatic path suffix.
    pub endpoint: String,
    /// Server-side Bearer credential, excluded from Debug output.
    pub api_key: String,
    /// Actual requested model name.
    pub model: String,
    /// Provider-supported none, low, high or max effort.
    pub reasoning_effort: String,
    /// Total HTTP send and response-read deadline, in milliseconds.
    pub timeout_ms: u64,
    /// Independent running capacity, consumed by the review queue service.
    pub max_concurrent: u32,
    /// Independent pending capacity, consumed by the review queue service.
    pub pending_max_count: u32,
    /// Combined visible and reasoning token budget.
    pub max_output_tokens: u64,
    /// Complete serialized request byte limit.
    pub max_request_bytes: usize,
    /// Response byte limit enforced while reading chunks.
    pub max_response_bytes: usize,
}

impl Default for LlmReviewConfig {
    /// Preserve disabled admission and the approved initial model and budgets.
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "https://api.deepseek.com/responses".into(),
            api_key: String::new(),
            model: "deepseek-flash".into(),
            reasoning_effort: "max".into(),
            timeout_ms: 120_000,
            max_concurrent: 2,
            pending_max_count: 16,
            max_output_tokens: 10_240,
            max_request_bytes: 1_048_576,
            max_response_bytes: 1_048_576,
        }
    }
}

impl fmt::Debug for LlmReviewConfig {
    /// Allow enclosing configuration diagnostics without printing credentials or the endpoint.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LlmReviewConfig")
            .field("enabled", &self.enabled)
            .field("api_key", &"[redacted]")
            .field("model", &self.model)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("timeout_ms", &self.timeout_ms)
            .field("max_concurrent", &self.max_concurrent)
            .field("pending_max_count", &self.pending_max_count)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("max_request_bytes", &self.max_request_bytes)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl LlmReviewConfig {
    /// Validate enabled settings without network access or a charged startup probe.
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        ensure!(
            reqwest::Url::parse(&self.endpoint)
                .ok()
                .is_some_and(
                    |url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
                ),
            "llm_review.endpoint must be a complete HTTP(S) URL"
        );
        ensure!(
            !self.api_key.trim().is_empty(),
            "llm_review.api_key must not be empty"
        );
        ensure!(
            !self.model.trim().is_empty(),
            "llm_review.model must not be empty"
        );
        ensure!(
            matches!(
                self.reasoning_effort.as_str(),
                "none" | "low" | "high" | "max"
            ),
            "llm_review.reasoning_effort must be none, low, high or max"
        );
        for (name, value) in [
            ("timeout_ms", self.timeout_ms),
            ("max_concurrent", u64::from(self.max_concurrent)),
            ("pending_max_count", u64::from(self.pending_max_count)),
            ("max_output_tokens", self.max_output_tokens),
            ("max_request_bytes", self.max_request_bytes as u64),
            ("max_response_bytes", self.max_response_bytes as u64),
        ] {
            ensure!(value > 0, "llm_review.{name} must be positive");
        }
        ReviewAttempt::pending(
            ReviewTrigger::Auto,
            i64::MIN,
            self.model.clone(),
            self.reasoning_effort.clone(),
            PROMPT_VERSION.into(),
        )
        .context("Review admission metadata exceeds persistence budget")?;
        Ok(())
    }
}

impl Config {
    /// Read TOML and validate startup relationships without opening the database.
    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(
            &std::fs::read_to_string(path)
                .with_context(|| format!("Read backend configuration {}", path.display()))?,
        )
    }

    /// Parse a complete configuration and reject unsupported budgets.
    pub fn parse(text: &str) -> Result<Self> {
        // TOML errors normally echo source lines, which may now contain an API key.
        let config: Self = toml::from_str(text).map_err(|error: toml::de::Error| {
            let span = error.span().map(|span| span.start);
            anyhow::anyhow!("Invalid backend TOML configuration at byte {span:?}")
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Check the configured resource, pagination and endpoint requirements.
    pub fn validate(&self) -> Result<()> {
        self.kafka.validate()?;
        self.worker_membership.validate()?;
        self.llm_review.validate()?;
        ensure!(
            self.health.interval_ms > 0,
            "health.interval_ms must be positive"
        );
        ensure!(
            (1..=i32::MAX as u64).contains(&self.health.timeout_ms),
            "health.timeout_ms must fit positive client milliseconds"
        );
        self.health.stale_after()?;
        for (name, value) in [
            ("http.max_request_bytes", self.http.max_request_bytes as u64),
            (
                "http.default_page_size",
                u64::from(self.http.default_page_size),
            ),
            ("http.max_page_size", u64::from(self.http.max_page_size)),
            (
                "database.acquire_timeout_ms",
                self.database.acquire_timeout_ms,
            ),
            (
                "scheduler.pending_max_count",
                u64::from(self.scheduler.pending_max_count),
            ),
            (
                "scheduler.retry_interval_ms",
                self.scheduler.retry_interval_ms,
            ),
            ("grpc.max_request_bytes", self.grpc.max_request_bytes as u64),
            (
                "grpc.max_response_bytes",
                self.grpc.max_response_bytes as u64,
            ),
            (
                "request.source_max_bytes",
                self.request.source_max_bytes as u64,
            ),
            (
                "request.case_max_count",
                u64::from(self.request.case_max_count),
            ),
            (
                "request.input_max_bytes",
                self.request.input_max_bytes as u64,
            ),
            (
                "request.expected_output_max_bytes",
                self.request.expected_output_max_bytes as u64,
            ),
            (
                "request.payload_max_bytes",
                self.request.payload_max_bytes as u64,
            ),
            (
                "request.max_run_cpu_time_ms",
                self.request.max_run_cpu_time_ms,
            ),
            (
                "request.max_run_wall_time_ms",
                self.request.max_run_wall_time_ms,
            ),
            (
                "request.max_run_memory_kib",
                self.request.max_run_memory_kib,
            ),
            (
                "results.compile_diagnostic_max_bytes",
                self.results.compile_diagnostic_max_bytes as u64,
            ),
            (
                "results.stdout_max_bytes",
                self.results.stdout_max_bytes as u64,
            ),
            (
                "results.stderr_max_bytes",
                self.results.stderr_max_bytes as u64,
            ),
            (
                "results.failure_diagnostic_max_bytes",
                self.results.failure_diagnostic_max_bytes as u64,
            ),
        ] {
            ensure!(
                value > 0 && value <= i64::MAX as u64,
                "{name} must be positive and fit the signed integer range"
            );
        }
        ensure!(
            !self.database.name.trim().is_empty(),
            "database.name must not be empty"
        );
        ensure!(
            !self.database.username.trim().is_empty(),
            "database.username must not be empty"
        );
        self.database.address()?;
        ensure!(
            self.database.max_connections > 0,
            "database.max_connections must be positive"
        );
        ensure!(
            (1..=31_536_000).contains(&self.database.lock_wait_timeout_secs),
            "database.lock_wait_timeout_secs must be between 1 and 31536000"
        );
        ensure!(
            self.test_user.user_id > 0,
            "test_user.user_id must be positive"
        );
        ensure!(
            self.http.default_page_size <= self.http.max_page_size,
            "Default page size exceeds maximum"
        );
        ensure!(
            self.request.max_run_cpu_time_ms <= self.request.max_run_wall_time_ms,
            "Run CPU ceiling exceeds wall ceiling"
        );
        ensure!(
            self.request.max_run_memory_kib <= i32::MAX as u64,
            "request.max_run_memory_kib exceeds Isolate's supported KiB range"
        );
        ensure!(
            !self.scheduler.workers.is_empty(),
            "At least one Worker endpoint is required"
        );
        let mut endpoints = HashSet::new();
        for endpoint in &self.scheduler.workers {
            tonic::transport::Endpoint::from_shared(endpoint.clone())
                .context("Invalid Worker endpoint URI")?;
            ensure!(
                endpoints.insert(endpoint),
                "Duplicate Worker endpoint: {endpoint}"
            );
        }
        self.logging
            .level
            .parse::<tracing::level_filters::LevelFilter>()
            .context("Invalid logging.level")?;
        let http_required = self
            .request
            .source_max_bytes
            .checked_mul(6)
            .and_then(|size| size.checked_add(1024))
            .context("HTTP body budget overflow")?;
        ensure!(
            self.http.max_request_bytes >= http_required,
            "HTTP body limit cannot hold JSON-escaped source"
        );
        let request_required = (self.request.case_max_count as usize)
            .checked_mul(32)
            .and_then(|size| size.checked_add(self.request.payload_max_bytes))
            .and_then(|size| size.checked_add(36 + 1024))
            .context("Task message budget overflow")?;
        ensure!(
            self.kafka.max_task_record_bytes >= request_required,
            "Kafka task record limit cannot hold configured payload"
        );
        let result_required = self
            .results
            .stdout_max_bytes
            .checked_add(self.results.stderr_max_bytes)
            .map(|size| {
                size.max(self.results.compile_diagnostic_max_bytes)
                    .max(self.results.failure_diagnostic_max_bytes)
            })
            .and_then(|size| size.checked_add(36 + 1024))
            .context("Event message budget overflow")?;
        ensure!(
            self.kafka.max_event_record_bytes >= result_required,
            "Kafka event record limit cannot hold bounded results"
        );
        Ok(())
    }
}
