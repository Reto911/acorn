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
async fn execute(
    config: &acorn_worker::config::Config,
    request: SubmitJobRequest,
) -> ExecutionOutcome {
    let mut executor = Executor::new(config.clone());
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
        &fixture.config,
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
    assert_eq!(operations.matches("--run").count(), 5);
    assert_eq!(operations.matches("/usr/bin/gcc").count(), 1);
    assert_eq!(operations.matches("--init").count(), 3);
    assert_eq!(operations.matches("--cleanup").count(), 3);

    let before = fixture.operations().matches("--run").count();
    fixture.config.compile.diagnostic_max_bytes = 64;
    let ce = execute(
        &fixture.config,
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
        &fixture.config,
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
    assert_eq!(fixture.operations().matches("--run").count() - before, 5);
}

/// Python receives an original source copy each time; syntax errors are first-case RE.
#[tokio::test]
async fn python_independent_copies_and_syntax_error() {
    let fixture = Fixture::new("cleanup_failure");
    let source = "import pathlib\np = pathlib.Path(__file__)\nassert p.read_text().startswith('import')\np.write_text('broken source')\nprint(input())\n";
    let accepted = execute(
        &fixture.config,
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
    assert_eq!(fixture.operations().matches("--run").count(), 4);
    let before = fixture.operations().matches("--run").count();
    let re = execute(
        &fixture.config,
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
    assert_eq!(operations.matches("--init").count(), 4);
    assert_eq!(operations.matches("--run").count(), 12);
    assert_eq!(operations.matches("--cleanup").count(), 4);
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
                &fixture.config,
                job("compare", "python", "print('a  b')", &[(b"", expected)]),
            )
            .await,
        );
        assert_eq!(outcome.verdict, Verdict::WrongAnswer as i32);
    }
    let accepted = execute(
        &fixture.config,
        job(
            "binary-trim",
            "python",
            "import sys\nsys.stdout.buffer.write(sys.stdin.buffer.read())",
            &[(b"\x00\xffword\t\n\x0b\x0c\r ", b"\x00\xffword")],
        ),
    )
    .await;
    assert_eq!(result(accepted).verdict, Verdict::Accepted as i32);
}

