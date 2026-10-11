//! Isolate lifecycle and bounded asynchronous I/O for one stage at a time.

use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use nix::{
    errno::Errno,
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    time::{Instant, sleep_until, timeout},
};

use crate::config::SandboxConfig;

use super::{
    SandboxError, SandboxErrorKind, SandboxRunOutcome, SandboxRunSpec, StageControl, StopReason,
    metadata::Metadata, output::OutputCollector,
};

/// Serial stage backend; a stop failure permanently retains this instance's slot.
///
/// Callers must await `run` to completion and request stops through `StageControl`,
/// rather than aborting its future. Cancellation during execution also leaves the
/// instance occupied and private files retained; there is no automatic recovery.
pub struct IsolateSandbox {
    /// Deployment-owned isolation and auxiliary-operation budgets.
    config: SandboxConfig,
    /// Remains true until a launched manager establishes that execution stopped.
    occupied: bool,
    /// Keep an unconfirmed manager handle alive without sending SIGKILL on drop.
    retained_manager: Option<Child>,
}

impl IsolateSandbox {
    /// Construct the backend for the Worker's dedicated box ID.
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            occupied: false,
            retained_manager: None,
        }
    }

    /// Whether this instance can start another stage after the previous result.
    pub fn is_available(&self) -> bool {
        !self.occupied
    }

    /// Initialize, copy, execute, confirm stopping, export and attempt cleanup.
    pub async fn run(
        &mut self,
        spec: SandboxRunSpec,
        mut control: StageControl,
    ) -> std::result::Result<SandboxRunOutcome, SandboxError> {
        if self.occupied {
            return Err(SandboxError {
                kind: SandboxErrorKind::StopUnconfirmed,
                source: anyhow::anyhow!(
                    "Isolate execution slot is retained after an unconfirmed stop"
                ),
                retained_workspace: None,
                stop_reason: None,
            });
        }
        validate(&spec, &self.config).map_err(execution_error)?;
        // Explicit removal below is essential: dropping a future must not delete
        // files that an unconfirmed execution may still be using.
        let workspace = tempfile::Builder::new()
            .prefix("acorn-stage-")
            .tempdir()
            .context("Create private stage metadata directory")
            .map_err(execution_error)?
            .keep();
        let mut init = self.command("--init");
        init.arg(format!(
            "--quota={},{}",
            self.config.disk_quota_kib, self.config.disk_quota_inodes
        ));
        init.kill_on_drop(true);
        let mut early_stop = None;
        let initialized = tokio::select! {
            output = init.output() => output.context("Start Isolate initialization").and_then(|output| {
                ensure!(output.status.success(), "Isolate initialization failed: {}", String::from_utf8_lossy(&output.stderr));
                let root = PathBuf::from(String::from_utf8(output.stdout).context("Invalid Isolate initialization path")?.trim());
                ensure!(root.is_absolute(), "Isolate initialization returned a nonabsolute path");
                Ok(root)
            }),
            reason = control.wait_stop() => {
                early_stop = Some(reason);
                Err(anyhow::anyhow!("Stage stopped during initialization: {reason:?}"))
            },
        };
        let result = match initialized {
            Ok(root) => {
                self.run_initialized(&root, &workspace, &spec, &mut control)
                    .await
            }
            Err(error) => Err(execution_error(error).with_stop(early_stop)),
        };
        if result
            .as_ref()
            .is_err_and(|error| error.kind == SandboxErrorKind::StopUnconfirmed)
        {
            return result.map_err(|mut error| {
                error.retained_workspace = Some(workspace);
                error
            });
        }
        self.cleanup().await;
        if let Err(error) = tokio::fs::remove_dir_all(&workspace).await {
            tracing::warn!(path = %workspace.display(), %error, "Failed to remove stopped stage metadata");
        }
        result
    }

    /// Configure all Isolate operations with identical box ownership and cgroup mode.
    fn command(&self, operation: &str) -> Command {
        let mut command = Command::new(&self.config.executable);
        command.args([
            "--cg",
            &format!("--box-id={}", self.config.box_id),
            operation,
        ]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// Prepare private copies and run only after successful quota initialization.
    async fn run_initialized(
        &mut self,
        root: &Path,
        workspace: &Path,
        spec: &SandboxRunSpec,
        control: &mut StageControl,
    ) -> std::result::Result<SandboxRunOutcome, SandboxError> {
        let work = root.join("box");
        for file in &spec.files {
            let path = work.join(&file.name);
            tokio::fs::write(&path, &file.bytes)
                .await
                .context("Copy stage input file")
                .map_err(execution_error)?;
            tokio::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(if file.executable { 0o700 } else { 0o600 }),
            )
            .await
            .context("Set stage input permissions")
            .map_err(execution_error)?;
        }
        if let Some(reason) = control.current_stop() {
            return Err(execution_error(anyhow::anyhow!(
                "Stage stopped before execution: {reason:?}"
            ))
            .with_stop(Some(reason)));
        }
        let meta_path = workspace.join("metadata");
        let mut command = self.command("--run");
        command.args([
            "--silent".to_owned(),
            format!("--meta={}", meta_path.display()),
            format!("--time={}", seconds(spec.limits.cpu_time_ms)),
            format!("--wall-time={}", seconds(spec.limits.wall_time_ms)),
            format!("--cg-mem={}", spec.limits.memory_bytes / 1024),
            format!("--processes={}", spec.limits.process_max_count),
            format!("--open-files={}", self.config.fd_max_count),
            format!("--fsize={}", self.config.file_size_max_bytes / 1024),
            "--dir=/dev/shm:tmp".to_owned(),
        ]);
        for (key, value) in &spec.command.environment {
            command.arg(format!("--env={key}={value}"));
        }
        command
            .arg("--")
            .arg(&spec.command.executable)
            .args(&spec.command.arguments)
            .stdin(Stdio::piped());
        let mut child = command
            .spawn()
            .context("Start Isolate stage manager")
            .map_err(execution_error)?;
        self.occupied = true;
        let collected = collect(&mut child, spec, control, self.config.stop_timeout_ms).await;
        let (output, stop_reason) = match collected {
            Ok(value) => {
                self.occupied = false;
                value
            }
            Err(error) => {
                if error.kind == SandboxErrorKind::StopUnconfirmed {
                    self.retained_manager = Some(child);
                } else {
                    self.occupied = false;
                }
                return Err(error);
            }
        };
        let meta_text = tokio::fs::read_to_string(meta_path)
            .await
            .context("Read Isolate execution metadata")
            .map_err(execution_error)?;
        let metadata = Metadata::parse(&meta_text).map_err(execution_error)?;
        let success = metadata.exit_code == Some(0)
            && !metadata.oom
            && !metadata.timeout
            && !output.exceeded
            && stop_reason.is_none();
        let artifact = if success {
            spec.artifact
                .as_ref()
                .map(|artifact| export(&work.join(&artifact.name), artifact.max_bytes))
                .transpose()
                .map_err(execution_error)?
        } else {
            None
        };
        Ok(SandboxRunOutcome {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: metadata.exit_code,
            exit_signal: metadata.exit_signal,
            memory_limit_exceeded: metadata.oom,
            time_limit_exceeded: metadata.timeout,
            output_limit_exceeded: output.exceeded,
            stop_reason,
            usage: metadata.usage,
            artifact,
        })
    }

    /// Cleanup failures after a safe stop are warnings and do not change the result.
    async fn cleanup(&self) {
        let mut command = self.command("--cleanup");
        command.kill_on_drop(true);
        match timeout(
            Duration::from_millis(self.config.cleanup_timeout_ms),
            command.output(),
        )
        .await
        {
            Ok(Ok(output)) if output.status.success() => (),
            Ok(Ok(output)) => {
                tracing::warn!(box_id = self.config.box_id, status = %output.status, diagnostic = %String::from_utf8_lossy(&output.stderr), "Isolate cleanup failed")
            }
            Ok(Err(error)) => {
                tracing::warn!(box_id = self.config.box_id, %error, "Failed to start Isolate cleanup")
            }
            Err(_) => tracing::warn!(box_id = self.config.box_id, "Isolate cleanup timed out"),
        }
    }
}

