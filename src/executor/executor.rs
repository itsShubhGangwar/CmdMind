use std::fmt;
use std::io;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::security::{tokenize_command, ValidatedPlan};

/// Structured outcome of executing a command process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResult {
    /// The process exit status code (e.g. 0 for success, non-zero for failure, or None if terminated by signal)
    pub exit_code: Option<i32>,
    /// Whether the process exited successfully with exit code 0
    pub success: bool,
}

impl ExecutionResult {
    /// Creates a new `ExecutionResult` from an exit code.
    pub fn new(exit_code: Option<i32>) -> Self {
        let success = exit_code == Some(0);
        Self { exit_code, success }
    }
}

/// Errors occurring during command parsing or process execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    /// The validated command string contained no executable token.
    EmptyCommand,
    /// The command could not be safely tokenized into program and arguments.
    StructuredParseError(String),
    /// The requested binary was not found in PATH or at the specified location.
    CommandNotFound(String),
    /// Permission denied when attempting to execute the program.
    PermissionDenied(String),
    /// Process failed to start or execution error.
    ProcessFailed(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionError::EmptyCommand => write!(f, "Execution failed: empty command."),
            ExecutionError::StructuredParseError(msg) => {
                write!(f, "Structured command parsing failed: {}", msg)
            }
            ExecutionError::CommandNotFound(prog) => {
                write!(f, "Command not found: '{}'", prog)
            }
            ExecutionError::PermissionDenied(prog) => {
                write!(f, "Permission denied executing: '{}'", prog)
            }
            ExecutionError::ProcessFailed(msg) => {
                write!(f, "Process execution failed: {}", msg)
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

/// Trait abstraction for command execution to permit isolated testing with mock runners.
pub trait CommandRunner: Send + Sync {
    /// Runs a program with the given arguments list directly via OS process creation.
    fn run(&self, program: &str, args: &[String]) -> Result<ExecutionResult, ExecutionError>;
}

/// Real OS process execution runner using `std::process::Command`.
///
/// Security & Architecture:
/// - Invokes the OS process directly (via `execve` / `CreateProcessW`).
/// - NEVER invokes `sh`, `bash`, `zsh`, or `eval`.
/// - Inherits terminal I/O via `Stdio::inherit()` so actual process output streams to the user.
/// - Captures real exit status.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<ExecutionResult, ExecutionError> {
        let mut child = Command::new(program);
        child.args(args);
        child.stdin(Stdio::inherit());
        child.stdout(Stdio::inherit());
        child.stderr(Stdio::inherit());

        match child.status() {
            Ok(status) => Ok(ExecutionResult::new(status.code())),
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => {
                    Err(ExecutionError::CommandNotFound(program.to_string()))
                }
                io::ErrorKind::PermissionDenied => {
                    Err(ExecutionError::PermissionDenied(program.to_string()))
                }
                _ => Err(ExecutionError::ProcessFailed(e.to_string())),
            },
        }
    }
}

/// Mock command runner for testing without executing actual host binaries.
#[derive(Debug, Clone, Default)]
pub struct MockCommandRunner {
    calls: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    mock_result: Arc<Mutex<Option<Result<ExecutionResult, ExecutionError>>>>,
}

impl MockCommandRunner {
    /// Creates a new mock command runner that defaults to returning exit code 0.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            mock_result: Arc::new(Mutex::new(None)),
        }
    }

    /// Creates a mock command runner with a predetermined execution result.
    pub fn with_result(result: Result<ExecutionResult, ExecutionError>) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            mock_result: Arc::new(Mutex::new(Some(result))),
        }
    }

    /// Returns a copy of all calls made to this mock runner: `(program, args)`.
    pub fn get_calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }

    /// Returns the number of times `run` was called.
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl CommandRunner for MockCommandRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<ExecutionResult, ExecutionError> {
        self.calls
            .lock()
            .unwrap()
            .push((program.to_string(), args.to_vec()));

        if let Some(res) = self.mock_result.lock().unwrap().clone() {
            res
        } else {
            Ok(ExecutionResult::new(Some(0)))
        }
    }
}

/// Executes a security-validated plan using the default OS process runner.
///
/// Invariant:
/// Accepts ONLY `&ValidatedPlan`. Unvalidated `CommandPlan`, `String`, or `&str`
/// cannot be passed to this function.
pub fn execute(plan: &ValidatedPlan) -> Result<ExecutionResult, ExecutionError> {
    execute_with_runner(plan, &OsCommandRunner)
}

