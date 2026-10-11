//! Serial judging over complete sandbox stages, with one common Job cleanup path.

use std::path::PathBuf;

use acorn_protocol::worker::v1::{
    FailedCase, JobFailure, JobPhase, JudgeResult, ResourceUsage, SubmitJobRequest, Verdict,
    failed_case::Exit, get_job_reply::Terminal,
};
use anyhow::Context;
use tracing::Instrument;

use crate::{
    config::Config,
    language::{self, LanguagePlan, ProgramFiles, StageLimits},
    sandbox::{
        ArtifactSpec, IsolateSandbox, OutputPolicy, SandboxError, SandboxErrorKind, SandboxFile,
        SandboxRunOutcome, SandboxRunSpec, StageControl, StopReason,
    },
};

/// Failure text budget within the response envelope reserved by configuration.
const FAILURE_DIAGNOSTIC_BYTES: usize = 512;

/// Observable execution progress, independent of any transport connection.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionProgress {
    /// Current preparation, compilation, execution or checking phase.
    pub phase: JobPhase,
    /// Cases already adjudicated, including the first failed case.
    pub completed_cases: u32,
}

/// Bounded terminal content returned only after the Job's cleanup decision.
#[derive(Debug)]
pub struct ExecutionOutcome {
    /// A judging result or an execution failure, never both.
    pub terminal: Terminal,
    /// Number of cases adjudicated before this terminal outcome.
    pub completed_cases: u32,
}

/// Original files and request data kept until execution is safely stopped.
struct JobContext {
    /// Server-generated private path, unrelated to the caller's Job ID.
    directory: PathBuf,
    /// Owned source, ordered inputs, answers and case budgets.
    request: SubmitJobRequest,
    /// Original compiler artifact, copied independently into each case sandbox.
    artifact: Option<Vec<u8>>,
}

/// Common C/Python executor retaining resources after an unconfirmed stop.
pub struct Executor {
    /// Validated deployment settings and output budgets.
    config: Config,
    /// One serial backend for this Worker's dedicated box ID.
    sandbox: IsolateSandbox,
    /// Resources that must not be dropped or cleaned while stopping is unconfirmed.
    retained_job: Option<JobContext>,
}

impl Executor {
    /// Construct the executor without initializing a sandbox or running user code.
    pub fn new(config: Config) -> Self {
        Self {
            sandbox: IsolateSandbox::new(config.sandbox.clone()),
            config,
            retained_job: None,
        }
    }

    /// Whether another Job can safely use this executor.
    pub fn is_available(&self) -> bool {
        self.retained_job.is_none() && self.sandbox.is_available()
    }

    /// Execute an admitted Job; callers must await completion and stop via the channel.
    pub async fn run(
        &mut self,
        request: SubmitJobRequest,
        control: StageControl,
        mut progress: impl FnMut(ExecutionProgress) + Send,
    ) -> ExecutionOutcome {
        let started = tokio::time::Instant::now();
        let job_id = request.job_id.clone();
        let language_id = request.language_id.clone();
        let mut current = ExecutionProgress {
            phase: JobPhase::Preparing,
            completed_cases: 0,
        };
        progress(current);
        if !self.is_available() {
            return terminal_failure("StopUnconfirmed", "Execution slot remains occupied", 0);
        }
        let directory = match tempfile::Builder::new().prefix("acorn-job-").tempdir() {
            Ok(directory) => directory.keep(),
            Err(error) => return terminal_failure("PreparationFailed", &error.to_string(), 0),
        };
        let mut context = JobContext {
            directory,
            request,
            artifact: None,
        };
        let result = self
            .execute(&mut context, &control, &mut current, &mut progress)
            .instrument(tracing::info_span!("job", job = %job_id, language = %language_id))
            .await;
        if self.sandbox.is_available() {
            if let Err(error) = tokio::fs::remove_dir_all(&context.directory).await {
                tracing::warn!(job = %job_id, %error, "Failed to remove stopped Job workspace; continuing");
            }
            drop(context);
        } else {
            self.retained_job = Some(context);
        }
        let terminal = match result {
            Ok(result) => Terminal::Result(result),
            Err(failure) => Terminal::Failure(failure),
        };
        current.phase = JobPhase::Done;
        progress(current);
        match &terminal {
            Terminal::Result(result) => {
                tracing::info!(job = %job_id, language = %language_id, phase = "done", verdict = ?Verdict::try_from(result.verdict), elapsed_ms = started.elapsed().as_millis(), "Judging finished")
            }
            Terminal::Failure(failure) => {
                tracing::error!(job = %job_id, language = %language_id, phase = "done", code = %failure.code, diagnostic = %failure.diagnostic, elapsed_ms = started.elapsed().as_millis(), "Job execution failed")
            }
        }
        ExecutionOutcome {
            terminal,
            completed_cases: current.completed_cases,
        }
    }

