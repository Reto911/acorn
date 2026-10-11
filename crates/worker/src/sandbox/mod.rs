//! Complete, serial Isolate stages with bounded output and explicit stop failures.

mod isolate;
mod metadata;
mod output;

use std::path::PathBuf;

use tokio::{sync::watch, time::Instant};

use crate::language::{CommandPlan, StageLimits};

pub use isolate::IsolateSandbox;

/// An independent file copied into a fresh stage under a deployment-chosen name.
#[derive(Debug)]
pub struct SandboxFile {
    /// Single relative filename; never derived from a Job ID or user path.
    pub name: String,
    /// Original source or previously exported executable bytes.
    pub bytes: Vec<u8>,
    /// Whether the copied file must be executable.
    pub executable: bool,
}

/// Bounded collection differs between compilation diagnostics and case output.
#[derive(Clone, Copy, Debug)]
pub enum OutputPolicy {
    /// Retain at most this many bytes across both streams, draining the remainder.
    Compile {
        /// Combined diagnostic budget.
        max_bytes: usize,
    },
    /// Stop the stage when either stream exceeds its independent limit.
    Run {
        /// Maximum retained stdout bytes.
        stdout_max_bytes: usize,
        /// Maximum retained stderr bytes.
        stderr_max_bytes: usize,
    },
}

/// Fixed compiler artifact eligible for export after successful execution.
#[derive(Debug)]
pub struct ArtifactSpec {
    /// Single relative filename supplied by the language plan.
    pub name: String,
    /// Maximum exported size in bytes.
    pub max_bytes: usize,
}

/// All trusted inputs needed for one compilation or one case execution.
#[derive(Debug)]
pub struct SandboxRunSpec {
    /// Deployment-controlled executable, arguments and finite environment.
    pub command: CommandPlan,
    /// Independent copies placed in the newly initialized working directory.
    pub files: Vec<SandboxFile>,
    /// Only the current stage's standard input.
    pub stdin: Vec<u8>,
    /// CPU, elapsed time, memory and process limits.
    pub limits: StageLimits,
    /// Retention and output overflow behavior.
    pub output: OutputPolicy,
    /// Optional fixed compiler output; runtime stages never export files.
    pub artifact: Option<ArtifactSpec>,
}

/// Original reason for a Worker-initiated stop, preserved across Isolate SIGTERM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Runtime stdout or stderr exceeded its budget.
    OutputLimit,
    /// Deadline measured from Job admission expired.
    JobDeadline,
    /// Worker is shutting down.
    Shutdown,
    /// Stage I/O failed and execution must stop before returning the error.
    Infrastructure,
}

/// Job deadline and internal stop channel; independent of RPC connection lifetime.
#[derive(Clone)]
pub struct StageControl {
    /// Absolute deadline established at Job admission.
    pub deadline: Instant,
    /// Internal stop requests; channel closure alone does not request a stop.
    pub stop: watch::Receiver<Option<StopReason>>,
}

/// Available measurements; missing Isolate statistics remain absent.
#[derive(Debug, Default)]
pub struct ResourceUsage {
    /// Process-tree CPU consumption in microseconds.
    pub cpu_time_us: Option<u64>,
    /// Elapsed execution time in microseconds.
    pub wall_time_us: Option<u64>,
    /// Peak cgroup memory in bytes, without substituting single-process RSS.
    pub memory_bytes: Option<u64>,
}

/// Retained bytes and whether further bytes were discarded.
#[derive(Debug, Default)]
pub struct CapturedOutput {
    /// Bounded retained prefix.
    pub bytes: Vec<u8>,
    /// True when the stream contained additional bytes.
    pub truncated: bool,
}

/// Confirmed execution facts; verdict selection belongs to the judging executor.
#[derive(Debug)]
pub struct SandboxRunOutcome {
    /// Bounded standard output.
    pub stdout: CapturedOutput,
    /// Bounded standard error.
    pub stderr: CapturedOutput,
    /// Program exit code, distinct from the Isolate manager exit code.
    pub exit_code: Option<i32>,
    /// Program signal (or Isolate interruption signal when actively stopped).
    pub exit_signal: Option<i32>,
    /// Isolate reported cgroup OOM, rather than merely SIGKILL.
    pub memory_limit_exceeded: bool,
    /// Isolate reported a CPU or elapsed-time limit.
    pub time_limit_exceeded: bool,
    /// Runtime collection exceeded stdout or stderr retention limits.
    pub output_limit_exceeded: bool,
    /// Original Worker stop cause, if one was necessary.
    pub stop_reason: Option<StopReason>,
    /// Optional actual measurements.
    pub usage: ResourceUsage,
    /// Independently copied bytes of a successful compiler artifact.
    pub artifact: Option<Vec<u8>>,
}

/// Whether dependent cleanup and execution-slot release are safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxErrorKind {
    /// Execution never started, or Isolate confirmed that its tree stopped.
    Execution,
    /// The manager did not establish that the execution tree stopped.
    StopUnconfirmed,
}

/// Stage failure with enough classification for P04 cleanup and P05 admission.
#[derive(Debug)]
pub struct SandboxError {
    /// Determines whether the caller may release execution resources.
    pub kind: SandboxErrorKind,
    /// Operation and underlying failure context.
    pub source: anyhow::Error,
    /// Original internal stop cause, including when confirmation failed.
    pub stop_reason: Option<StopReason>,
    /// Private metadata directory retained when stopping could not be confirmed.
    pub retained_workspace: Option<PathBuf>,
}

impl SandboxError {
    /// Preserve the original request alongside the infrastructure failure.
    pub(super) fn with_stop(mut self, reason: Option<StopReason>) -> Self {
        self.stop_reason = reason;
        self
    }
}

impl std::fmt::Display for SandboxError {
    /// Include the complete operation context in English diagnostics.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {:#}", self.kind, self.source)
    }
}

impl std::error::Error for SandboxError {
    /// Expose the underlying infrastructure failure.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}
