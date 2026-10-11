//! Parse Isolate facts without inferring limits from signals or inventing statistics.

use anyhow::{Context, Result, bail};

use super::ResourceUsage;

/// Machine-readable Isolate termination and resource facts.
#[derive(Debug, Default)]
pub(super) struct Metadata {
    /// Optional program exit code.
    pub exit_code: Option<i32>,
    /// Optional terminating signal.
    pub exit_signal: Option<i32>,
    /// Confirmed cgroup OOM event.
    pub oom: bool,
    /// Confirmed CPU or elapsed timeout.
    pub timeout: bool,
    /// Actual measurements only.
    pub usage: ResourceUsage,
}

impl Metadata {
    /// Accept absent optional statistics and future unknown keys, rejecting bad facts.
    pub fn parse(text: &str) -> Result<Self> {
        let mut result = Self::default();
        for line in text.lines() {
            let (key, value) = line.split_once(':').context("Malformed Isolate metadata")?;
            match key {
                "exitcode" => result.exit_code = Some(value.parse().context("Invalid exitcode")?),
                "exitsig" => result.exit_signal = Some(value.parse().context("Invalid exitsig")?),
                "cg-oom-killed" => result.oom = value == "1",
                "status" => match value {
                    "TO" => result.timeout = true,
                    "SG" | "RE" => (),
                    "XX" => bail!("Isolate reported an internal execution error"),
                    _ => bail!("Unknown Isolate status: {value}"),
                },
                "time" => result.usage.cpu_time_us = Some(seconds_us(value)?),
                "time-wall" => result.usage.wall_time_us = Some(seconds_us(value)?),
                "cg-mem" => {
                    result.usage.memory_bytes = Some(
                        value
                            .parse::<u64>()
                            .context("Invalid cg-mem")?
                            .checked_mul(1024)
                            .context("cg-mem overflow")?,
                    );
                }
                _ => (),
            }
        }
        if result.exit_code.is_none()
            && result.exit_signal.is_none()
            && !result.timeout
            && !result.oom
        {
            bail!("Isolate metadata has no termination reason");
        }
        Ok(result)
    }
}

/// Convert fractional seconds exactly to microseconds, avoiding floating-point rounding.
fn seconds_us(value: &str) -> Result<u64> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 6 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("Invalid Isolate fractional time: {value}");
    }
    seconds
        .parse::<u64>()
        .context("Invalid Isolate seconds")?
        .checked_mul(1_000_000)
        .and_then(|whole| {
            whole.checked_add(
                fraction.parse::<u64>().unwrap_or(0) * 10_u64.pow(6 - fraction.len() as u32),
            )
        })
        .context("Isolate time overflow")
}

#[cfg(test)]
mod tests {
    //! Concrete metadata contracts needed for verdicts and optional statistics.

    use super::Metadata;

    /// A SIGKILL is not sufficient evidence of an OOM event.
    #[test]
    fn signal_without_oom_does_not_invent_memory_limit_or_stats() {
        let metadata = Metadata::parse("status:SG\nexitsig:9\n").unwrap();
        assert!(!metadata.oom);
        assert!(metadata.usage.memory_bytes.is_none());
        assert!(metadata.usage.cpu_time_us.is_none());
        let oom =
            Metadata::parse("status:SG\nexitsig:9\ncg-oom-killed:1\ncg-mem:4096\ntime:1.234\n")
                .unwrap();
        assert!(oom.oom);
        assert_eq!(oom.usage.memory_bytes, Some(4_194_304));
        assert_eq!(oom.usage.cpu_time_us, Some(1_234_000));
    }
}