    /// Prepare once, then run and adjudicate each case before considering the next.
    async fn execute(
        &mut self,
        context: &mut JobContext,
        control: &StageControl,
        current: &mut ExecutionProgress,
        progress: &mut (impl FnMut(ExecutionProgress) + Send),
    ) -> std::result::Result<JudgeResult, JobFailure> {
        check_stop(control)?;
        let plan = language::plan(&context.request.language_id, &self.config)
            .map_err(preparation_failure)?;
        tokio::fs::write(
            context.directory.join(&plan.source_name),
            &context.request.source,
        )
        .await
        .context("Save private Job source")
        .map_err(preparation_failure)?;
        if let Some(compile) = &plan.compile {
            check_stop(control)?;
            current.phase = JobPhase::Compiling;
            progress(*current);
            tracing::info!(job = %context.request.job_id, language = %context.request.language_id, phase = "compiling", "Compilation started");
            let outcome = self
                .sandbox
                .run(
                    SandboxRunSpec {
                        command: compile.command.clone(),
                        files: vec![SandboxFile {
                            name: plan.source_name.clone(),
                            bytes: context.request.source.clone(),
                            executable: false,
                        }],
                        stdin: vec![],
                        limits: compile.limits.clone(),
                        output: OutputPolicy::Compile {
                            max_bytes: self.config.compile.diagnostic_max_bytes,
                        },
                        artifact: Some(ArtifactSpec {
                            name: compile.artifact_name.clone(),
                            max_bytes: self.config.compile.artifact_max_bytes,
                        }),
                    },
                    control.clone(),
                )
                .await
                .map_err(sandbox_failure)?;
            check_outcome_stop(&outcome, control)?;
            tracing::info!(job = %context.request.job_id, language = %context.request.language_id, phase = "compiling", "Compilation finished");
            if !successful(&outcome) {
                let truncated = outcome.stdout.truncated || outcome.stderr.truncated;
                let mut diagnostic = outcome.stdout.bytes;
                diagnostic.extend(outcome.stderr.bytes);
                return Ok(JudgeResult {
                    verdict: Verdict::CompileError as i32,
                    compile_diagnostic: diagnostic,
                    compile_diagnostic_truncated: truncated,
                    failed_case: None,
                });
            }
            let artifact = outcome.artifact.ok_or_else(|| {
                failure(
                    "SandboxError",
                    "Successful compilation returned no artifact",
                )
            })?;
            tokio::fs::write(context.directory.join(&compile.artifact_name), &artifact)
                .await
                .context("Save private compiler artifact")
                .map_err(preparation_failure)?;
            context.artifact = Some(artifact);
        }
        let limits = context
            .request
            .run_limits
            .as_ref()
            .ok_or_else(|| failure("InvalidJob", "Missing run limits"))?;
        let limits = StageLimits {
            cpu_time_ms: limits.cpu_time_ms,
            wall_time_ms: limits.wall_time_ms,
            memory_bytes: limits.memory_bytes,
            process_max_count: self.config.execution.run_process_max_count,
        };
        for (index, case) in context.request.cases.iter().enumerate() {
            check_stop(control)?;
            current.phase = JobPhase::RunningCase;
            progress(*current);
            tracing::info!(job = %context.request.job_id, language = %context.request.language_id, phase = "running_case", case = index + 1, "Case started");
            let outcome = self
                .sandbox
                .run(
                    SandboxRunSpec {
                        command: plan.run.clone(),
                        files: vec![program_file(&plan, context)],
                        stdin: case.input.clone(),
                        limits: limits.clone(),
                        output: OutputPolicy::Run {
                            stdout_max_bytes: self.config.execution.stdout_max_bytes,
                            stderr_max_bytes: self.config.execution.stderr_max_bytes,
                        },
                        artifact: None,
                    },
                    control.clone(),
                )
                .await
                .map_err(sandbox_failure)?;
            check_outcome_stop(&outcome, control)?;
            current.phase = JobPhase::Checking;
            progress(*current);
            let verdict = verdict(&outcome, &case.expected_output);
            check_stop(control)?;
            current.completed_cases += 1;
            progress(*current);
            tracing::info!(job = %context.request.job_id, language = %context.request.language_id, phase = "checking", case = index + 1, ?verdict, "Case adjudicated");
            if verdict != Verdict::Accepted {
                return Ok(JudgeResult {
                    verdict: verdict as i32,
                    failed_case: Some(failed_case(index as u32 + 1, outcome)),
                    ..Default::default()
                });
            }
        }
        check_stop(control)?;
        Ok(JudgeResult {
            verdict: Verdict::Accepted as i32,
            ..Default::default()
        })
    }
}

/// Copy the original program independently; a previous case cannot modify this source.
fn program_file(plan: &LanguagePlan, context: &JobContext) -> SandboxFile {
    match plan.program_files {
        ProgramFiles::Source => SandboxFile {
            name: plan.source_name.clone(),
            bytes: context.request.source.clone(),
            executable: false,
        },
        ProgramFiles::CompiledArtifact => SandboxFile {
            name: plan
                .compile
                .as_ref()
                .expect("compiled plan")
                .artifact_name
                .clone(),
            bytes: context
                .artifact
                .as_ref()
                .expect("successful compilation")
                .clone(),
            executable: true,
        },
    }
}

