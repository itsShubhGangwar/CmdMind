use std::fmt;

use crate::intent::{CommandPlan, CommandSource};

/// Maximum allowed length in characters for a command to be considered valid.
/// Restricts excessively large command strings to prevent parser exhaustion or buffer abuse.
pub const MAX_COMMAND_LENGTH: usize = 1000;

/// Explicit list of dangerous command executables that are unconditionally rejected.
pub const DANGEROUS_COMMANDS: &[&str] = &[
    "rm", "sudo", "mkfs", "dd", "shutdown", "reboot", "poweroff", "halt", "fdisk", "diskutil",
];

/// Critical filesystem paths and system root locations that commands may not target.
pub const DANGEROUS_PATH_PREFIXES: &[&str] = &[
    "/etc", "/system", "/library", "/bin", "/sbin", "/usr", "/var", "/dev",
];

/// Represents a command plan that has successfully passed the security validation boundary.
///
/// Type-State Security Boundary:
/// An instance of `ValidatedPlan` can ONLY be created by passing an untrusted `CommandPlan`
/// through the `validate()` function. It has no public constructors and its fields are immutable,
/// guaranteeing at compile time that only validated commands can be accepted by downstream execution APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedPlan {
    inner: CommandPlan,
}

impl ValidatedPlan {
    /// Internal constructor, accessible only within the security module.
    fn new_validated(inner: CommandPlan) -> Self {
        Self { inner }
    }

    /// Returns a reference to the validated shell command string.
    pub fn command(&self) -> &str {
        &self.inner.command
    }

    /// Returns a reference to the human-readable explanation.
    pub fn explanation(&self) -> &str {
        &self.inner.explanation
    }

    /// Returns a reference to the command's origin source.
    pub fn source(&self) -> &CommandSource {
        &self.inner.source
    }

    /// Borrows the underlying `CommandPlan`.
    #[allow(dead_code)]
    pub fn as_plan(&self) -> &CommandPlan {
        &self.inner
    }

    /// Consumes the `ValidatedPlan` and returns the underlying `CommandPlan`.
    #[allow(dead_code)]
    pub fn into_inner(self) -> CommandPlan {
        self.inner
    }
}

/// Errors produced when a command fails the security validation policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityError {
    /// The command is empty or composed solely of whitespace.
    EmptyCommand,
    /// The command exceeds the maximum allowed length.
    CommandTooLong { length: usize, max: usize },
    /// The command references an explicitly prohibited executable.
    DangerousCommand(String),
    /// The command attempts to target a prohibited system root or critical directory.
    DangerousPath(String),
    /// The command contains shell chaining, piping, substitution, or redirection.
    ShellControlConstruct(String),
    /// The command format cannot be safely reasoned about (fails closed).
    UnsupportedCommandFormat(String),
}

impl fmt::Display for SecurityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecurityError::EmptyCommand => {
                write!(f, "Empty or whitespace-only command rejected.")
            }
            SecurityError::CommandTooLong { length, max } => {
                write!(
                    f,
                    "Command length ({} characters) exceeds maximum allowed limit of {} characters.",
                    length, max
                )
            }
            SecurityError::DangerousCommand(cmd) => {
                write!(
                    f,
                    "Dangerous command detected: '{}' is prohibited by security policy.",
                    cmd
                )
            }
            SecurityError::DangerousPath(path) => {
                write!(
                    f,
                    "Dangerous filesystem target detected: '{}' accesses restricted or critical system paths.",
                    path
                )
            }
            SecurityError::ShellControlConstruct(construct) => {
                write!(
                    f,
                    "Shell control construct detected: '{}' (chaining, redirection, and substitution are prohibited).",
                    construct
                )
            }
            SecurityError::UnsupportedCommandFormat(reason) => {
                write!(f, "Unsupported command format: {}.", reason)
            }
        }
    }
}

impl std::error::Error for SecurityError {}

