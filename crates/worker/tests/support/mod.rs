//! Controlled Isolate transport for orchestration tests, never isolation evidence.

use std::{fs, os::unix::fs::PermissionsExt};

use acorn_protocol::worker::v1::{CheckerLimits, RunLimits, SubmitJobRequest, TestCase};
use acorn_worker::config::Config;

/// Test-owned manager and files; only trusted test programs run on the host.
pub struct Fixture {
    /// Temporary operation log and independent directories for each box ID.
    pub directory: tempfile::TempDir,
    /// Deployment configuration selecting the controlled manager.
    pub config: Config,
}

impl Fixture {
    /// Use real GCC/Python with controlled Isolate init, metadata and cleanup calls.
    pub fn new(mode: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let manager = directory.path().join("manager");
        fs::write(
            &manager,
            format!(
                r#"#!/usr/bin/python3
import pathlib, shutil, signal, subprocess, sys, time
root = pathlib.Path({root:?})
mode = {mode:?}
if (root / 'mode').exists():
    mode = (root / 'mode').read_text()
args = sys.argv[1:]
box_id = next(a[9:] for a in args if a.startswith('--box-id=')) if '--version' not in args else 'version'
box_root = root / ('isolate-' + box_id)
with (root / 'operations').open('a') as log:
    log.write(' '.join(args) + '\n')
if '--version' in args:
    print('controlled manager')
elif '--init' in args:
    (box_root / 'box').mkdir(parents=True)
    print(box_root)
elif '--run' in args:
    meta = pathlib.Path(next(a[7:] for a in args if a.startswith('--meta=')))
    if mode == 'manager_failure':
        sys.exit(2)
    command = args[args.index('--') + 1:]
    stopped = False
    child = None
    def stop(signum, frame):
        global stopped
        stopped = True
        if child is not None:
            child.terminate()
    signal.signal(signal.SIGTERM, stop)
    child = subprocess.Popen(command, cwd=box_root / 'box')
    if stopped:
        child.terminate()
    code = child.wait()
    if mode == 'limits':
        meta.write_text('status:TO\ncg-oom-killed:1\nexitsig:9\n')
    elif code < 0:
        meta.write_text('status:SG\nexitsig:' + str(-code) + '\n')
    else:
        meta.write_text('exitcode:' + str(code) + '\n')
    sys.exit(1 if code else 0)
elif '--cleanup' in args:
    if mode == 'slow_cleanup':
        time.sleep(1.2)
    shutil.rmtree(box_root)
    if mode == 'cleanup_failure':
        sys.exit(2)
"#,
                root = directory.path().to_str().unwrap()
            ),
        )
        .unwrap();
        fs::set_permissions(&manager, fs::Permissions::from_mode(0o700)).unwrap();
        let mut config =
            Config::parse(include_str!("../../../../config/worker.example.toml")).unwrap();
        config.sandbox.executable = manager;
        Self { directory, config }
    }

    /// Read the operations actually requested by the executor.
    pub fn operations(&self) -> String {
        fs::read_to_string(self.directory.path().join("operations")).unwrap_or_default()
    }
}

/// Build a small admitted request for executor and RPC contract tests.
pub fn job(id: &str, language: &str, source: &str, cases: &[(&[u8], &[u8])]) -> SubmitJobRequest {
    SubmitJobRequest {
        job_id: id.into(),
        language_id: language.into(),
        source: source.as_bytes().to_vec(),
        cases: cases
            .iter()
            .map(|(input, expected)| TestCase {
                input: input.to_vec(),
                expected_output: expected.to_vec(),
            })
            .collect(),
        run_limits: Some(RunLimits {
            cpu_time_ms: 1000,
            wall_time_ms: 2000,
            memory_kib: 128 * 1024,
        }),
        checker_source: br#"import json
import sys
from pathlib import Path
trailing = b"\t\n\v\f\r "
actual = Path(sys.argv[3]).read_bytes()
expected = Path(sys.argv[2]).read_bytes()
print(json.dumps({"verdict": "AC" if actual.rstrip(trailing) == expected.rstrip(trailing) else "WA"}))
"#
        .to_vec(),
        checker_config_json: "{}".into(),
        checker_limits: Some(CheckerLimits {
            cpu_time_ms: Some(1000),
            wall_time_ms: Some(2000),
            memory_kib: Some(128 * 1024),
        }),
    }
}
