//! Internal judging contracts tested without RPC or host isolation privileges.

mod support;

#[path = "support/reuse.rs"]
mod reuse;

use std::time::Duration;

use acorn_protocol::worker::v1::{JobPhase, SubmitJobRequest, Verdict, get_job_reply::Terminal};
use acorn_worker::{
    executor::{ExecutionOutcome, Executor},
    sandbox::StageControl,
};
use support::{Fixture, job};
use tokio::{sync::watch, time::Instant};

/// Judge through the complete executor while retaining the internal stop sender.
async fn execute(fixture: &Fixture, request: SubmitJobRequest) -> ExecutionOutcome {
    let mut executor = Executor::new(fixture.config.clone());
    let (_sender, stop) = watch::channel(None);
    let outcome = executor
        .run(
            request,
            StageControl {
                deadline: Instant::now() + Duration::from_secs(10),
                stop,
            },
            |_| {},
        )
        .await;
    assert!(executor.is_available());
    outcome
}

/// Extract a business result, reporting execution failures explicitly.
fn result(outcome: ExecutionOutcome) -> acorn_protocol::worker::v1::JudgeResult {
    match outcome.terminal {
        Terminal::Result(result) => result,
        Terminal::Failure(error) => panic!("{}: {}", error.code, error.diagnostic),
    }
}

/// C compiles once, executes cases serially and preserves only the first failed case.
#[tokio::test]
async fn c_ac_ce_and_wa_short_circuit() {
    let mut fixture = Fixture::new("normal");
    let source =
        "#include <stdio.h>\nint main(void) { int n; scanf(\"%d\", &n); printf(\"%d\\n\", n); }";
    let accepted = execute(
        &fixture,
        job(
            "c-ac",
            "c",
            source,
            &[(b"1", b"1\t\n\x0b\x0c\r "), (b"2", b"2")],
        ),
    )
    .await;
    assert_eq!(accepted.completed_cases, 2);
    let accepted = result(accepted);
    assert_eq!(accepted.verdict, Verdict::Accepted as i32);
    assert!(accepted.failed_case.is_none());
    let operations = fixture.operations();
    assert_eq!(operations.matches("--run").count(), 3);
    assert_eq!(operations.matches("/usr/bin/gcc").count(), 1);
    assert_eq!(operations.matches("--init").count(), 2);
    assert_eq!(operations.matches("--cleanup").count(), 2);

    let before = fixture.operations().matches("--run").count();
    fixture.config.compile.diagnostic_max_bytes = 64;
    let ce = execute(
        &fixture,
        job("c-ce", "c", "invalid C code", &[(b"", b""), (b"", b"")]),
    )
    .await;
    assert_eq!(ce.completed_cases, 0);
    let ce = result(ce);
    assert_eq!(ce.verdict, Verdict::CompileError as i32);
    assert!(ce.failed_case.is_none());
    assert!(!ce.compile_diagnostic.is_empty());
    assert!(ce.compile_diagnostic_truncated);
    assert_eq!(ce.compile_diagnostic.len(), 64);
    assert_eq!(fixture.operations().matches("--run").count() - before, 1);

    let before = fixture.operations().matches("--run").count();
    let wa = execute(
        &fixture,
        job(
            "c-wa",
            "c",
            source,
            &[(b"1", b"1"), (b"2", b"3"), (b"4", b"4")],
        ),
    )
    .await;
    assert_eq!(wa.completed_cases, 2);
    let wa = result(wa);
    assert_eq!(wa.verdict, Verdict::WrongAnswer as i32);
    let case = wa.failed_case.unwrap();
    assert_eq!(case.case_index, 2);
    assert_eq!(case.stdout, b"2\n");
    let usage = case.resources.unwrap();
    assert!(
        usage.cpu_time_us.is_none()
            && usage.wall_time_us.is_none()
            && usage.peak_memory_bytes.is_none()
    );
    assert_eq!(fixture.operations().matches("--run").count() - before, 3);
}

/// Python receives an original source copy each time; syntax errors are first-case RE.
#[tokio::test]
async fn python_independent_copies_and_syntax_error() {
    let fixture = Fixture::new("cleanup_failure");
    let source = "import pathlib\np = pathlib.Path(__file__)\nassert p.read_text().startswith('import')\np.write_text('broken source')\nprint(input())\n";
    let accepted = execute(
        &fixture,
        job(
            "python-ac",
            "python",
            source,
            &[(b"first\n", b"first"), (b"second\n", b"second")],
        ),
    )
    .await;
    assert_eq!(accepted.completed_cases, 2);
    assert_eq!(result(accepted).verdict, Verdict::Accepted as i32);
    assert_eq!(fixture.operations().matches("--run").count(), 2);
    let before = fixture.operations().matches("--run").count();
    let re = execute(
        &fixture,
        job(
            "python-re",
            "python",
            "def invalid(:",
            &[(b"", b""), (b"", b"")],
        ),
    )
    .await;
    assert_eq!(re.completed_cases, 1);
    let re = result(re);
    assert_eq!(re.verdict, Verdict::RuntimeError as i32);
    let case = re.failed_case.unwrap();
    assert_eq!(case.case_index, 1);
    assert!(!case.stderr.is_empty());
    assert_eq!(fixture.operations().matches("--run").count() - before, 1);
}