/// Executes a security-validated plan with a specified runner.
///
/// Invariant:
/// Accepts ONLY `&ValidatedPlan`. Parses the validated command into structured
/// `program` and `args` tokens, and passes them directly to the runner.
/// Never invokes any shell interpreter (`sh -c`, `bash -c`, `zsh -c`, `eval`).
pub fn execute_with_runner<R: CommandRunner>(
    plan: &ValidatedPlan,
    runner: &R,
) -> Result<ExecutionResult, ExecutionError> {
    let tokens = tokenize_command(plan.command()).map_err(|e| {
        ExecutionError::StructuredParseError(format!("Failed to parse command into tokens: {}", e))
    })?;

    if tokens.is_empty() {
        return Err(ExecutionError::EmptyCommand);
    }

    let program = &tokens[0];
    let args = &tokens[1..];

    runner.run(program, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{CommandPlan, CommandSource};
    use crate::security::validate;

    fn make_validated(cmd: &str) -> ValidatedPlan {
        let plan = CommandPlan::new(cmd, "Test command", CommandSource::Tier1);
        validate(plan).expect("command should be valid")
    }

    #[test]
    fn test_mock_executor_receives_structured_args_for_ls() {
        let runner = MockCommandRunner::new();
        let plan = make_validated("ls");
        let res = execute_with_runner(&plan, &runner).unwrap();

        assert!(res.success);
        assert_eq!(res.exit_code, Some(0));
        assert_eq!(runner.call_count(), 1);

        let calls = runner.get_calls();
        assert_eq!(calls[0].0, "ls");
        assert!(calls[0].1.is_empty());
    }

    #[test]
    fn test_mock_executor_receives_structured_args_for_git_status() {
        let runner = MockCommandRunner::new();
        let plan = make_validated("git status");
        let res = execute_with_runner(&plan, &runner).unwrap();

        assert!(res.success);
        assert_eq!(runner.call_count(), 1);

        let calls = runner.get_calls();
        assert_eq!(calls[0].0, "git");
        assert_eq!(calls[0].1, vec!["status".to_string()]);
    }

    #[test]
    fn test_mock_executor_receives_structured_args_for_find_pdf() {
        let runner = MockCommandRunner::new();
        let plan = make_validated("find . -type f -name '*.pdf'");
        let res = execute_with_runner(&plan, &runner).unwrap();

        assert!(res.success);
        assert_eq!(runner.call_count(), 1);

        let calls = runner.get_calls();
        assert_eq!(calls[0].0, "find");
        assert_eq!(
            calls[0].1,
            vec![
                ".".to_string(),
                "-type".to_string(),
                "f".to_string(),
                "-name".to_string(),
                "*.pdf".to_string(),
            ]
        );
    }

    #[test]
    fn test_mock_executor_receives_quoted_arguments_with_spaces() {
        let runner = MockCommandRunner::new();
        let plan = make_validated("find \"my folder\" -name 'my file.txt'");
        let res = execute_with_runner(&plan, &runner).unwrap();

        assert!(res.success);
        assert_eq!(runner.call_count(), 1);

        let calls = runner.get_calls();
        assert_eq!(calls[0].0, "find");
        assert_eq!(
            calls[0].1,
            vec![
                "my folder".to_string(),
                "-name".to_string(),
                "my file.txt".to_string(),
            ]
        );
    }

    #[test]
    fn test_mock_executor_preserves_non_zero_exit_code() {
        let runner = MockCommandRunner::with_result(Ok(ExecutionResult::new(Some(42))));
        let plan = make_validated("ls");
        let res = execute_with_runner(&plan, &runner).unwrap();

        assert!(!res.success);
        assert_eq!(res.exit_code, Some(42));
    }

    #[test]
    fn test_mock_executor_propagates_command_not_found_error() {
        let runner = MockCommandRunner::with_result(Err(ExecutionError::CommandNotFound(
            "nonexistent_bin".to_string(),
        )));
        let plan = make_validated("ls");
        let err = execute_with_runner(&plan, &runner).unwrap_err();

        match err {
            ExecutionError::CommandNotFound(cmd) => assert_eq!(cmd, "nonexistent_bin"),
            other => panic!("Expected CommandNotFound, got {:?}", other),
        }
    }

    #[test]
    fn test_mock_executor_propagates_permission_denied_error() {
        let runner = MockCommandRunner::with_result(Err(ExecutionError::PermissionDenied(
            "restricted_bin".to_string(),
        )));
        let plan = make_validated("ls");
        let err = execute_with_runner(&plan, &runner).unwrap_err();

        match err {
            ExecutionError::PermissionDenied(cmd) => assert_eq!(cmd, "restricted_bin"),
            other => panic!("Expected PermissionDenied, got {:?}", other),
        }
    }
}