impl StageControl {
    /// Check an already requested stop before starting an execution process.
    fn current_stop(&self) -> Option<StopReason> {
        (*self.stop.borrow())
            .or_else(|| (Instant::now() >= self.deadline).then_some(StopReason::JobDeadline))
    }

    /// Wait for an internal request or Job deadline, without spinning on a closed channel.
    async fn wait_stop(&mut self) -> StopReason {
        loop {
            if let Some(reason) = self.current_stop() {
                return reason;
            }
            tokio::select! {
                _ = sleep_until(self.deadline) => return StopReason::JobDeadline,
                changed = self.stop.changed() => {
                    if changed.is_err() {
                        sleep_until(self.deadline).await;
                        return StopReason::JobDeadline;
                    }
                }
            }
        }
    }
}

/// Drain both program streams while waiting asynchronously for manager completion.
async fn collect(
    child: &mut Child,
    spec: &SandboxRunSpec,
    control: &mut StageControl,
    stop_timeout_ms: u64,
) -> std::result::Result<(OutputCollector, Option<StopReason>), SandboxError> {
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let write_input = async {
        let written = stdin.write_all(&spec.stdin).await;
        drop(stdin);
        match written {
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            result => result,
        }
    };
    tokio::pin!(write_input);
    let mut input_done = false;
    let mut stdout_done = false;
    let mut stderr_done = false;
    let mut stdout_buffer = [0; 8192];
    let mut stderr_buffer = [0; 8192];
    let mut output = OutputCollector::new(spec.output);
    let mut status: Option<ExitStatus> = None;
    let mut stop_reason = None;
    let mut stop_deadline = None;
    let mut io_error = None;
    loop {
        if status.is_some() && stdout_done && stderr_done {
            break;
        }
        let stopping_at = stop_deadline.unwrap_or(control.deadline);
        tokio::select! {
            waited = child.wait(), if status.is_none() => {
                let waited = waited.context("Wait for Isolate manager").map_err(|error| stop_error(error).with_stop(stop_reason))?;
                if !matches!(waited.code(), Some(0 | 1)) {
                    return Err(stop_error(anyhow::anyhow!("Isolate manager exited without confirming stop: {waited}; retained stderr: {}", String::from_utf8_lossy(&output.stderr.bytes))).with_stop(stop_reason));
                }
                status = Some(waited);
                // Program input no longer needs to be written after a confirmed exit.
                input_done = true;
            }
            read = stdout.read(&mut stdout_buffer), if !stdout_done => match read {
                Ok(0) => stdout_done = true,
                Ok(length) => output.append(false, &stdout_buffer[..length]),
                Err(error) => { stdout_done = true; io_error = Some(anyhow::Error::new(error).context("Collect stage stdout")); }
            },
            read = stderr.read(&mut stderr_buffer), if !stderr_done => match read {
                Ok(0) => stderr_done = true,
                Ok(length) => output.append(true, &stderr_buffer[..length]),
                Err(error) => { stderr_done = true; io_error = Some(anyhow::Error::new(error).context("Collect stage stderr")); }
            },
            written = &mut write_input, if !input_done => {
                input_done = true;
                if let Err(error) = written { io_error = Some(anyhow::Error::new(error).context("Write stage stdin")); }
            }
            reason = control.wait_stop(), if stop_reason.is_none() && status.is_none() => stop_reason = Some(reason),
            _ = sleep_until(stopping_at), if stop_deadline.is_some() && status.is_none() => {
                return Err(stop_error(anyhow::anyhow!("Timed out waiting for Isolate to confirm stop" )).with_stop(stop_reason));
            }
        }
        if stop_reason.is_none() {
            if output.exceeded {
                stop_reason = Some(StopReason::OutputLimit);
            } else if io_error.is_some() {
                stop_reason = Some(StopReason::Infrastructure);
            }
        }
        if stop_reason.is_some() && stop_deadline.is_none() && status.is_none() {
            let pid = child
                .id()
                .context("Missing active Isolate manager PID")
                .map_err(|error| stop_error(error).with_stop(stop_reason))?;
            match kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
                Ok(()) | Err(Errno::ESRCH) => (), // A concurrent exit is resolved by wait(), never by this signal result.
                Err(error) => {
                    return Err(stop_error(
                        anyhow::Error::new(error).context("Signal Isolate manager with SIGTERM"),
                    )
                    .with_stop(stop_reason));
                }
            }
            stop_deadline = Some(Instant::now() + Duration::from_millis(stop_timeout_ms));
        }
    }
    if let Some(error) = io_error {
        return Err(execution_error(error).with_stop(stop_reason));
    }
    Ok((output, stop_reason))
}

