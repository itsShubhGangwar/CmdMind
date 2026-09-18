use crate::intent::CommandSource;
use crate::security::ValidatedPlan;

/// Represents a validated command prepared for insertion into the macOS zsh line editor (ZLE) buffer.
///
/// Type-State Security Enforcement:
/// This struct can ONLY be constructed from a `ValidatedPlan`. Unvalidated `CommandPlan` instances
/// or raw strings cannot be converted into a `ZshBufferCommand`, maintaining the integrity
/// of the Phase 4 security boundary.
///
/// Human-in-the-Loop:
/// The command is NEVER executed automatically. It is formatted for insertion into zsh's editing
/// buffer via `print -z`, allowing the user to review, edit, or cancel before pressing Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZshBufferCommand {
    command: String,
    explanation: String,
    source: CommandSource,
}

impl ZshBufferCommand {
    /// Creates a new `ZshBufferCommand` from a security-validated plan.
    ///
    /// Preserves the exact command and explanation without mutation or regeneration.
    pub fn from_validated(plan: ValidatedPlan) -> Self {
        Self {
            command: plan.command().to_string(),
            explanation: plan.explanation().to_string(),
            source: plan.source().clone(),
        }
    }

    /// Accessor for the exact validated command string.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Accessor for the human-readable explanation.
    pub fn explanation(&self) -> &str {
        &self.explanation
    }

    /// Accessor for the command's origin source.
    pub fn source(&self) -> &CommandSource {
        &self.source
    }

    /// Formats the command as a canonical zsh `print -z` statement.
    ///
    /// In macOS zsh, `print -z "<command>"` is a shell builtin that places the specified
    /// text directly into the interactive command-line editing buffer (ZLE).
    ///
    /// The string is escaped using single quotes to protect all arguments from shell expansion.
    /// Internal single quotes are safely escaped via `'\''`.
    #[allow(dead_code)]
    pub fn to_print_z_statement(&self) -> String {
        let escaped = self.command.replace('\'', "'\\''");
        format!("print -z '{}'", escaped)
    }

    /// Returns the human review readiness status for the zsh buffer.
    pub fn review_status(&self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            "Ready for human review in macOS zsh buffer"
        }
        #[cfg(not(target_os = "macos"))]
        {
            "Ready for human review"
        }
    }
}

/// Prepares a security-validated plan for zsh line editor buffer insertion.
///
/// This function enforces the Phase 4 Type-State security boundary by requiring
/// a `ValidatedPlan` argument. It is impossible to pass an unvalidated `CommandPlan`.
pub fn prepare_for_zsh(plan: ValidatedPlan) -> ZshBufferCommand {
    ZshBufferCommand::from_validated(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::CommandPlan;
    use crate::security::validate;

    fn create_validated_plan(cmd: &str) -> ValidatedPlan {
        let plan = CommandPlan::new(cmd, "Test explanation", CommandSource::Tier1);
        validate(plan).expect("command should be valid")
    }

    #[test]
    fn test_validated_ls_to_zsh_buffer() {
        let validated = create_validated_plan("ls");
        let buffer_cmd = prepare_for_zsh(validated);

        assert_eq!(buffer_cmd.command(), "ls");
        assert_eq!(buffer_cmd.explanation(), "Test explanation");
        assert_eq!(buffer_cmd.source(), &CommandSource::Tier1);
        assert_eq!(buffer_cmd.to_print_z_statement(), "print -z 'ls'");
    }

    #[test]
    fn test_validated_find_pdf_to_zsh_buffer() {
        let validated = create_validated_plan("find . -type f -name '*.pdf'");
        let buffer_cmd = prepare_for_zsh(validated);

        assert_eq!(buffer_cmd.command(), "find . -type f -name '*.pdf'");
        assert_eq!(
            buffer_cmd.to_print_z_statement(),
            r#"print -z 'find . -type f -name '\''*.pdf'\'''"#
        );
    }

    #[test]
    fn test_preserves_command_contents_exactly() {
        let original_command = "find . -type f -name '*.py' -mtime -2";
        let plan = CommandPlan::new(
            original_command,
            "Find recent Python files",
            CommandSource::Ollama,
        );
        let validated = validate(plan).expect("plan must validate");

        let buffer_cmd = prepare_for_zsh(validated);

        // Verify that the command string was not mutated, truncated, or regenerated
        assert_eq!(buffer_cmd.command(), original_command);
        assert_eq!(buffer_cmd.source(), &CommandSource::Ollama);
    }

    #[test]
    fn test_to_print_z_statement_escaping() {
        let plan = CommandPlan::new(
            "git commit -m 'initial commit'",
            "Git commit",
            CommandSource::Tier1,
        );
        let validated = validate(plan).expect("plan must validate");
        let buffer_cmd = prepare_for_zsh(validated);

        assert_eq!(
            buffer_cmd.to_print_z_statement(),
            r#"print -z 'git commit -m '\''initial commit'\'''"#
        );
    }

    #[test]
    fn test_review_status_is_available() {
        let validated = create_validated_plan("git status");
        let buffer_cmd = prepare_for_zsh(validated);
        assert!(!buffer_cmd.review_status().is_empty());
    }

    // --- Type-State Enforcement Test ---

    fn mock_zsh_buffer_consumer(cmd: &ZshBufferCommand) -> String {
        format!("Loaded into ZLE buffer: {}", cmd.command())
    }

    #[test]
    fn test_type_state_boundary_prevents_raw_plan() {
        let raw_plan = CommandPlan::new("ls", "List files", CommandSource::Tier1);

        // Attempting to pass `raw_plan` directly to `prepare_for_zsh` or `ZshBufferCommand::from_validated`
        // is a compile-time type mismatch error because they strictly take `ValidatedPlan`.
        let validated = validate(raw_plan).expect("validates successfully");
        let zsh_cmd = prepare_for_zsh(validated);

        let result = mock_zsh_buffer_consumer(&zsh_cmd);
        assert_eq!(result, "Loaded into ZLE buffer: ls");
    }
}
