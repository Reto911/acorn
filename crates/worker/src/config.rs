//! Worker startup arguments, TOML configuration and deployment validation.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::Deserialize;
use tracing::level_filters::LevelFilter;

pub use crate::kafka::KafkaConfig;

/// Command-line options for the Worker process.
#[derive(Debug, Parser)]
#[command(version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub struct Cli {
    /// Path to the Worker TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: PathBuf,
}

/// Complete Worker configuration, loaded once at startup.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Kafka transport settings for the judging communication migration.
    pub kafka: KafkaConfig,
    /// gRPC transport settings.
    pub server: ServerConfig,
    /// Incoming Job payload limits.
    pub request: RequestConfig,
    /// Job and per-case execution ceilings.
    pub execution: ExecutionConfig,
    /// Deployment-controlled toolchain commands.
    pub languages: LanguagesConfig,
    /// Compilation budgets and retained diagnostics.
    pub compile: CompileConfig,
    /// Isolate resources and lifecycle timeouts.
    pub sandbox: SandboxConfig,
    /// In-memory terminal record capacities.
    pub results: ResultsConfig,
    /// Worker log filtering.
    pub logging: LoggingConfig,
}

/// gRPC listener and binary message limits.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Address on which the Worker will listen.
    pub listen: SocketAddr,
    /// Maximum encoded incoming gRPC message size.
    pub max_request_bytes: usize,
    /// Maximum encoded outgoing gRPC message size.
    pub max_response_bytes: usize,
}

/// Limits applied to a newly submitted Job.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestConfig {
    /// Maximum Job ID length in UTF-8 bytes.
    pub job_id_max_bytes: usize,
    /// Maximum source length in bytes.
    pub source_max_bytes: usize,
    /// Maximum number of ordered test cases.
    pub case_max_count: usize,
    /// Maximum input length per case in bytes.
    pub input_max_bytes: usize,
    /// Maximum expected output length per case in bytes.
    pub expected_output_max_bytes: usize,
    /// Maximum combined source, input and answer length in bytes.
    pub payload_max_bytes: usize,
}

/// Execution limits independent of the selected language.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    /// Job deadline measured from admission, in milliseconds.
    pub job_wall_time_ms: u64,
    /// Maximum requested per-case CPU time, in milliseconds.
    pub max_run_cpu_time_ms: u64,
    /// Maximum requested per-case wall time, in milliseconds.
    pub max_run_wall_time_ms: u64,
    /// Maximum requested per-case process-tree memory, in KiB.
    pub max_run_memory_kib: u64,
    /// Process count for case execution; the first version requires one.
    pub run_process_max_count: u32,
    /// Retained stdout limit per case, in bytes.
    pub stdout_max_bytes: usize,
    /// Retained stderr limit per case, in bytes.
    pub stderr_max_bytes: usize,
}

/// Built-in C and Python toolchain settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguagesConfig {
    /// C compiler and compilation arguments.
    pub c: CConfig,
    /// Python interpreter and per-case arguments.
    pub python: PythonConfig,
}

/// Deployment-selected C compiler invocation.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CConfig {
    /// Absolute path to GCC.
    pub compiler: PathBuf,
    /// Arguments compiling main.c into the fixed program artifact.
    pub compile_arguments: Vec<String>,
}

/// Deployment-selected Python interpreter invocation.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PythonConfig {
    /// Absolute path to the Python 3 interpreter.
    pub interpreter: PathBuf,
    /// Isolated single-file invocation arguments.
    pub run_arguments: Vec<String>,
}

/// Resource and collection budgets used only during compilation.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileConfig {
    /// Compiler process-tree CPU time limit, in milliseconds.
    pub cpu_time_ms: u64,
    /// Compilation wall time limit, in milliseconds.
    pub wall_time_ms: u64,
    /// Compiler process-tree memory limit, in KiB.
    pub memory_kib: u64,
    /// Maximum compiler processes and threads.
    pub process_max_count: u32,
    /// Maximum combined retained compiler stdout and stderr, in bytes.
    pub diagnostic_max_bytes: usize,
    /// Maximum exported executable size, in bytes.
    pub artifact_max_bytes: usize,
}