/// User files persist only within a Job, while fixed symlinks and program bytes are replaced.
#[tokio::test]
async fn job_reuses_environment_and_restores_fixed_program() {
    let fixture = Fixture::new("normal");
    let mut executor = Executor::new(fixture.config.clone());
    let source = r#"from pathlib import Path
import sys
counter = Path('counter')
number = int(counter.read_text()) + 1 if counter.exists() else 1
counter.write_text(str(number))
keep = Path('keep')
if number == 1:
    keep.write_text('untouched')
assert keep.read_text() == 'untouched'
program = Path(__file__)
program.unlink()
program.symlink_to('keep')
if sys.stdin.buffer.read():
    print(number, end='')
"#;
    for id in ["first-job", "next-job"] {
        let (_sender, stop) = watch::channel(None);
        let outcome = executor
            .run(
                job(
                    id,
                    "python",
                    source,
                    &[(b"one", b"1"), (b"two", b"2"), (b"", b"")],
                ),
                StageControl {
                    deadline: Instant::now() + Duration::from_secs(10),
                    stop,
                },
                |_| {},
            )
            .await;
        assert_eq!(outcome.completed_cases, 3);
        assert_eq!(result(outcome).verdict, Verdict::Accepted as i32);
        assert!(executor.is_available());
    }
    let operations = fixture.operations();
    assert_eq!(operations.matches("--init").count(), 2);
    assert_eq!(operations.matches("--run").count(), 6);
    assert_eq!(operations.matches("--cleanup").count(), 2);
    assert!(
        !operations.contains("--box-id=1 "),
        "Checker initializes only when actually needed"
    );
}

/// The controlled manager proves dual-box file plumbing, rather than host isolation.
#[tokio::test]
async fn checker_file_handoff_uses_independent_environments() {
    let fixture = Fixture::new("normal");
    reuse::environment_file_contract(&fixture.config).await;
    let operations = fixture.operations();
    for box_id in [0, 1] {
        let lines: Vec<_> = operations
            .lines()
            .filter(|line| line.contains(&format!("--box-id={box_id} ")))
            .collect();
        assert_eq!(
            lines.iter().filter(|line| line.contains("--init")).count(),
            2
        );
        assert_eq!(
            lines.iter().filter(|line| line.contains("--run")).count(),
            4
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("--cleanup"))
                .count(),
            2
        );
    }
}

/// Internal whitespace and non-ASCII bytes are not normalized by the checker.
#[tokio::test]
async fn comparison_preserves_internal_and_non_ascii_whitespace() {
    let fixture = Fixture::new("normal");
    for expected in [b"a b".as_slice(), b"a  b\xc2\xa0"] {
        let outcome = result(
            execute(
                &fixture,
                job("compare", "python", "print('a  b')", &[(b"", expected)]),
            )
            .await,
        );
        assert_eq!(outcome.verdict, Verdict::WrongAnswer as i32);
    }
}

/// Admission deadline stops the current stage and never starts subsequent cases.
#[tokio::test]
async fn deadline_stops_execution_and_reports_failure() {
    let fixture = Fixture::new("normal");
    let mut executor = Executor::new(fixture.config.clone());
    let (_sender, stop) = watch::channel(None);
    let mut phases = vec![];
    let outcome = executor
        .run(
            job(
                "deadline",
                "python",
                "import time\ntime.sleep(60)",
                &[(b"", b""), (b"", b"")],
            ),
            StageControl {
                deadline: Instant::now() + Duration::from_millis(300),
                stop,
            },
            |progress| phases.push(progress.phase),
        )
        .await;
    let Terminal::Failure(error) = outcome.terminal else {
        panic!("Expected deadline failure")
    };
    assert_eq!(error.code, "JobDeadlineExceeded");
    assert_eq!(outcome.completed_cases, 0);
    assert!(executor.is_available());
    assert_eq!(phases.last(), Some(&JobPhase::Done));
    assert_eq!(fixture.operations().matches("--run").count(), 1);
    assert_eq!(fixture.operations().matches("--cleanup").count(), 1);
}

