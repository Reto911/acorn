//! Shared bounded output retention for compilation and runtime collection.

use super::{CapturedOutput, OutputPolicy};

/// Collection state shared by stdout and stderr in the stage's I/O loop.
pub(super) struct OutputCollector {
    /// Combined compilation or independent runtime budgets.
    policy: OutputPolicy,
    /// Retained standard output prefix.
    pub stdout: CapturedOutput,
    /// Retained standard error prefix.
    pub stderr: CapturedOutput,
    /// Whether an independent runtime budget was exceeded.
    pub exceeded: bool,
}

impl OutputCollector {
    /// Start collection without allocating the entire configured budget eagerly.
    pub fn new(policy: OutputPolicy) -> Self {
        Self {
            policy,
            stdout: CapturedOutput::default(),
            stderr: CapturedOutput::default(),
            exceeded: false,
        }
    }

    /// Retain a bounded prefix; return overflow only for runtime streams.
    pub fn append(&mut self, stderr: bool, bytes: &[u8]) {
        let remaining = match self.policy {
            OutputPolicy::Compile { max_bytes } => {
                max_bytes.saturating_sub(self.stdout.bytes.len() + self.stderr.bytes.len())
            }
            OutputPolicy::Run {
                stdout_max_bytes,
                stderr_max_bytes,
            } => {
                if stderr {
                    stderr_max_bytes.saturating_sub(self.stderr.bytes.len())
                } else {
                    stdout_max_bytes.saturating_sub(self.stdout.bytes.len())
                }
            }
        };
        let output = if stderr {
            &mut self.stderr
        } else {
            &mut self.stdout
        };
        let keep = bytes.len().min(remaining);
        output.bytes.extend_from_slice(&bytes[..keep]);
        output.truncated |= keep < bytes.len();
        self.exceeded |= matches!(self.policy, OutputPolicy::Run { .. }) && output.truncated;
    }
}