/// Isolate deployment parameters shared by all stages.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Sandbox backend identifier; currently only isolate is supported.
    pub backend: String,
    /// Absolute path to the Isolate executable.
    pub executable: PathBuf,
    /// Dedicated Isolate box ID for this Worker.
    pub box_id: u32,
    /// Maximum open file descriptors inside a stage.
    pub fd_max_count: u32,
    /// Maximum size of a single file, in KiB.
    pub file_size_max_kib: u64,
    /// Shared writable storage hard quota, in KiB.
    pub disk_quota_kib: u32,
    /// Shared writable inode hard quota.
    pub disk_quota_inodes: u32,
    /// Maximum time to confirm a requested stop, in milliseconds.
    pub stop_timeout_ms: u64,
    /// Maximum time for Isolate cleanup, in milliseconds.
    pub cleanup_timeout_ms: u64,
    /// Worker graceful shutdown budget, in milliseconds.
    pub shutdown_timeout_ms: u64,
    /// Explicit environment variables visible inside the stage.
    pub environment: BTreeMap<String, String>,
}

/// Bounded terminal record retention settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultsConfig {
    /// Maximum number of terminal records awaiting ACK.
    pub max_unacknowledged_results: usize,
    /// Capacity of the acknowledged-result FIFO.
    pub acknowledged_queue_length: usize,
}

/// Text log filtering settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Minimum enabled level: off, error, warn, info, debug or trace.
    pub level: String,
}