/// Cleanup remains inside the admission deadline even after a case has been adjudicated.
#[tokio::test]
async fn deadline_includes_environment_cleanup() {
    let fixture = Fixture::new("slow_cleanup");
    let mut executor = Executor::new(fixture.config.clone());
    let (_sender, stop) = watch::channel(None);
    let outcome = executor
        .run(
            job("cleanup-deadline", "python", "print(42)", &[(b"", b"42")]),
            StageControl {
                deadline: Instant::now() + Duration::from_secs(1),
                stop,
            },
            |_| {},
        )
        .await;
    let Terminal::Failure(error) = outcome.terminal else {
        panic!("Expected cleanup to exceed the Job deadline")
    };
    assert_eq!(error.code, "JobDeadlineExceeded");
    assert_eq!(outcome.completed_cases, 1);
    assert!(executor.is_available());
    assert_eq!(fixture.operations().matches("--cleanup").count(), 1);
}

/// Confirmed OOM takes precedence over a simultaneous timeout.
#[tokio::test]
async fn confirmed_limits_have_documented_priority() {
    let fixture = Fixture::new("limits");
    let outcome = result(
        execute(
            &fixture,
            job("limits", "python", "print(42)", &[(b"", b"42")]),
        )
        .await,
    );
    assert_eq!(outcome.verdict, Verdict::MemoryLimitExceeded as i32);
}

/// Runtime overflow is OLE even though the controlled manager stops via SIGTERM.
#[tokio::test]
async fn output_limit_is_a_bounded_case_result() {
    let mut fixture = Fixture::new("normal");
    fixture.config.execution.stdout_max_bytes = 1024;
    let outcome = execute(
        &fixture,
        job(
            "ole",
            "python",
            "import os\nwhile True: os.write(1, b'x' * 8192)",
            &[(b"", b""), (b"", b"")],
        ),
    )
    .await;
    assert_eq!(outcome.completed_cases, 1);
    let result = result(outcome);
    assert_eq!(result.verdict, Verdict::OutputLimitExceeded as i32);
    let case = result.failed_case.unwrap();
    assert_eq!(case.stdout.len(), 1024);
    assert!(case.stdout_truncated);
    assert_eq!(fixture.operations().matches("--run").count(), 1);
}

/// Exercise the complete internal judge on a prepared host and dedicated real box.
#[tokio::test]
#[ignore = "requires prepared Linux/Isolate and dedicated ACORN_ISOLATE_BOX_ID/ACORN_CHECKER_BOX_ID"]
async fn real_executor_acceptance() {
    let mut config =
        acorn_worker::config::Config::parse(include_str!("../../../config/worker.example.toml"))
            .unwrap();
    config.sandbox.box_id = std::env::var("ACORN_ISOLATE_BOX_ID")
        .expect("Set a dedicated unused box ID")
        .parse()
        .unwrap();
    config.sandbox.checker_box_id = std::env::var("ACORN_CHECKER_BOX_ID")
        .expect("Set a dedicated checker box ID")
        .parse()
        .unwrap();
    config.validate().unwrap();
    let mut executor = Executor::new(config);
    let c = "#include <stdio.h>\nint main(void) { int a,b; scanf(\"%d%d\", &a,&b); printf(\"%d\\n\", a+b); }";
    for (id, language, source, expected, verdict, completed) in [
        ("real-c-ac", "c", c, b"42".as_slice(), Verdict::Accepted, 2),
        (
            "real-c-ce",
            "c",
            "invalid C",
            b"42".as_slice(),
            Verdict::CompileError,
            0,
        ),
        (
            "real-c-wa",
            "c",
            c,
            b"43".as_slice(),
            Verdict::WrongAnswer,
            1,
        ),
        (
            "real-python-ac",
            "python",
            "a,b=map(int,input().split());print(a+b)",
            b"42".as_slice(),
            Verdict::Accepted,
            2,
        ),
        (
            "real-python-re",
            "python",
            "def invalid(:",
            b"42".as_slice(),
            Verdict::RuntimeError,
            1,
        ),
    ] {
        let (_sender, stop) = watch::channel(None);
        let outcome = executor
            .run(
                job(
                    id,
                    language,
                    source,
                    &[(b"20 22\n", expected), (b"40 2\n", expected)],
                ),
                StageControl {
                    deadline: Instant::now() + Duration::from_secs(30),
                    stop,
                },
                |_| {},
            )
            .await;
        assert_eq!(outcome.completed_cases, completed, "{outcome:?}");
        assert_eq!(result(outcome).verdict, verdict as i32);
        assert!(executor.is_available());
    }
}