/// A normal zero exit without any confirmed resource limit is successful.
fn successful(outcome: &SandboxRunOutcome) -> bool {
    outcome.exit_code == Some(0)
        && outcome.exit_signal.is_none()
        && !outcome.memory_limit_exceeded
        && !outcome.output_limit_exceeded
        && !outcome.time_limit_exceeded
}

/// Select confirmed limits in MLE, OLE, TLE order before exit and output comparison.
fn verdict(outcome: &SandboxRunOutcome, expected: &[u8]) -> Verdict {
    if outcome.memory_limit_exceeded {
        Verdict::MemoryLimitExceeded
    } else if outcome.output_limit_exceeded {
        Verdict::OutputLimitExceeded
    } else if outcome.time_limit_exceeded {
        Verdict::TimeLimitExceeded
    } else if !successful(outcome) {
        Verdict::RuntimeError
    } else if trim(&outcome.stdout.bytes) != trim(expected) {
        Verdict::WrongAnswer
    } else {
        Verdict::Accepted
    }
}

/// Remove only HT, LF, VT, FF, CR and SPACE at the end of the entire byte string.
fn trim(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' '))
        .map_or(0, |index| index + 1);
    &bytes[..end]
}

/// Preserve available facts without adding unknown statistics or placeholder cases.
fn failed_case(index: u32, outcome: SandboxRunOutcome) -> FailedCase {
    let exit = outcome
        .exit_signal
        .map(|signal| Exit::Signal(signal as u32))
        .or_else(|| outcome.exit_code.map(|code| Exit::ExitCode(code as u32)));
    FailedCase {
        case_index: index,
        stdout: outcome.stdout.bytes,
        stderr: outcome.stderr.bytes,
        stdout_truncated: outcome.stdout.truncated,
        stderr_truncated: outcome.stderr.truncated,
        resources: Some(ResourceUsage {
            cpu_time_us: outcome.usage.cpu_time_us,
            wall_time_us: outcome.usage.wall_time_us,
            peak_memory_bytes: outcome.usage.memory_bytes,
        }),
        exit,
    }
}

/// Respect an already requested shutdown or the absolute admission deadline.
fn check_stop(control: &StageControl) -> std::result::Result<(), JobFailure> {
    match control.current_stop() {
        None | Some(StopReason::OutputLimit) => Ok(()),
        Some(reason) => Err(stop_failure(reason)),
    }
}

/// Preserve internal stop causes instead of treating the manager's SIGTERM as RE.
fn check_outcome_stop(
    outcome: &SandboxRunOutcome,
    control: &StageControl,
) -> std::result::Result<(), JobFailure> {
    if let Some(reason) = outcome
        .stop_reason
        .filter(|reason| *reason != StopReason::OutputLimit)
    {
        return Err(stop_failure(reason));
    }
    check_stop(control)
}

/// Turn a safe stage error or an unconfirmed stop into an explicit bounded failure.
fn sandbox_failure(error: SandboxError) -> JobFailure {
    if error.kind == SandboxErrorKind::StopUnconfirmed {
        failure("StopUnconfirmed", &error.to_string())
    } else if let Some(reason) = error.stop_reason {
        stop_failure(reason)
    } else {
        failure("SandboxError", &error.to_string())
    }
}

/// Map internal reasons to stable execution-failure codes.
fn stop_failure(reason: StopReason) -> JobFailure {
    match reason {
        StopReason::JobDeadline => {
            failure("JobDeadlineExceeded", "Job wall-clock deadline exceeded")
        }
        StopReason::Shutdown => failure("WorkerShutdown", "Worker is shutting down"),
        StopReason::Infrastructure | StopReason::OutputLimit => failure(
            "SandboxError",
            "Stage stopped after an infrastructure failure",
        ),
    }
}

/// Preserve preparation error context without returning an unbounded message.
fn preparation_failure(error: anyhow::Error) -> JobFailure {
    failure("PreparationFailed", &format!("{error:#}"))
}

/// Bound UTF-8 diagnostics while retaining a valid string and stable failure code.
fn failure(code: &str, diagnostic: &str) -> JobFailure {
    let mut end = diagnostic.len().min(FAILURE_DIAGNOSTIC_BYTES);
    while !diagnostic.is_char_boundary(end) {
        end -= 1;
    }
    JobFailure {
        code: code.into(),
        diagnostic: diagnostic[..end].into(),
    }
}

/// Construct failures that occur before a private context can be created.
fn terminal_failure(code: &str, diagnostic: &str, completed_cases: u32) -> ExecutionOutcome {
    ExecutionOutcome {
        terminal: Terminal::Failure(failure(code, diagnostic)),
        completed_cases,
    }
}
