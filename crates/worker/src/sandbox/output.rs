//! Shared bounded output retention for compilation, user execution and checking.

use super::{CapturedOutput, OutputPolicy};

/// Collection state shared by stdout and stderr in the stage's I/O loop.
pub(super) struct OutputCollector {
    /// Stage-specific retention and overflow behavior.
    policy: OutputPolicy,
    /// Retained standard output prefix.
    pub stdout: CapturedOutput,
    /// Retained standard error prefix.
    pub stderr: CapturedOutput,
    /// Whether a stream whose overflow stops execution exceeded its budget.
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

    /// Retain a bounded prefix; checker stderr overflow only marks truncation.
    pub fn append(&mut self, stderr: bool, bytes: &[u8]) {
        let remaining = match self.policy {
            OutputPolicy::Compile { max_bytes } => {
                max_bytes.saturating_sub(self.stdout.bytes.len() + self.stderr.bytes.len())
            }
            OutputPolicy::Run {
                stdout_max_bytes,
                stderr_max_bytes,
            }
            | OutputPolicy::Checker {
                result_max_bytes: stdout_max_bytes,
                diagnostic_max_bytes: stderr_max_bytes,
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
        self.exceeded |= output.truncated
            && match self.policy {
                OutputPolicy::Compile { .. } => false,
                OutputPolicy::Run { .. } => true,
                OutputPolicy::Checker { .. } => !stderr,
            };
    }
}

#[cfg(test)]
mod tests {
    //! Checker stderr truncation and complete-result overflow have different outcomes.

    use super::*;

    /// Drain diagnostic overflow while requiring all result bytes to fit their own budget.
    #[test]
    fn checker_output_budgets() {
        let mut output = OutputCollector::new(OutputPolicy::Checker {
            result_max_bytes: 4,
            diagnostic_max_bytes: 3,
        });
        output.append(true, b"private long diagnostics");
        output.append(false, b"1234");
        assert_eq!(output.stderr.bytes, b"pri");
        assert!(output.stderr.truncated);
        assert!(!output.exceeded && !output.stdout.truncated);
        output.append(false, b"5");
        assert!(output.exceeded && output.stdout.truncated);
        assert_eq!(output.stdout.bytes, b"1234");
    }
}
