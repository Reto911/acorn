//! Opt-in acceptance checks requiring a prepared Linux host and a dedicated Isolate box.

use std::{fs, net::TcpListener, os::fd::AsRawFd, time::Duration};

use acorn_worker::{
    config::Config,
    language::{self, StageLimits},
    sandbox::{
        ArtifactSpec, IsolateSandbox, OutputPolicy, SandboxFile, SandboxRunOutcome, SandboxRunSpec,
        StageControl, StopReason,
    },
};
use nix::fcntl::{FcntlArg, fcntl};
use tokio::{sync::watch, time::Instant};

/// Source and configuration for real stages, using the same public API as P04.
async fn python(config: &Config, source: &str) -> SandboxRunOutcome {
    let plan = language::plan("python", config).unwrap();
    let spec = SandboxRunSpec {
        command: plan.run,
        files: vec![SandboxFile {
            name: plan.source_name,
            bytes: source.as_bytes().to_vec(),
            executable: false,
        }],
        stdin: vec![],
        limits: limits(),
        output: output(),
        artifact: None,
    };
    execute(config, spec).await
}

/// Small case budgets suitable for these acceptance commands.
fn limits() -> StageLimits {
    StageLimits {
        cpu_time_ms: 2000,
        wall_time_ms: 3000,
        memory_kib: 128 * 1024,
        process_max_count: 1,
    }
}

/// Bounded runtime output, shared by representative real commands.
fn output() -> OutputPolicy {
    OutputPolicy::Run {
        stdout_max_bytes: 4096,
        stderr_max_bytes: 4096,
    }
}

/// Await a complete real stage, retaining its stop sender until completion.
async fn execute(config: &Config, spec: SandboxRunSpec) -> SandboxRunOutcome {
    let (_sender, stop) = watch::channel(None);
    let mut sandbox = IsolateSandbox::new(config.sandbox.clone());
    let outcome = sandbox
        .run(
            spec,
            StageControl {
                deadline: Instant::now() + Duration::from_secs(30),
                stop,
            },
        )
        .await
        .unwrap();
    assert!(sandbox.is_available());
    assert!(
        !std::path::Path::new(&format!("/var/lib/isolate/{}", config.sandbox.box_id)).exists(),
        "Real stage must invoke cleanup"
    );
    outcome
}