/// Validates an untrusted `CommandPlan` against the conservative security policy.
///
/// Transition:
///   `CommandPlan` (untrusted) ──► `validate()` ──► `Result<ValidatedPlan, SecurityError>`
///
/// Fails Closed:
///   Any command that cannot be confidently verified as safe is rejected.
pub fn validate(plan: CommandPlan) -> Result<ValidatedPlan, SecurityError> {
    // 1. Length Check
    if plan.command.len() > MAX_COMMAND_LENGTH {
        return Err(SecurityError::CommandTooLong {
            length: plan.command.len(),
            max: MAX_COMMAND_LENGTH,
        });
    }

    // 2. Empty / Whitespace Check
    let trimmed = plan.command.trim();
    if trimmed.is_empty() {
        return Err(SecurityError::EmptyCommand);
    }

    // 3. Shell Control Constructs, Chaining, Substitution, and Redirection
    // We fail closed if any shell-chaining or substitution token is present.
    check_shell_control_constructs(trimmed)?;

    // 4. Dangerous Path Traversal Checks
    check_path_traversal(trimmed)?;

    // 5. Tokenization & Structural Inspection
    let tokens = tokenize_command(trimmed)?;
    if tokens.is_empty() {
        return Err(SecurityError::EmptyCommand);
    }

    // 6. Prohibited Executables & Standalone Dangerous Commands
    check_dangerous_commands(&tokens)?;

    // 7. Dangerous Filesystem Targets
    check_dangerous_targets(&tokens)?;

    // All validation criteria satisfied; elevate to trusted type state.
    Ok(ValidatedPlan::new_validated(plan))
}

/// Checks for shell control operators, pipes, substitutions, and redirections.
fn check_shell_control_constructs(cmd: &str) -> Result<(), SecurityError> {
    // Check characters that introduce chaining, background execution, or substitution
    for c in cmd.chars() {
        match c {
            ';' => {
                return Err(SecurityError::ShellControlConstruct(
                    "semicolon ';'".to_string(),
                ))
            }
            '&' => {
                return Err(SecurityError::ShellControlConstruct(
                    "operator '&'".to_string(),
                ))
            }
            '|' => {
                return Err(SecurityError::ShellControlConstruct(
                    "pipe operator '|'".to_string(),
                ))
            }
            '$' => {
                return Err(SecurityError::ShellControlConstruct(
                    "substitution or variable expansion '$'".to_string(),
                ))
            }
            '`' => {
                return Err(SecurityError::ShellControlConstruct(
                    "backtick substitution '`'".to_string(),
                ))
            }
            '>' => {
                return Err(SecurityError::ShellControlConstruct(
                    "redirection operator '>'".to_string(),
                ))
            }
            '<' => {
                return Err(SecurityError::ShellControlConstruct(
                    "redirection operator '<'".to_string(),
                ))
            }
            _ => {}
        }
    }

    Ok(())
}

/// Checks for relative path traversal constructs.
fn check_path_traversal(cmd: &str) -> Result<(), SecurityError> {
    if cmd.contains("../") || cmd.contains("..\\") || cmd.ends_with("/..") || cmd.ends_with("\\..")
    {
        return Err(SecurityError::DangerousPath(
            ".. (directory traversal)".to_string(),
        ));
    }
    Ok(())
}

