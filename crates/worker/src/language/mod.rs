//! Deployment-controlled language plans interpreted by the common executor.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::config::Config;

/// CPU, wall-clock, process-tree memory and process-count budgets for one stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageLimits {
    /// Maximum process-tree CPU time, in milliseconds.
    pub cpu_time_ms: u64,
    /// Maximum elapsed wall time, in milliseconds.
    pub wall_time_ms: u64,
    /// Maximum process-tree memory, in bytes.
    pub memory_bytes: u64,
    /// Maximum number of processes and threads.
    pub process_max_count: u32,
}

/// Executable, argument array and explicit environment for a sandbox stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPlan {
    /// Executable path as visible inside the sandbox.
    pub executable: PathBuf,
    /// Arguments passed directly, without a shell.
    pub arguments: Vec<String>,
    /// Explicit environment entries; host environment is not inherited.
    pub environment: BTreeMap<String, String>,
}

/// Optional compilation stage producing a fixed executable artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilePlan {
    /// Compiler invocation.
    pub command: CommandPlan,
    /// Resource budget for the compiler process tree.
    pub limits: StageLimits,
    /// Fixed relative path of the artifact eligible for export.
    pub artifact_name: String,
}

/// Which original file must be copied into each fresh runtime sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramFiles {
    /// Use the untouched source bytes under the fixed source filename.
    Source,
    /// Use the artifact exported after successful compilation.
    CompiledArtifact,
}

/// Complete language-specific preparation rules without executing any code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguagePlan {
    /// Fixed source filename inside the stage.
    pub source_name: String,
    /// Optional compiler invocation and budget.
    pub compile: Option<CompilePlan>,
    /// Rule selecting the file copied into each runtime sandbox.
    pub program_files: ProgramFiles,
    /// Per-case invocation; limits come from the admitted Job.
    pub run: CommandPlan,
}

/// Build a C or Python plan entirely from trusted deployment configuration.
pub fn plan(language_id: &str, config: &Config) -> Result<LanguagePlan> {
    let environment = config.sandbox.environment.clone();
    match language_id {
        "c" => Ok(LanguagePlan {
            source_name: "main.c".into(),
            compile: Some(CompilePlan {
                command: CommandPlan {
                    executable: config.languages.c.compiler.clone(),
                    arguments: config.languages.c.compile_arguments.clone(),
                    environment: environment.clone(),
                },
                limits: StageLimits {
                    cpu_time_ms: config.compile.cpu_time_ms,
                    wall_time_ms: config.compile.wall_time_ms,
                    memory_bytes: config.compile.memory_bytes,
                    process_max_count: config.compile.process_max_count,
                },
                artifact_name: "program".into(),
            }),
            program_files: ProgramFiles::CompiledArtifact,
            run: CommandPlan {
                executable: "./program".into(),
                arguments: Vec::new(),
                environment,
            },
        }),
        "python" => Ok(LanguagePlan {
            source_name: "main.py".into(),
            compile: None,
            program_files: ProgramFiles::Source,
            run: CommandPlan {
                executable: config.languages.python.interpreter.clone(),
                arguments: config.languages.python.run_arguments.clone(),
                environment,
            },
        }),
        _ => bail!("Unsupported language: {language_id}"),
    }
}