/// Validate C, Python, OLE, shared quotas and host isolation using the actual backend.
#[tokio::test]
#[ignore = "requires prepared Linux/Isolate, ACORN_ISOLATE_BOX_ID and ACORN_ISOLATION_SENTINEL"]
async fn real_stage_acceptance() {
    let mut config = Config::parse(include_str!("../../../config/worker.example.toml")).unwrap();
    config.sandbox.box_id = std::env::var("ACORN_ISOLATE_BOX_ID")
        .expect("Set a dedicated unused box ID")
        .parse()
        .unwrap();
    assert_eq!(
        std::env::var("ACORN_ISOLATION_SENTINEL").unwrap(),
        "host-only-value"
    );
    let plan = language::plan("c", &config).unwrap();
    let compile = plan.compile.unwrap();
    let compiled = execute(&config, SandboxRunSpec {
        command: compile.command,
        files: vec![SandboxFile { name: plan.source_name, bytes: b"#include <stdio.h>\nint main(void) { int a,b; if(scanf(\"%d%d\", &a,&b)!=2)return 1; printf(\"%d\\n\", a+b); }".to_vec(), executable: false }],
        stdin: vec![], limits: compile.limits,
        output: OutputPolicy::Compile { max_bytes: config.compile.diagnostic_max_bytes },
        artifact: Some(ArtifactSpec { name: compile.artifact_name, max_bytes: config.compile.artifact_max_bytes }),
    }).await;
    assert_eq!(
        compiled.exit_code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&compiled.stderr.bytes)
    );
    let artifact = compiled.artifact.unwrap();
    for input in [b"20 22\n".to_vec(), b"40 2\n".to_vec()] {
        let ran = execute(
            &config,
            SandboxRunSpec {
                command: plan.run.clone(),
                files: vec![SandboxFile {
                    name: "program".into(),
                    bytes: artifact.clone(),
                    executable: true,
                }],
                stdin: input,
                limits: limits(),
                output: output(),
                artifact: None,
            },
        )
        .await;
        assert_eq!(ran.exit_code, Some(0));
        assert_eq!(ran.stdout.bytes, b"42\n");
        assert!(ran.usage.cpu_time_us.is_some() && ran.usage.wall_time_us.is_some());
    }
    let ran = python(&config, "print(6 * 7)\n").await;
    assert_eq!(ran.exit_code, Some(0));
    assert_eq!(ran.stdout.bytes, b"42\n");
    let overflow = python(&config, "import os\nwhile True: os.write(1, b'x' * 8192)\n").await;
    assert!(overflow.output_limit_exceeded && overflow.stdout.truncated);
    assert_eq!(overflow.stdout.bytes.len(), 4096);
    assert_eq!(overflow.stop_reason, Some(StopReason::OutputLimit));

    let mut file_limited = config.clone();
    file_limited.sandbox.file_size_max_kib = 1;
    let ran = python(
        &file_limited,
        "import errno, os, signal\nsignal.signal(signal.SIGXFSZ, signal.SIG_IGN)\nwith open('/box/limited', 'wb', buffering=0) as f:\n    assert f.write(b'x' * 1024) == 1024\n    try:\n        f.write(b'x')\n    except OSError as e:\n        assert e.errno == errno.EFBIG, e\n    else:\n        raise AssertionError('One-KiB file limit did not apply')\n    assert os.fstat(f.fileno()).st_size == 1024\nprint('file-limit')\n",
    ).await;
    assert_eq!(
        ran.exit_code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&ran.stderr.bytes)
    );
    assert_eq!(ran.stdout.bytes, b"file-limit\n");

    // These host resources exist while the isolated interpreter probes them.
    let private = tempfile::tempdir().unwrap();
    let private_file = private.path().join("private");
    fs::write(&private_file, "host-private").unwrap();
    let file = fs::File::open(&private_file).unwrap();
    let extra_fd = fcntl(&file, FcntlArg::F_DUPFD(55)).unwrap();
    assert!(extra_fd >= 55 && extra_fd != file.as_raw_fd());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        r#"import os, socket
assert not os.path.exists({private:?})
assert 'ACORN_ISOLATION_SENTINEL' not in os.environ
try:
    os.fstat({extra_fd})
    raise AssertionError('Host FD inherited')
except OSError:
    pass
s = socket.socket()
s.settimeout(0.2)
assert s.connect_ex(('127.0.0.1', {port})) != 0
print('isolated')
"#,
        private = private_file.to_str().unwrap()
    );
    let isolated = python(&config, &probe).await;
    nix::unistd::close(extra_fd).unwrap();
    assert_eq!(
        isolated.exit_code,
        Some(0),
        "{}",
        String::from_utf8_lossy(&isolated.stderr.bytes)
    );
    assert_eq!(isolated.stdout.bytes, b"isolated\n");

    for inodes in [false, true] {
        let mut quota = config.clone();
        quota.sandbox.disk_quota_kib = if inodes { 65536 } else { 512 };
        quota.sandbox.disk_quota_inodes = if inodes { 12 } else { 4096 };
        let script = format!(
            r#"import errno, os
paths = ['/box', '/tmp', '/dev/shm']
touched = set()
total = 0
try:
    for i in range(1000):
        path = paths[i % 3]
        if {inodes}:
            open(path + '/inode-' + str(i), 'wb').close()
        else:
            with open(path + '/data', 'ab', buffering=0) as f:
                f.write(b'x' * 8192)
        touched.add(path)
        total += 1
    raise AssertionError('Quota did not apply')
except OSError as e:
    assert e.errno == errno.EDQUOT, e
    assert len(touched) == 3, touched
    assert total < {maximum}, total
    print('shared-quota', total)
"#,
            inodes = if inodes { "True" } else { "False" },
            maximum = if inodes { 12 } else { 64 }
        );
        let ran = python(&quota, &script).await;
        assert_eq!(
            ran.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&ran.stderr.bytes)
        );
        assert!(ran.stdout.bytes.starts_with(b"shared-quota "));
    }
}