/// Tokenizes a command into words while honoring single and double quotes.
pub fn tokenize_command(cmd: &str) -> Result<Vec<String>, SecurityError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in cmd.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if in_single || in_double {
        return Err(SecurityError::UnsupportedCommandFormat(
            "unmatched quote in command".to_string(),
        ));
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

/// Validates that no explicitly dangerous executables are invoked.
fn check_dangerous_commands(tokens: &[String]) -> Result<(), SecurityError> {
    // 1. Identify primary executable (skip leading environment variable assignments like VAR=val)
    let primary_token = tokens
        .iter()
        .find(|t| !t.contains('='))
        .unwrap_or(&tokens[0]);

    let primary_exe = extract_executable_name(primary_token);
    if is_prohibited_command(&primary_exe) {
        return Err(SecurityError::DangerousCommand(primary_exe));
    }

    // 2. Check for sub-commands or standalone dangerous commands
    // e.g., 'sudo ...', 'xargs rm', 'find ... -exec rm ...'
    for (i, token) in tokens.iter().enumerate() {
        let exe = extract_executable_name(token);
        if is_prohibited_command(&exe) {
            return Err(SecurityError::DangerousCommand(exe));
        }

        // Check if token introduces a sub-command like '-exec' or 'xargs'
        if (token == "-exec" || token == "-execdir" || token == "xargs") && i + 1 < tokens.len() {
            let sub_exe = extract_executable_name(&tokens[i + 1]);
            if is_prohibited_command(&sub_exe) {
                return Err(SecurityError::DangerousCommand(sub_exe));
            }
        }
    }

    Ok(())
}

/// Extracts the bare binary name from a path (e.g. "/usr/bin/rm" -> "rm", "rm.exe" -> "rm").
fn extract_executable_name(token: &str) -> String {
    let clean = token.trim();
    let file_name = clean
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .unwrap_or(clean);

    let base = file_name
        .strip_suffix(".exe")
        .or_else(|| file_name.strip_suffix(".EXE"))
        .unwrap_or(file_name);

    base.to_lowercase()
}

/// Returns true if the command name matches the prohibited dangerous command list or variants.
fn is_prohibited_command(name: &str) -> bool {
    let lower = name.to_lowercase();
    DANGEROUS_COMMANDS
        .iter()
        .any(|&cmd| lower == cmd || lower.starts_with(&format!("{}.", cmd)))
}

/// Checks that command tokens do not target critical filesystem roots or system locations.
fn check_dangerous_targets(tokens: &[String]) -> Result<(), SecurityError> {
    for token in tokens {
        let clean = token.trim().replace('\\', "/");
        let lower = clean.to_lowercase();

        // Check root filesystem target
        if lower == "/" || lower == "//" || lower == "/*" || lower.starts_with("/*") {
            return Err(SecurityError::DangerousPath(
                "/ (root filesystem)".to_string(),
            ));
        }

        // Check parent directory token
        if lower == ".." {
            return Err(SecurityError::DangerousPath(
                ".. (directory traversal)".to_string(),
            ));
        }

        // Check critical system directories
        for prefix in DANGEROUS_PATH_PREFIXES {
            let prefix_with_slash = format!("{}/", prefix);
            if lower == *prefix || lower.starts_with(&prefix_with_slash) {
                return Err(SecurityError::DangerousPath(token.clone()));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(command: &str) -> CommandPlan {
        CommandPlan::new(command, "Test explanation", CommandSource::Tier1)
    }

    // --- Safe Commands Tests ---

    #[test]
    fn test_safe_ls_command() {
        let plan = make_plan("ls");
        let validated = validate(plan).expect("ls should be valid");
        assert_eq!(validated.command(), "ls");
    }

    #[test]
    fn test_safe_git_status() {
        let plan = make_plan("git status");
        let validated = validate(plan).expect("git status should be valid");
        assert_eq!(validated.command(), "git status");
    }

    #[test]
    fn test_safe_find_pdf() {
        let plan = make_plan("find . -type f -name '*.pdf'");
        let validated = validate(plan).expect("find pdf should be valid");
        assert_eq!(validated.command(), "find . -type f -name '*.pdf'");
    }

    #[test]
    fn test_safe_find_python_mtime() {
        let plan = make_plan("find . -type f -name '*.py' -mtime -2");
        let validated = validate(plan).expect("find python should be valid");
        assert_eq!(validated.command(), "find . -type f -name '*.py' -mtime -2");
    }

    #[test]
    fn test_safe_command_with_substring_words() {
        // Filename containing "rm" inside "formal" or "format"
        let plan = make_plan("cat formal_document.txt");
        assert!(validate(plan).is_ok());

        // Filename containing "dd" inside "middle"
        let plan2 = make_plan("find . -type f -name '*middle*'");
        assert!(validate(plan2).is_ok());

        // Filename containing "remove"
        let plan3 = make_plan("ls remove_old.py");
        assert!(validate(plan3).is_ok());

        // Safe relative path
        let plan4 = make_plan("find ./src -name '*.rs'");
        assert!(validate(plan4).is_ok());
    }

    // --- Dangerous Commands Rejection Tests ---

    #[test]
    fn test_reject_rm() {
        let plan = make_plan("rm -rf my_folder");
        let err = validate(plan).unwrap_err();
        match err {
            SecurityError::DangerousCommand(cmd) => assert_eq!(cmd, "rm"),
            other => panic!("expected DangerousCommand, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_sudo() {
        let plan = make_plan("sudo apt update");
        let err = validate(plan).unwrap_err();
        match err {
            SecurityError::DangerousCommand(cmd) => assert_eq!(cmd, "sudo"),
            other => panic!("expected DangerousCommand, got {:?}", other),
        }
    }

    #[test]
    fn test_reject_case_variation_dangerous_command() {
        let plan = make_plan("RM -rf /tmp");
        assert!(matches!(
            validate(plan),
            Err(SecurityError::DangerousCommand(_))
        ));

        let plan2 = make_plan("SUDO ls");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::DangerousCommand(_))
        ));
    }

    #[test]
    fn test_reject_other_dangerous_commands() {
        let prohibited = [
            "mkfs /dev/sda1",
            "dd if=/dev/zero of=/dev/null",
            "shutdown -h now",
            "reboot",
            "poweroff",
            "halt",
            "fdisk /dev/sda",
            "diskutil eraseDisk",
        ];

        for cmd in prohibited {
            let plan = make_plan(cmd);
            assert!(
                matches!(validate(plan), Err(SecurityError::DangerousCommand(_))),
                "command should be rejected: {}",
                cmd
            );
        }
    }

    #[test]
    fn test_reject_subcommand_in_find_exec() {
        let plan = make_plan("find . -exec rm {} +");
        let err = validate(plan).unwrap_err();
        assert!(matches!(err, SecurityError::DangerousCommand(_)));
    }

    // --- Shell Control Constructs Rejection Tests ---

    #[test]
    fn test_reject_semicolon_chaining() {
        let plan = make_plan("ls; rm something");
        let err = validate(plan).unwrap_err();
        assert!(matches!(err, SecurityError::ShellControlConstruct(_)));
    }

    #[test]
    fn test_reject_logical_operators() {
        let plan1 = make_plan("ls && rm something");
        assert!(matches!(
            validate(plan1),
            Err(SecurityError::ShellControlConstruct(_))
        ));

        let plan2 = make_plan("ls || rm something");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::ShellControlConstruct(_))
        ));
    }

    #[test]
    fn test_reject_pipe_operator() {
        let plan = make_plan("ls | grep something");
        assert!(matches!(
            validate(plan),
            Err(SecurityError::ShellControlConstruct(_))
        ));
    }

    #[test]
    fn test_reject_command_substitution() {
        let plan1 = make_plan("echo $(whoami)");
        assert!(matches!(
            validate(plan1),
            Err(SecurityError::ShellControlConstruct(_))
        ));

        let plan2 = make_plan("echo `whoami`");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::ShellControlConstruct(_))
        ));
    }

    #[test]
    fn test_reject_redirections() {
        let plan1 = make_plan("cat file > out.txt");
        assert!(matches!(
            validate(plan1),
            Err(SecurityError::ShellControlConstruct(_))
        ));

        let plan2 = make_plan("cat file >> out.txt");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::ShellControlConstruct(_))
        ));

        let plan3 = make_plan("cat < in.txt");
        assert!(matches!(
            validate(plan3),
            Err(SecurityError::ShellControlConstruct(_))
        ));

        let plan4 = make_plan("cmd 2> err.log");
        assert!(matches!(
            validate(plan4),
            Err(SecurityError::ShellControlConstruct(_))
        ));
    }

    // --- Dangerous Paths Rejection Tests ---

    #[test]
    fn test_reject_root_target() {
        let plan = make_plan("ls /");
        let err = validate(plan).unwrap_err();
        assert!(matches!(err, SecurityError::DangerousPath(_)));

        let plan2 = make_plan("ls /*");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::DangerousPath(_))
        ));
    }

    #[test]
    fn test_reject_directory_traversal() {
        let plan = make_plan("cat ../secret.txt");
        let err = validate(plan).unwrap_err();
        assert!(matches!(err, SecurityError::DangerousPath(_)));

        let plan2 = make_plan("ls ..");
        assert!(matches!(
            validate(plan2),
            Err(SecurityError::DangerousPath(_))
        ));
    }

    #[test]
    fn test_reject_system_directory_paths() {
        let targets = [
            "cat /etc/passwd",
            "ls /System/Library",
            "find /Library/Preferences",
            "ls /bin",
            "ls /sbin",
            "find /usr/local",
            "cat /var/log/syslog",
        ];

        for cmd in targets {
            let plan = make_plan(cmd);
            let result = validate(plan);
            assert!(
                matches!(result, Err(SecurityError::DangerousPath(_))),
                "path should be rejected: {}",
                cmd
            );
        }
    }

    // --- Edge Cases: Empty, Whitespace, Max Length ---

    #[test]
    fn test_reject_empty_and_whitespace() {
        let plan1 = make_plan("");
        assert!(matches!(validate(plan1), Err(SecurityError::EmptyCommand)));

        let plan2 = make_plan("     ");
        assert!(matches!(validate(plan2), Err(SecurityError::EmptyCommand)));
    }

    #[test]
    fn test_reject_command_too_long() {
        let long_cmd = "a".repeat(MAX_COMMAND_LENGTH + 1);
        let plan = make_plan(&long_cmd);
        assert!(matches!(
            validate(plan),
            Err(SecurityError::CommandTooLong { .. })
        ));
    }

    // --- Type-State Enforcement Test ---

    fn mock_executor(validated: &ValidatedPlan) -> String {
        format!("Executing verified command: {}", validated.command())
    }

    #[test]
    fn test_type_state_boundary_enforcement() {
        let raw_plan = make_plan("ls");

        // The mock executor only accepts ValidatedPlan.
        // Calling mock_executor(&raw_plan) would be a COMPILE-TIME type error!
        let validated_plan = validate(raw_plan).expect("ls must validate");
        let execution_result = mock_executor(&validated_plan);
        assert_eq!(execution_result, "Executing verified command: ls");
    }
}