/// Validate trusted stage names and Isolate's nonzero, signed-integer resource units.
fn validate(spec: &SandboxRunSpec, config: &SandboxConfig) -> Result<()> {
    for file in &spec.files {
        filename(&file.name)?;
    }
    if let Some(artifact) = &spec.artifact {
        filename(&artifact.name)?;
        ensure!(artifact.max_bytes > 0, "Artifact budget must be positive");
    }
    for (name, value) in [
        ("CPU milliseconds", spec.limits.cpu_time_ms),
        ("wall milliseconds", spec.limits.wall_time_ms),
        ("memory KiB", spec.limits.memory_bytes / 1024),
        ("process count", u64::from(spec.limits.process_max_count)),
        ("file size KiB", config.file_size_max_bytes / 1024),
        ("FD count", u64::from(config.fd_max_count)),
        ("disk quota KiB", u64::from(config.disk_quota_kib)),
        ("inode quota", u64::from(config.disk_quota_inodes)),
    ] {
        ensure!(
            value > 0 && value <= i32::MAX as u64,
            "Stage {name} is outside Isolate's supported range"
        );
    }
    Ok(())
}

/// Require a single ordinary name, excluding path traversal and absolute paths.
fn filename(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "Stage file must have a single relative filename"
    );
    Ok(())
}