/// Float material distinguishes user format errors from invalid private reference/configuration data.
#[tokio::test]
async fn float_preset_contract() {
    let fixture = Fixture::new("normal");
    for (actual, reference, config, expected) in [
        (
            b"1.000e0\t-2.500\r\n".as_slice(),
            b"1 -2.5".as_slice(),
            "{}",
            "AC",
        ),
        (b"1e-6", b"0", "{}", "AC"),
        (b"2e-6", b"0", "{}", "WA"),
        (
            b"1000000.5",
            b"1000000",
            r#"{"abs_tol":0,"rel_tol":1e-6}"#,
            "AC",
        ),
        (
            b"1000002",
            b"1000000",
            r#"{"abs_tol":0,"rel_tol":1e-6}"#,
            "WA",
        ),
        (b"1.0000001", b"1", r#"{"abs_tol":0,"rel_tol":0}"#, "WA"),
        (b"", b"1", "{}", "WA"),
        (b"1 2", b"1", "{}", "WA"),
        (b"bad", b"1", "{}", "WA"),
        (b"NaN", b"1", "{}", "WA"),
        (b"inf", b"1", "{}", "WA"),
        (b"", b"bad-reference", "{}", "CheckerFailed"),
        (b"0", b"NaN", "{}", "CheckerFailed"),
        (b"0", b"inf", "{}", "CheckerFailed"),
        (b"0", b"0", r#"{"abs_tol":true}"#, "CheckerFailed"),
        (b"0", b"0", r#"{"rel_tol":-0.1}"#, "CheckerFailed"),
        (b"0", b"0", r#"{"rel_tol":1}"#, "CheckerFailed"),
        (b"0", b"0", r#"{"abs_tol":"0"}"#, "CheckerFailed"),
        (b"0", b"0", r#"{"abs_tol":null}"#, "CheckerFailed"),
        // Script-level validation; the public admission parser may reject this number earlier.
        (b"0", b"0", r#"{"abs_tol":1e309}"#, "CheckerFailed"),
        (b"0", b"0", r#"{"unexpected":0}"#, "CheckerFailed"),
    ] {
        let mut request = job(
            "float-preset",
            "python",
            "import sys\nsys.stdout.buffer.write(sys.stdin.buffer.read())",
            &[(actual, reference)],
        );
        request.checker_source = include_bytes!("../../../examples/checkers/float.py").to_vec();
        request.checker_config_json = config.into();
        let outcome = execute(&fixture.config, request).await;
        match outcome.terminal {
            Terminal::Result(judged) => {
                assert_eq!(outcome.completed_cases, 1);
                assert_eq!(
                    judged.verdict,
                    if expected == "AC" {
                        Verdict::Accepted
                    } else {
                        Verdict::WrongAnswer
                    } as i32,
                    "{expected}: {actual:?}, {config}"
                );
                assert_ne!(expected, "CheckerFailed");
            }
            Terminal::Failure(failure) => {
                assert_eq!(expected, "CheckerFailed", "{failure:?}");
                assert_eq!(failure.code, expected);
                assert_eq!(outcome.completed_cases, 0);
                assert!(!failure.diagnostic.contains("bad-reference"));
            }
        }
    }
}

/// Execute shipped material through the existing pipeline; no expected verdict means CheckerFailed.
async fn assert_preset(
    config: &acorn_worker::config::Config,
    checker: &[u8],
    actual: &[u8],
    reference: &[u8],
    parameters: &str,
    expected: Option<Verdict>,
) {
    let mut request = job(
        "generic-preset",
        "python",
        "import sys\nsys.stdout.buffer.write(sys.stdin.buffer.read())",
        &[(actual, reference)],
    );
    request.checker_source = checker.to_vec();
    request.checker_config_json = parameters.into();
    let outcome = execute(config, request).await;
    assert_eq!(outcome.completed_cases, u32::from(expected.is_some()));
    if let Some(expected) = expected {
        assert_eq!(result(outcome).verdict, expected as i32);
    } else {
        let Terminal::Failure(failure) = outcome.terminal else {
            panic!("Expected CheckerFailed");
        };
        assert_eq!(failure.code, "CheckerFailed");
        assert!(!failure.diagnostic.contains("private-reference"));
    }
}

/// Ordered and unordered word materials preserve raw tokens, whitespace rules and multiplicities.
#[tokio::test]
async fn word_presets_contract() {
    let fixture = Fixture::new("normal");
    for (checker, unordered) in [
        (
            include_bytes!("../../../examples/checkers/tokens.py").as_slice(),
            false,
        ),
        (
            include_bytes!("../../../examples/checkers/unordered-tokens.py").as_slice(),
            true,
        ),
    ] {
        for (actual, reference, accepted) in [
            (b"a\tb\nc\r\x0b\x0c ".as_slice(), b"a b c".as_slice(), true),
            (b"c a b", b"a b c", unordered),
            (b"A b", b"a b", false),
            (b"a", b"a a", false),
            (b"a b b", b"a a b", false),
            (b"\x00\xff b", b"\x00\xff\tb", true),
            (b"a\xc2\xa0b", b"a b", false),
            (b" \t\r\n", b"", true),
            (b"a b c", b"a b", false),
        ] {
            assert_preset(
                &fixture.config,
                checker,
                actual,
                reference,
                "{}",
                Some(if accepted {
                    Verdict::Accepted
                } else {
                    Verdict::WrongAnswer
                }),
            )
            .await;
        }
    }
}

/// Decimal integer material keeps exact unbounded values and validates reference data first.
#[tokio::test]
async fn integer_preset_contract() {
    let fixture = Fixture::new("normal");
    let checker = include_bytes!("../../../examples/checkers/integers.py");
    for (actual, reference, expected) in [
        (
            b"+0001 -0002 -0 00 +0".as_slice(),
            b"1 -2 0 -0 000".as_slice(),
            Some(Verdict::Accepted),
        ),
        (
            b"9007199254740993",
            b"9007199254740992",
            Some(Verdict::WrongAnswer),
        ),
        (b"2 1", b"1 2", Some(Verdict::WrongAnswer)),
        (b"1", b"1 2", Some(Verdict::WrongAnswer)),
        (b"1 2", b"1", Some(Verdict::WrongAnswer)),
        (b" \t\r\n", b"", Some(Verdict::Accepted)),
        (b"invalid-user", b"1 private-reference", None),
    ] {
        assert_preset(&fixture.config, checker, actual, reference, "{}", expected).await;
    }
    for invalid in [b"1_0".as_slice(), b"1.0", b"+", b"1e0", b"\xd9\xa1"] {
        assert_preset(
            &fixture.config,
            checker,
            invalid,
            b"1",
            "{}",
            Some(Verdict::WrongAnswer),
        )
        .await;
    }
    // Exceed Python's usual decimal conversion limit without raising that limit in the script.
    let reference = vec![b'9'; 5000];
    let mut actual = b"+000".to_vec();
    actual.extend_from_slice(&reference);
    assert_preset(
        &fixture.config,
        checker,
        &actual,
        &reference,
        "{}",
        Some(Verdict::Accepted),
    )
    .await;
    *actual.last_mut().unwrap() = b'8';
    assert_preset(
        &fixture.config,
        checker,
        &actual,
        &reference,
        "{}",
        Some(Verdict::WrongAnswer),
    )
    .await;
}

/// Unordered record material normalizes only permitted whitespace while retaining field grouping.
#[tokio::test]
async fn line_preset_contract() {
    let fixture = Fixture::new("normal");
    for (actual, reference, accepted) in [
        (
            b"c d\r\n\r\n a\tb \r\n".as_slice(),
            b"a b\nc d\n".as_slice(),
            true,
        ),
        (b"a b\na b\nb a", b"a b\nb a\na b", true),
        (b"a c\nb d", b"a b\nc d", false),
        (b"b a", b"a b", false),
        (b"a b\nc d\nc d", b"a b\na b\nc d", false),
        (b"\r\n \t\n", b"", true),
        (b"a b\nc d", b"a b", false),
        (b"a b\rc d", b"a b\nc d", false),
    ] {
        assert_preset(
            &fixture.config,
            include_bytes!("../../../examples/checkers/unordered-lines.py"),
            actual,
            reference,
            "{}",
            Some(if accepted {
                Verdict::Accepted
            } else {
                Verdict::WrongAnswer
            }),
        )
        .await;
    }
}

/// All parameter-free materials reject nonempty objects instead of judging with ignored options.
#[tokio::test]
async fn generic_preset_parameters() {
    let fixture = Fixture::new("normal");
    for checker in [
        include_bytes!("../../../examples/checkers/tokens.py").as_slice(),
        include_bytes!("../../../examples/checkers/integers.py").as_slice(),
        include_bytes!("../../../examples/checkers/unordered-tokens.py").as_slice(),
        include_bytes!("../../../examples/checkers/unordered-lines.py").as_slice(),
    ] {
        assert_preset(
            &fixture.config,
            checker,
            b"0",
            b"0",
            r#"{"unexpected":false}"#,
            None,
        )
        .await;
    }
}

/// The same sample and multi-answer checks run with a controlled or real Isolate manager.
async fn preset_examples(config: &acorn_worker::config::Config) {
    let cases: &[(&[u8], &[u8])] = &[
        (b"0 1\n", b"0\n"),
        (b"1 3\n", b"0.3333333333333333\n"),
        (b"-1 3\n", b"-0.3333333333333333\n"),
        (b"1 1000000\n", b"0.000001\n"),
        (b"-1 1000000\n", b"-0.000001\n"),
        (b"1000000 1\n", b"1000000\n"),
        (b"-1000000 1\n", b"-1000000\n"),
        (b"1000000 3\n", b"333333.3333333333\n"),
    ];
    for (language, source) in [
        (
            "c",
            include_str!("../../../examples/submissions/quotient.c"),
        ),
        (
            "python",
            include_str!("../../../examples/submissions/quotient.py"),
        ),
    ] {
        let mut request = job("quotient-preset", language, source, cases);
        request.checker_source = include_bytes!("../../../examples/checkers/float.py").to_vec();
        request.checker_config_json = r#"{"abs_tol":1e-6,"rel_tol":1e-6}"#.into();
        let outcome = execute(config, request).await;
        assert_eq!(outcome.completed_cases, 8);
        assert_eq!(result(outcome).verdict, Verdict::Accepted as i32);
    }
    for (output, expected) in [
        ("1 4", Verdict::Accepted),
        ("2 3", Verdict::Accepted),
        ("1 3", Verdict::WrongAnswer),
        ("0 5", Verdict::WrongAnswer),
        ("one four", Verdict::WrongAnswer),
    ] {
        let mut request = job(
            "sum-pair",
            "python",
            &format!("print({output:?})"),
            &[(b"5\n", b"")],
        );
        request.checker_source = include_bytes!("../../../examples/checkers/sum-pair.py").to_vec();
        let outcome = execute(config, request).await;
        assert_eq!(outcome.completed_cases, 1);
        assert_eq!(result(outcome).verdict, expected as i32);
    }
}

/// Sample submissions use the shipped checker bytes, rather than an embedded test comparator.
#[tokio::test]
async fn preset_samples_and_multiple_answers() {
    let fixture = Fixture::new("normal");
    preset_examples(&fixture.config).await;
}

/// Real Isolate executes C/Python quotient samples and independent valid sum-pair outputs.
#[tokio::test]
#[ignore = "requires prepared Linux/Isolate and dedicated ACORN_ISOLATE_BOX_ID/ACORN_CHECKER_BOX_ID"]
async fn real_preset_acceptance() {
    preset_examples(&real_config()).await;
}

/// Production checker execution restores its fixed files and returns only user measurements on WA.
#[tokio::test]
async fn checker_inputs_reuse_and_user_resources() {
    let fixture = Fixture::new("resources");
    let mut executor = Executor::new(fixture.config.clone());
    let user = r#"from pathlib import Path
import sys
for name in ['checker.py', 'input.bin', 'answer.bin', 'output.bin', 'config.json']:
    assert not Path(name).exists()
Path('ordinary').write_text('user-only')
sys.stdout.buffer.write(sys.stdin.buffer.read())
"#;
    let checker = r#"from pathlib import Path
import json, sys
assert sys.argv[1:] == ['input.bin', 'answer.bin', 'output.bin', 'config.json']
assert sys.flags.isolated and sys.flags.no_site and sys.dont_write_bytecode
assert sys.stdin.buffer.read() == b''
assert not Path('ordinary').exists()
assert Path('input.bin').read_bytes() == Path('output.bin').read_bytes()
assert Path('answer.bin').read_bytes() == b'\x00\xffprivate-answer'
assert json.loads(Path('config.json').read_text()) == {'secret': 'private-config'}
counter = Path('counter')
number = int(counter.read_text()) + 1 if counter.exists() else 1
counter.write_text(str(number))
for name in ['checker.py', 'input.bin', 'answer.bin', 'output.bin', 'config.json']:
    Path(name).write_bytes(b'changed')
print(json.dumps({'verdict': 'AC' if number == 1 else 'WA'}))
"#;
    for id in ["checker-first", "checker-next"] {
        let mut request = job(
            id,
            "python",
            user,
            &[
                (b"\x00\xfffirst", b"\x00\xffprivate-answer"),
                (b"second", b"\x00\xffprivate-answer"),
                (b"third", b""),
            ],
        );
        request.checker_source = checker.as_bytes().to_vec();
        request.checker_config_json = r#"{"secret":"private-config"}"#.into();
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
        assert_eq!(outcome.completed_cases, 2);
        let result = result(outcome);
        assert_eq!(result.verdict, Verdict::WrongAnswer as i32);
        let case = result.failed_case.unwrap();
        assert_eq!(case.case_index, 2);
        assert_eq!(case.stdout, b"second");
        assert_eq!(
            case.exit,
            Some(acorn_protocol::worker::v1::failed_case::Exit::ExitCode(0))
        );
        let usage = case.resources.unwrap();
        assert_eq!(usage.cpu_time_us, Some(1000));
        assert_eq!(usage.wall_time_us, Some(2000));
        assert_eq!(usage.peak_memory_bytes, Some(123 * 1024));
        assert!(executor.is_available());
    }
    let operations = fixture.operations();
    assert_eq!(operations.matches("--run").count(), 8);
    assert_eq!(operations.matches("--init").count(), 4);
    assert_eq!(operations.matches("--cleanup").count(), 4);
    assert!(operations.contains("-I -B -S checker.py input.bin answer.bin output.bin config.json"));
}

/// Failed execution and malformed results never publish checker bytes or count an undecided case.
#[tokio::test]
async fn checker_failures_are_private_and_preserve_progress() {
    for (mode, source, reason) in [
        ("normal", "def invalid(:", "exit code 1"),
        (
            "normal",
            "raise RuntimeError('private-script')",
            "exit code 1",
        ),
        (
            "normal",
            "import sys\nprint('{\"verdict\":\"AC\"}', flush=True)\nprint('private-stderr', file=sys.stderr)\nsys.exit(7)",
            "exit code 7",
        ),
        (
            "normal",
            "print('private-debug {\"verdict\":\"AC\"}')",
            "invalid result JSON",
        ),
        (
            "normal",
            "import os, signal\nos.kill(os.getpid(), signal.SIGTERM)",
            "signal 15",
        ),
        (
            "checker_timeout",
            "print('{\"verdict\":\"AC\"}')",
            "time limit",
        ),
        (
            "checker_oom",
            "print('{\"verdict\":\"AC\"}')",
            "memory limit",
        ),
    ] {
        let fixture = Fixture::new(mode);
        let mut request = job(
            "checker-error",
            "python",
            "print('user')",
            &[(b"", b"private-answer")],
        );
        request.checker_source = source.as_bytes().to_vec();
        let outcome = execute(&fixture.config, request).await;
        assert_eq!(outcome.completed_cases, 0, "{mode}: {source}");
        let Terminal::Failure(error) = outcome.terminal else {
            panic!("Expected CheckerFailed")
        };
        assert_eq!(error.code, "CheckerFailed");
        assert!(error.diagnostic.starts_with("Checker failed for case 1:"));
        assert!(error.diagnostic.contains(reason), "{}", error.diagnostic);
        assert!(!error.diagnostic.contains("private"));
    }
    let fixture = Fixture::new("normal");
    let mut request = job(
        "later-checker-error",
        "python",
        "print('user')",
        &[(b"first", b""), (b"second", b""), (b"third", b"")],
    );
    request.checker_source = b"from pathlib import Path\nif Path('input.bin').read_bytes() == b'first':\n    print('{\"verdict\":\"AC\"}')\nelse:\n    raise RuntimeError('private-error')".to_vec();
    let outcome = execute(&fixture.config, request).await;
    assert_eq!(outcome.completed_cases, 1);
    let Terminal::Failure(error) = outcome.terminal else {
        panic!("Expected CheckerFailed")
    };
    assert_eq!(error.code, "CheckerFailed");
    assert_eq!(error.diagnostic, "Checker failed for case 2: exit code 1.");
    assert_eq!(fixture.operations().matches("--run").count(), 4);
}

/// Long private diagnostics drain normally, but result overflow rejects even an early valid prefix.
#[tokio::test]
async fn checker_output_limits_are_independent() {
    let mut fixture = Fixture::new("normal");
    fixture.config.execution.stdout_max_bytes = 8;
    fixture.config.checker.result_max_bytes = 64;
    fixture.config.checker.diagnostic_max_bytes = 32;
    let mut request = job("checker-stderr", "python", "print('user')", &[(b"", b"")]);
    request.checker_source =
        b"import os\nos.write(2, b'\\xff' * 8192)\nprint('{\"verdict\":\"AC\"}')".to_vec();
    assert_eq!(
        result(execute(&fixture.config, request.clone()).await).verdict,
        Verdict::Accepted as i32
    );
    request.checker_source =
        b"import os\nprint('{\"verdict\":\"AC\"}', flush=True)\nos.write(1, b'x' * 8192)".to_vec();
    let outcome = execute(&fixture.config, request).await;
    assert_eq!(outcome.completed_cases, 0);
    let Terminal::Failure(error) = outcome.terminal else {
        panic!("Expected result overflow failure")
    };
    assert_eq!(error.code, "CheckerFailed");
    assert!(error.diagnostic.contains("result output limit"));
}

/// Absolute Job deadlines and shutdown stop active checkers even with no independent limits.
#[tokio::test]
async fn checker_deadline_and_shutdown_keep_their_causes() {
    for shutdown in [false, true] {
        let fixture = Fixture::new("normal");
        let mut executor = Executor::new(fixture.config.clone());
        let mut request = job("checker-stop", "python", "print('user')", &[(b"", b"")]);
        request.checker_source =
            b"from pathlib import Path\nPath('ready').touch()\nimport time\ntime.sleep(60)"
                .to_vec();
        request.checker_limits = Some(Default::default());
        let (sender, stop) = watch::channel(None);
        let run = executor.run(
            request,
            StageControl {
                deadline: Instant::now()
                    + Duration::from_millis(if shutdown { 5000 } else { 2000 }),
                stop,
            },
            |_| {},
        );
        let signal = async {
            if shutdown {
                let ready = fixture.directory.path().join(format!(
                    "isolate-{}/box/ready",
                    fixture.config.sandbox.checker_box_id
                ));
                tokio::time::timeout(Duration::from_secs(2), async {
                    while !ready.exists() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
                sender
                    .send(Some(acorn_worker::sandbox::StopReason::Shutdown))
                    .unwrap();
            }
        };
        let (outcome, ()) = tokio::join!(run, signal);
        assert_eq!(outcome.completed_cases, 0);
        let Terminal::Failure(error) = outcome.terminal else {
            panic!("Expected checker stop")
        };
        assert_eq!(
            error.code,
            if shutdown {
                "WorkerShutdown"
            } else {
                "JobDeadlineExceeded"
            }
        );
        assert!(executor.is_available());
        let operations = fixture.operations();
        let checker_run = operations
            .lines()
            .find(|line| line.contains("--run") && line.contains("checker.py"))
            .unwrap();
        assert!(
            !checker_run.contains("--time=")
                && !checker_run.contains("--wall-time=")
                && !checker_run.contains("--cg-mem=")
        );
        assert_eq!(operations.matches("--cleanup").count(), 2);
    }
}

/// An unconfirmed checker stop retains the Job and blocks all subsequent execution.
#[tokio::test]
async fn checker_unconfirmed_stop_retains_execution_slot() {
    let fixture = Fixture::new("checker_manager_failure");
    let mut executor = Executor::new(fixture.config.clone());
    for _ in 0..2 {
        let (_sender, stop) = watch::channel(None);
        let outcome = executor
            .run(
                job("blocked-checker", "python", "print(42)", &[(b"", b"42")]),
                StageControl {
                    deadline: Instant::now() + Duration::from_secs(10),
                    stop,
                },
                |_| {},
            )
            .await;
        let Terminal::Failure(error) = outcome.terminal else {
            panic!("Expected unconfirmed checker stop")
        };
        assert_eq!(error.code, "StopUnconfirmed");
        assert_eq!(outcome.completed_cases, 0);
        assert!(!executor.is_available());
        let operations = fixture.operations();
        assert_eq!(operations.matches("--run").count(), 2);
        assert_eq!(operations.matches("--cleanup").count(), 1);
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
                deadline: Instant::now() + Duration::from_secs(2),
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
    assert_eq!(fixture.operations().matches("--cleanup").count(), 2);
}

/// Confirmed OOM takes precedence over a simultaneous timeout.
#[tokio::test]
async fn confirmed_limits_have_documented_priority() {
    let fixture = Fixture::new("limits");
    let outcome = result(
        execute(
            &fixture.config,
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
        &fixture.config,
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
    let config = real_config();
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

/// Real metadata separates checker limits, Job deadlines and historical user memory.
#[tokio::test]
#[ignore = "requires prepared Linux/Isolate and dedicated ACORN_ISOLATE_BOX_ID/ACORN_CHECKER_BOX_ID"]
async fn real_checker_acceptance() {
    use acorn_protocol::worker::v1::CheckerLimits;

    let config = real_config();
    let mut executor = Executor::new(config.clone());
    let mut request = job(
        "real-checker-wa",
        "python",
        "import sys\nif sys.stdin.buffer.read() == b'first':\n    allocation = bytearray(32 * 1024 * 1024)\nprint('user')",
        &[(b"first", b"different"), (b"second", b"different")],
    );
    request.checker_source = b"from pathlib import Path\np = Path('counter')\nn = int(p.read_text()) + 1 if p.exists() else 1\np.write_text(str(n))\nprint('{\"verdict\":\"AC\"}' if n == 1 else '{\"verdict\":\"WA\"}')".to_vec();
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
    assert_eq!(outcome.completed_cases, 2);
    let judged = result(outcome);
    assert_eq!(judged.verdict, Verdict::WrongAnswer as i32);
    let case = judged.failed_case.unwrap();
    assert_eq!(case.stdout, b"user\n");
    assert!(case.resources.unwrap().peak_memory_bytes.unwrap() >= 32 * 1024 * 1024);

    for (source, limits, reason) in [
        (
            "while True: pass",
            CheckerLimits {
                cpu_time_ms: Some(100),
                ..Default::default()
            },
            "time limit",
        ),
        (
            "import time\ntime.sleep(60)",
            CheckerLimits {
                wall_time_ms: Some(100),
                ..Default::default()
            },
            "time limit",
        ),
        (
            "allocation = bytearray(128 * 1024 * 1024)",
            CheckerLimits {
                memory_kib: Some(32 * 1024),
                ..Default::default()
            },
            "memory limit",
        ),
        (
            "import sys\nprint('{\"verdict\":\"AC\"}', flush=True)\nsys.exit(7)",
            CheckerLimits::default(),
            "exit code 7",
        ),
    ] {
        let mut request = job(
            "real-checker-limits",
            "python",
            "print('user')",
            &[(b"", b"")],
        );
        request.checker_source = source.as_bytes().to_vec();
        request.checker_limits = Some(limits);
        let (_sender, stop) = watch::channel(None);
        let outcome = executor
            .run(
                request,
                StageControl {
                    deadline: Instant::now() + Duration::from_secs(5),
                    stop,
                },
                |_| {},
            )
            .await;
        assert_eq!(outcome.completed_cases, 0);
        let Terminal::Failure(error) = outcome.terminal else {
            panic!("Expected checker execution failure")
        };
        assert_eq!(error.code, "CheckerFailed");
        assert!(error.diagnostic.contains(reason), "{}", error.diagnostic);
        assert!(executor.is_available());
    }
    let mut request = job(
        "real-checker-deadline",
        "python",
        "print('user')",
        &[(b"", b"")],
    );
    request.checker_source = b"import time\ntime.sleep(60)".to_vec();
    request.checker_limits = Some(CheckerLimits::default());
    let (_sender, stop) = watch::channel(None);
    let outcome = executor
        .run(
            request,
            StageControl {
                deadline: Instant::now() + Duration::from_millis(300),
                stop,
            },
            |_| {},
        )
        .await;
    let Terminal::Failure(error) = outcome.terminal else {
        panic!("Expected absolute deadline failure")
    };
    assert_eq!(error.code, "JobDeadlineExceeded");
    assert_eq!(outcome.completed_cases, 0);
    assert!(executor.is_available());
    for id in [config.sandbox.box_id, config.sandbox.checker_box_id] {
        assert!(!std::path::Path::new(&format!("/var/lib/isolate/{id}")).exists());
    }
}

/// Select two deployment-reserved real Isolate IDs for serial acceptance checks.
fn real_config() -> acorn_worker::config::Config {
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
    config
}
