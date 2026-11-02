//! Shared file and lifecycle probes run with controlled and real Isolate managers.

use std::time::Duration;

use acorn_protocol::worker::v1::{SubmitJobRequest, TestCase};
use acorn_worker::{
    config::Config,
    executor::checker_files,
    language::{self, StageLimits},
    sandbox::{IsolateSandbox, OutputPolicy, SandboxFile, SandboxRunSpec, StageControl},
};
use tokio::{sync::watch, time::Instant};

/// Verify independent environments, fixed-file replay, empty output and fresh Job state.
pub async fn environment_file_contract(config: &Config) {
    let mut user = IsolateSandbox::new(config.sandbox.clone());
    let mut checker_config = config.sandbox.clone();
    checker_config.box_id = checker_config.checker_box_id;
    let mut checker = IsolateSandbox::new(checker_config);
    let user_source = br#"from pathlib import Path
import sys
for name in ['checker.py', 'input.bin', 'answer.bin', 'output.bin', 'config.json']:
    assert not Path(name).exists(), name
data = sys.stdin.buffer.read()
ordinary = Path('ordinary')
number = int(ordinary.read_text()) + 1 if ordinary.exists() else 1
assert number == (1 if data else 2)
ordinary.write_text(str(number))
sys.stdout.buffer.write(data)
"#;
    let checker_source = br#"from pathlib import Path
import json
assert not Path('ordinary').exists()
data = Path('input.bin').read_bytes()
assert Path('output.bin').read_bytes() == data
assert Path('answer.bin').read_bytes() == b'\x00\xffprivate'
assert json.loads(Path('config.json').read_text()) == {'secret': 'private'}
counter = Path('checker-counter')
number = int(counter.read_text()) + 1 if counter.exists() else 1
assert number == (1 if data else 2)
counter.write_text(str(number))
for name in ['checker.py', 'input.bin', 'answer.bin', 'output.bin', 'config.json']:
    Path(name).write_bytes(b'changed')
print('ready')
"#;
    let request = SubmitJobRequest {
        checker_source: checker_source.to_vec(),
        checker_config_json: r#"{"secret":"private"}"#.into(),
        ..Default::default()
    };
    let plan = language::plan("python", config).unwrap();
    let private = tempfile::tempdir().unwrap();
    let output = private.path().join("output.bin");
    for _ in 0..2 {
        let (_sender, stop) = watch::channel(None);
        let control = StageControl {
            deadline: Instant::now() + Duration::from_secs(30),
            stop,
        };
        for input in [b"\x00\xffone".as_slice(), b""] {
            let case = TestCase {
                input: input.to_vec(),
                expected_output: b"\x00\xffprivate".to_vec(),
            };
            let actual = user
                .run_in_environment(
                    SandboxRunSpec {
                        command: plan.run.clone(),
                        files: vec![SandboxFile {
                            name: "main.py".into(),
                            bytes: user_source.to_vec(),
                            executable: false,
                        }],
                        stdin: case.input.clone(),
                        limits: StageLimits {
                            cpu_time_ms: Some(1000),
                            wall_time_ms: Some(2000),
                            memory_kib: Some(128 * 1024),
                            process_max_count: 1,
                        },
                        output: OutputPolicy::Run {
                            stdout_max_bytes: 1024,
                            stderr_max_bytes: 1024,
                        },
                        artifact: None,
                    },
                    control.clone(),
                )
                .await
                .unwrap();
            assert_eq!(
                actual.exit_code,
                Some(0),
                "{}",
                String::from_utf8_lossy(&actual.stderr.bytes)
            );
            assert_eq!(actual.stdout.bytes, case.input);
            tokio::fs::write(&output, actual.stdout.bytes)
                .await
                .unwrap();
            let mut command = plan.run.clone();
            *command.arguments.last_mut().unwrap() = "checker.py".into();
            let checked = checker
                .run_in_environment(
                    SandboxRunSpec {
                        command,
                        files: checker_files(&request, &case, &output).await.unwrap(),
                        stdin: vec![],
                        limits: StageLimits {
                            cpu_time_ms: None,
                            wall_time_ms: None,
                            memory_kib: None,
                            process_max_count: 1,
                        },
                        output: OutputPolicy::Run {
                            stdout_max_bytes: 1024,
                            stderr_max_bytes: 1024,
                        },
                        artifact: None,
                    },
                    control.clone(),
                )
                .await
                .unwrap();
            assert_eq!(
                checked.exit_code,
                Some(0),
                "{}",
                String::from_utf8_lossy(&checked.stderr.bytes)
            );
            assert_eq!(checked.stdout.bytes, b"ready\n");
        }
        user.cleanup().await.unwrap();
        checker.cleanup().await.unwrap();
        assert!(user.is_available() && checker.is_available());
    }
}