/// Serialize millisecond budgets into Isolate's fractional seconds.
fn seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

/// Open a fixed artifact without following symlinks, rejecting hardlinks and special files.
fn export(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)
        .context("Open compiler artifact")?;
    let metadata = file.metadata().context("Inspect compiler artifact")?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "Compiler artifact must be a regular file without links"
    );
    ensure!(
        metadata.len() <= max_bytes as u64,
        "Compiler artifact exceeds its size budget"
    );
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .context("Copy compiler artifact")?;
    ensure!(
        bytes.len() <= max_bytes,
        "Compiler artifact exceeds its size budget"
    );
    Ok(bytes)
}

/// Construct an error where ordinary cleanup and slot release are safe.
fn execution_error(source: anyhow::Error) -> SandboxError {
    SandboxError {
        kind: SandboxErrorKind::Execution,
        source,
        stop_reason: None,
        retained_workspace: None,
    }
}

/// Construct an error where dependent resources and the execution slot must be retained.
fn stop_error(source: anyhow::Error) -> SandboxError {
    SandboxError {
        kind: SandboxErrorKind::StopUnconfirmed,
        source,
        stop_reason: None,
        retained_workspace: None,
    }
}

#[cfg(test)]
mod tests {
    //! Regression coverage for the fixed artifact export boundary.

    use std::{fs, os::unix::fs::symlink};

    use super::export;

    /// Only an independent, regular artifact within its budget may leave the stage.
    #[test]
    fn artifact_export_rejects_links_special_files_and_oversize() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("program");
        fs::write(&program, b"program").unwrap();
        assert_eq!(export(&program, 7).unwrap(), b"program");
        assert!(export(&program, 6).is_err());
        let link = directory.path().join("symlink");
        symlink(&program, &link).unwrap();
        assert!(export(&link, 7).is_err());
        let hardlink = directory.path().join("hardlink");
        fs::hard_link(&program, &hardlink).unwrap();
        assert!(export(&program, 7).is_err());
        assert!(export(directory.path(), 7).is_err());
        let fifo = directory.path().join("fifo");
        nix::unistd::mkfifo(&fifo, nix::sys::stat::Mode::S_IRUSR).unwrap();
        assert!(export(&fifo, 7).is_err());
    }
}
