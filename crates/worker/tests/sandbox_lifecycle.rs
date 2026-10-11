//! Controlled manager failures complement the separately invoked real Isolate checks.

use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

use acorn_worker::{
    config::Config,
    language::{CommandPlan, StageLimits},
    sandbox::{IsolateSandbox, OutputPolicy, SandboxErrorKind, SandboxRunSpec, StageControl},
};
use tokio::{sync::watch, time::Instant};

/// Create a manager substitute that records operations and injects one chosen failure.
fn fixture(mode: &str) -> (tempfile::TempDir, IsolateSandbox) {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("manager");
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/python3
import pathlib, signal, sys, time
root = pathlib.Path({root:?})
args = sys.argv[1:]
with (root / 'operations').open('a') as log:
    log.write(' '.join(args) + '\n')
mode = {mode:?}
if '--init' in args:
    if mode == 'quota_failure':
        print('Cannot set disk quota', file=sys.stderr)
        sys.exit(2)
    (root / 'box').mkdir()
    print(root)
elif '--run' in args:
    if mode == 'manager_failure':
        sys.exit(2)
    meta = pathlib.Path(next(a[7:] for a in args if a.startswith('--meta=')))
    if mode == 'internal_stop':
        stopped = False
        def stop(signum, frame):
            global stopped
            stopped = True
        signal.signal(signal.SIGTERM, stop)
        (root / 'ready').touch()
        while not stopped:
            time.sleep(0.01)
        meta.write_text('status:SG\nexitsig:15\n')
        sys.exit(1)
    meta.write_text('exitcode:0\n')
    sys.stdout.write('x' * 100000)
    sys.stdout.flush()
    sys.stderr.write('y' * 100000)
    sys.stderr.flush()
elif '--cleanup' in args:
    (root / 'cleaned').touch()
    if mode == 'cleanup_failure':
        sys.exit(2)
"#,
            root = directory.path().to_str().unwrap()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let mut config = Config::parse(include_str!("../../../config/worker.example.toml")).unwrap();
    config.sandbox.executable = script;
    (directory, IsolateSandbox::new(config.sandbox))
}

/// Minimal trusted stage for lifecycle checks, with compilation's shared output cap.
fn spec() -> SandboxRunSpec {
    SandboxRunSpec {
        command: CommandPlan {
            executable: "/usr/bin/true".into(),
            arguments: vec![],
            environment: Default::default(),
        },
        files: vec![],
        stdin: vec![],
        limits: StageLimits {
            cpu_time_ms: 1000,
            wall_time_ms: 2000,
            memory_bytes: 16 * 1024 * 1024,
            process_max_count: 1,
        },
        output: OutputPolicy::Compile { max_bytes: 1024 },
        artifact: None,
    }
}

/// A live channel and future deadline represent an admitted Job.
fn control() -> (
    watch::Sender<Option<acorn_worker::sandbox::StopReason>>,
    StageControl,
) {
    let (sender, stop) = watch::channel(None);
    (
        sender,
        StageControl {
            deadline: Instant::now() + Duration::from_secs(10),
            stop,
        },
    )
}

/// Compilation drains both streams while retaining a combined prefix; cleanup failure is harmless.
#[tokio::test]
async fn bounded_diagnostics_and_cleanup_warning_preserve_result() {
    let (directory, mut sandbox) = fixture("cleanup_failure");
    let (_sender, control) = control();
    let outcome = sandbox.run(spec(), control).await.unwrap();
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        outcome.stdout.bytes.len() + outcome.stderr.bytes.len(),
        1024
    );
    assert!(outcome.stdout.truncated && outcome.stderr.truncated);
    assert!(!outcome.output_limit_exceeded);
    assert!(outcome.usage.cpu_time_us.is_none());
    assert!(directory.path().join("cleaned").exists());
    assert!(sandbox.is_available());
    let operations = fs::read_to_string(directory.path().join("operations")).unwrap();
    for line in operations.lines() {
        assert!(line.starts_with("--cg --box-id=0 "));
    }
    assert!(operations.contains("--quota=65536,4096"));
    assert!(operations.contains("--dir=/dev/shm:tmp"));
}

/// A failed quota initialization cannot reach the stage execution operation.
#[tokio::test]
async fn quota_failure_does_not_execute_program() {
    let (directory, mut sandbox) = fixture("quota_failure");
    let (_sender, control) = control();
    let error = sandbox.run(spec(), control).await.unwrap_err();
    assert_eq!(error.kind, SandboxErrorKind::Execution);
    assert!(error.to_string().contains("Cannot set disk quota"));
    let operations = fs::read_to_string(directory.path().join("operations")).unwrap();
    assert!(!operations.contains("--run"));
    assert!(directory.path().join("cleaned").exists());
    assert!(sandbox.is_available());
}

/// An abnormal manager exit skips dependent cleanup and retains the execution slot.
#[tokio::test]
async fn unconfirmed_stop_retains_resources_and_blocks_reuse() {
    let (directory, mut sandbox) = fixture("manager_failure");
    let (_sender, stage_control) = control();
    let error = sandbox.run(spec(), stage_control).await.unwrap_err();
    assert_eq!(error.kind, SandboxErrorKind::StopUnconfirmed);
    assert!(!directory.path().join("cleaned").exists());
    assert!(!sandbox.is_available());
    let retained = error.retained_workspace.unwrap();
    assert!(retained.exists());
    let before = fs::read_to_string(directory.path().join("operations")).unwrap();
    let (_sender, stage_control) = control();
    assert_eq!(
        sandbox.run(spec(), stage_control).await.unwrap_err().kind,
        SandboxErrorKind::StopUnconfirmed
    );
    assert_eq!(
        before,
        fs::read_to_string(directory.path().join("operations")).unwrap()
    );
    // The substitute has exited without creating a user process; test-owned cleanup is safe.
    fs::remove_dir_all(retained).unwrap();
}

/// A Worker shutdown signal uses SIGTERM and preserves its cause through manager interruption.
#[tokio::test]
async fn internal_stop_is_confirmed_before_cleanup() {
    let (directory, mut sandbox) = fixture("internal_stop");
    let (sender, stage_control) = control();
    let request = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !directory.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        sender
            .send(Some(acorn_worker::sandbox::StopReason::Shutdown))
            .unwrap();
    };
    let (outcome, ()) = tokio::join!(sandbox.run(spec(), stage_control), request);
    let outcome = outcome.unwrap();
    assert_eq!(
        outcome.stop_reason,
        Some(acorn_worker::sandbox::StopReason::Shutdown)
    );
    assert_eq!(outcome.exit_signal, Some(15));
    assert!(sandbox.is_available());
    assert!(directory.path().join("cleaned").exists());
}
