//! Backend startup configuration and deployment-aligned input and result budgets.

use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::Deserialize;

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
    /// HTTP listener, request body and pagination settings.
    pub http: HttpConfig,
    /// Existing SQLite file and finite lock wait.
    pub database: DatabaseConfig,
    /// Deployment-provided submission owner.
    pub test_user: TestUserConfig,
    /// In-memory admission and Worker selection settings.
    pub scheduler: SchedulerConfig,
    /// Per-call deadlines and binary message ceilings.
    pub grpc: GrpcConfig,
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

/// SQLite startup settings; the application never creates or migrates this file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Database path, relative to the process working directory when not absolute.
    pub path: PathBuf,
    /// Positive, finite SQLite lock wait in milliseconds.
    pub busy_timeout_ms: u64,
}

/// The pre-existing user assigned to all new submissions.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestUserConfig {
    /// Positive users primary key; clients cannot override it.
    pub user_id: i64,
}

/// Waiting-admission capacity and Worker dispatch settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulerConfig {
    /// Capacity including reservations, queued and awaiting-admission tasks.
    pub pending_max_count: u32,
    /// Unique gRPC endpoint URIs, such as http://127.0.0.1:50051.
    pub workers: Vec<String>,
    /// Interval between normal progress queries, in milliseconds.
    pub poll_interval_ms: u64,
    /// Delay before reusing a rejected or unavailable endpoint, in milliseconds.
    pub retry_interval_ms: u64,
}

/// Independent deadlines and transport budgets for the three Worker RPCs.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrpcConfig {
    /// SubmitJob deadline in milliseconds.
    pub submit_timeout_ms: u64,
    /// GetJob deadline in milliseconds.
    pub get_timeout_ms: u64,
    /// AcknowledgeResult deadline in milliseconds.
    pub ack_timeout_ms: u64,
    /// Maximum outgoing encoded Worker request size.
    pub max_request_bytes: usize,
    /// Maximum incoming encoded Worker reply size.
    pub max_response_bytes: usize,
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

/// Raw result limits, distinct from Base64 JSON size and gRPC encoding overhead.
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
        let config: Self = toml::from_str(text).context("Invalid backend TOML configuration")?;
        config.validate()?;
        Ok(config)
    }

    /// Check the configured resource, pagination and endpoint requirements.
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("http.max_request_bytes", self.http.max_request_bytes as u64),
            (
                "http.default_page_size",
                u64::from(self.http.default_page_size),
            ),
            ("http.max_page_size", u64::from(self.http.max_page_size)),
            ("database.busy_timeout_ms", self.database.busy_timeout_ms),
            (
                "scheduler.pending_max_count",
                u64::from(self.scheduler.pending_max_count),
            ),
            (
                "scheduler.poll_interval_ms",
                self.scheduler.poll_interval_ms,
            ),
            (
                "scheduler.retry_interval_ms",
                self.scheduler.retry_interval_ms,
            ),
            ("grpc.submit_timeout_ms", self.grpc.submit_timeout_ms),
            ("grpc.get_timeout_ms", self.grpc.get_timeout_ms),
            ("grpc.ack_timeout_ms", self.grpc.ack_timeout_ms),
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
                "{name} must be positive and fit SQLite's integer range"
            );
        }
        ensure!(
            !self.database.path.as_os_str().is_empty(),
            "database.path must not be empty"
        );
        ensure!(
            self.database.busy_timeout_ms <= i32::MAX as u64,
            "database.busy_timeout_ms exceeds SQLite's range"
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
            .context("gRPC request budget overflow")?;
        ensure!(
            self.grpc.max_request_bytes >= request_required,
            "gRPC request limit cannot hold configured payload"
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
            .context("gRPC result budget overflow")?;
        ensure!(
            self.grpc.max_response_bytes >= result_required,
            "gRPC response limit cannot hold bounded results"
        );
        Ok(())
    }
}
