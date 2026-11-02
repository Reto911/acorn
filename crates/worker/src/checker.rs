//! Strict checker result parsing and bounded private diagnostics.

use acorn_protocol::worker::v1::Verdict;
use serde::Deserialize;

use crate::sandbox::SandboxRunOutcome;

/// Maximum UTF-8 bytes of private stderr included in a failure log.
const LOG_DIAGNOSTIC_BYTES: usize = 4096;

/// Only the two business judgments permitted by the checker protocol.
#[derive(Debug, Deserialize)]
enum CheckerVerdict {
    /// The user output satisfies this case.
    #[serde(rename = "AC")]
    Accepted,
    /// The user output does not satisfy this case.
    #[serde(rename = "WA")]
    WrongAnswer,
}

/// A single result object; Serde rejects missing, duplicate and unknown fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckerResult {
    /// The sole result field, with no optional or default value.
    verdict: CheckerVerdict,
}

/// Accept exactly one UTF-8 JSON object, allowing only surrounding JSON whitespace.
fn parse(bytes: &[u8]) -> Result<Verdict, serde_json::Error> {
    // Derived struct deserialization also accepts arrays; this protocol requires an object.
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(serde::de::Error::custom("Checker result must be an object"));
    }
    let result: CheckerResult = serde_json::from_slice(bytes)?;
    Ok(match result.verdict {
        CheckerVerdict::Accepted => Verdict::Accepted,
        CheckerVerdict::WrongAnswer => Verdict::WrongAnswer,
    })
}

/// Require actual successful execution before accepting the complete stdout result.
pub(crate) fn verdict(outcome: &SandboxRunOutcome) -> Result<Verdict, String> {
    if outcome.memory_limit_exceeded {
        Err("memory limit exceeded.".into())
    } else if outcome.output_limit_exceeded {
        Err("result output limit exceeded.".into())
    } else if outcome.time_limit_exceeded {
        Err("time limit exceeded.".into())
    } else if let Some(signal) = outcome.exit_signal {
        Err(format!("signal {signal}."))
    } else if outcome.exit_code != Some(0) {
        Err(match outcome.exit_code {
            Some(code) => format!("exit code {code}."),
            None => "missing exit code.".into(),
        })
    } else {
        parse(&outcome.stdout.bytes).map_err(|_| "invalid result JSON.".into())
    }
}

/// Convert invalid bytes and cap the resulting UTF-8 log text, including replacements.
pub(crate) fn stderr_summary(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut end = text.len().min(LOG_DIAGNOSTIC_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    //! Required JSON shape and private log byte limits, independent of process execution.

    use super::*;

    /// Objects retain duplicate-field evidence and consume the entire UTF-8 JSON input.
    #[test]
    fn strict_result_object() {
        assert_eq!(
            parse(b" \n{\"verdict\":\"AC\"}\t").unwrap(),
            Verdict::Accepted
        );
        assert_eq!(
            parse(b"{\"verdict\":\"WA\"}").unwrap(),
            Verdict::WrongAnswer
        );
        for bytes in [
            b"".as_slice(),
            b"{}",
            b"null",
            b"[]",
            b"[\"AC\"]",
            b"\"AC\"",
            b"42",
            b"{\"verdict\":\"AC\",\"verdict\":\"WA\"}",
            b"{\"verdict\":\"AC\",\"debug\":true}",
            b"{\"verdict\":null}",
            b"{\"verdict\":true}",
            b"{\"verdict\":0}",
            b"{\"verdict\":\"RE\"}",
            b"{\"verdict\":\"ac\"}",
            b"{\"verdict\":\"AC\"} {}",
            b"debug {\"verdict\":\"AC\"}",
            b"{\"verdict\":\"AC\"}\xff",
            b"{\"verdict\":\"\xff\"}",
        ] {
            assert!(parse(bytes).is_err(), "Accepted {bytes:?}");
        }
    }

    /// Invalid-byte replacement and multibyte characters cannot expand logs past 4 KiB.
    #[test]
    fn private_stderr_summary_is_bounded_utf8() {
        for bytes in [vec![0xff; 8192], "诊断".repeat(4096).into_bytes()] {
            let summary = stderr_summary(&bytes);
            assert!(summary.len() <= LOG_DIAGNOSTIC_BYTES);
            assert!(summary.len() >= LOG_DIAGNOSTIC_BYTES - 3);
        }
        assert_eq!(stderr_summary(b"short"), "short");
    }
}