impl Config {
    /// Read and validate a TOML file without executing toolchain commands.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read Worker configuration {}", path.display()))?;
        Self::parse(&text)
    }

    /// Deserialize startup settings and check resource relationships.
    pub fn parse(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text).context("Invalid Worker TOML configuration")?;
        config.validate()?;
        Ok(config)
    }

    /// Reject zero limits, unsupported execution settings and unusable budgets.
    pub fn validate(&self) -> Result<()> {
        self.kafka.validate()?;
        for (name, value) in [
            (
                "server.max_request_bytes",
                self.server.max_request_bytes as u64,
            ),
            (
                "server.max_response_bytes",
                self.server.max_response_bytes as u64,
            ),
            (
                "request.job_id_max_bytes",
                self.request.job_id_max_bytes as u64,
            ),
            (
                "request.source_max_bytes",
                self.request.source_max_bytes as u64,
            ),
            ("request.case_max_count", self.request.case_max_count as u64),
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
                "execution.job_wall_time_ms",
                self.execution.job_wall_time_ms,
            ),
            (
                "execution.max_run_cpu_time_ms",
                self.execution.max_run_cpu_time_ms,
            ),
            (
                "execution.max_run_wall_time_ms",
                self.execution.max_run_wall_time_ms,
            ),
            (
                "execution.max_run_memory_kib",
                self.execution.max_run_memory_kib,
            ),
            (
                "execution.stdout_max_bytes",
                self.execution.stdout_max_bytes as u64,
            ),
            (
                "execution.stderr_max_bytes",
                self.execution.stderr_max_bytes as u64,
            ),
            ("compile.cpu_time_ms", self.compile.cpu_time_ms),
            ("compile.wall_time_ms", self.compile.wall_time_ms),
            ("compile.memory_kib", self.compile.memory_kib),
            (
                "compile.process_max_count",
                self.compile.process_max_count as u64,
            ),
            (
                "compile.diagnostic_max_bytes",
                self.compile.diagnostic_max_bytes as u64,
            ),
            (
                "compile.artifact_max_bytes",
                self.compile.artifact_max_bytes as u64,
            ),
            ("sandbox.fd_max_count", self.sandbox.fd_max_count as u64),
            ("sandbox.file_size_max_kib", self.sandbox.file_size_max_kib),
            ("sandbox.disk_quota_kib", self.sandbox.disk_quota_kib as u64),
            (
                "sandbox.disk_quota_inodes",
                self.sandbox.disk_quota_inodes as u64,
            ),
            ("sandbox.stop_timeout_ms", self.sandbox.stop_timeout_ms),
            (
                "sandbox.cleanup_timeout_ms",
                self.sandbox.cleanup_timeout_ms,
            ),
            (
                "sandbox.shutdown_timeout_ms",
                self.sandbox.shutdown_timeout_ms,
            ),
            (
                "results.max_unacknowledged_results",
                self.results.max_unacknowledged_results as u64,
            ),
            (
                "results.acknowledged_queue_length",
                self.results.acknowledged_queue_length as u64,
            ),
        ] {
            ensure!(value > 0, "{name} must be positive");
        }
        for (name, value) in [
            (
                "execution.max_run_memory_kib",
                self.execution.max_run_memory_kib,
            ),
            ("compile.memory_kib", self.compile.memory_kib),
            ("sandbox.file_size_max_kib", self.sandbox.file_size_max_kib),
            (
                "sandbox.disk_quota_kib",
                u64::from(self.sandbox.disk_quota_kib),
            ),
        ] {
            ensure!(
                value <= i32::MAX as u64,
                "{name} exceeds Isolate's supported KiB range"
            );
        }
        ensure!(
            self.sandbox.backend == "isolate",
            "sandbox.backend must be isolate"
        );
        ensure!(
            self.execution.run_process_max_count == 1,
            "execution.run_process_max_count must be 1"
        );
        ensure!(
            self.request.case_max_count <= u32::MAX as usize,
            "request.case_max_count exceeds the protocol range"
        );
        ensure!(
            self.execution.max_run_cpu_time_ms <= self.execution.max_run_wall_time_ms,
            "Run CPU time must not exceed wall time"
        );
        ensure!(
            self.compile.cpu_time_ms <= self.compile.wall_time_ms,
            "Compile CPU time must not exceed wall time"
        );
        ensure!(
            self.compile.artifact_max_bytes as u64 <= self.sandbox.file_size_max_kib * 1024,
            "Artifact limit exceeds the single-file limit"
        );
        ensure!(
            self.compile.artifact_max_bytes as u64 <= u64::from(self.sandbox.disk_quota_kib) * 1024,
            "Artifact limit exceeds the disk quota"
        );
        let shutdown_required = self
            .sandbox
            .stop_timeout_ms
            .checked_add(self.sandbox.cleanup_timeout_ms)
            .context("Sandbox lifecycle timeout overflow")?;
        ensure!(
            self.sandbox.shutdown_timeout_ms >= shutdown_required,
            "Shutdown budget must cover stopping and cleanup"
        );
        // Allow for the Job ID, Proto tags, lengths, progress, limits and exit metadata.
        let request_envelope = self
            .request
            .case_max_count
            .checked_mul(32)
            .and_then(|size| size.checked_add(self.request.job_id_max_bytes))
            .and_then(|size| size.checked_add(1024))
            .and_then(|size| size.checked_add(self.request.payload_max_bytes))
            .context("Request message budget overflow")?;
        ensure!(
            self.server.max_request_bytes >= request_envelope,
            "Request message limit cannot hold the configured payload and protocol envelope"
        );
        ensure!(
            self.kafka.max_task_record_bytes >= request_envelope,
            "Kafka task record limit cannot hold configured payload"
        );
        let response_payload = self
            .execution
            .stdout_max_bytes
            .checked_add(self.execution.stderr_max_bytes)
            .context("Result output budget overflow")?
            .max(self.compile.diagnostic_max_bytes);
        let response_required = response_payload
            .checked_add(self.request.job_id_max_bytes)
            .and_then(|size| size.checked_add(1024))
            .context("Result message budget overflow")?;
        ensure!(
            self.server.max_response_bytes >= response_required,
            "Response message limit cannot hold bounded results"
        );
        ensure!(
            self.kafka.max_event_record_bytes >= response_required,
            "Kafka event record limit cannot hold bounded results"
        );
        self.logging
            .level
            .parse::<LevelFilter>()
            .context("Invalid logging.level")?;
        for path in [
            &self.sandbox.executable,
            &self.languages.c.compiler,
            &self.languages.python.interpreter,
        ] {
            ensure!(
                path.is_absolute(),
                "Tool path must be absolute: {}",
                path.display()
            );
        }
        for arguments in [
            &self.languages.c.compile_arguments,
            &self.languages.python.run_arguments,
        ] {
            ensure!(!arguments.is_empty(), "Tool arguments must not be empty");
            ensure!(
                arguments.iter().all(|arg| !arg.contains('\0')),
                "Tool arguments must not contain NUL"
            );
        }
        for (key, value) in &self.sandbox.environment {
            ensure!(
                !key.is_empty() && !key.contains(['=', '\0']) && !value.contains('\0'),
                "Invalid sandbox environment entry: {key:?}"
            );
        }
        Ok(())
    }

    /// Confirm that Isolate, GCC and Python can be invoked by the startup user.
    pub fn check_tools(&self) -> Result<()> {
        for path in [
            &self.sandbox.executable,
            &self.languages.c.compiler,
            &self.languages.python.interpreter,
        ] {
            let status = Command::new(path)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .with_context(|| format!("Failed to start configured tool {}", path.display()))?;
            ensure!(
                status.success(),
                "Configured tool version check failed: {} ({status})",
                path.display()
            );
        }
        Ok(())
    }
}
