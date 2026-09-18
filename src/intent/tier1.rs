use std::fmt;

/// Represents the origin or tier that generated a command plan.
/// Designed cleanly so that future tiers (such as Ollama) can be added as variants
/// without restructuring the application architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSource {
    Tier1,
    Ollama,
}

impl fmt::Display for CommandSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandSource::Tier1 => write!(f, "Tier-1"),
            CommandSource::Ollama => write!(f, "Ollama"),
        }
    }
}

/// Represents a generated shell command plan produced by an intent engine.
/// Holds the command strictly as inert data—commands are never executed in Phase 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPlan {
    /// The generated shell command to be displayed
    pub command: String,
    /// Human-readable explanation / intent description (e.g., "Find PDF files")
    pub explanation: String,
    /// Origin of the command plan
    pub source: CommandSource,
}

impl CommandPlan {
    /// Creates a new `CommandPlan` instance.
    pub fn new(
        command: impl Into<String>,
        explanation: impl Into<String>,
        source: CommandSource,
    ) -> Self {
        Self {
            command: command.into(),
            explanation: explanation.into(),
            source,
        }
    }
}

/// Normalizes a raw natural-language request by:
/// 1. Splitting on whitespace to collapse multiple spaces, tabs, or newlines into single spaces.
/// 2. Trimming leading and trailing whitespace.
/// 3. Converting all characters to lowercase for case-insensitive matching.
pub fn normalize_request(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
        .to_lowercase()
}

/// Matches a natural-language request against known Tier-1 deterministic patterns.
/// Returns `Some(CommandPlan)` if recognized, or `None` if unrecognized.
pub fn match_intent(raw_request: &str) -> Option<CommandPlan> {
    let normalized = normalize_request(raw_request);

    match normalized.as_str() {
        // 1. List files
        "list files" | "show files" | "list all files" => {
            Some(CommandPlan::new("ls", "List files", CommandSource::Tier1))
        }

        // 2. Git status
        "show git status" | "check git status" | "git status" => Some(CommandPlan::new(
            "git status",
            "Git status",
            CommandSource::Tier1,
        )),

        // 3. Find PDF files
        "find pdf files" | "find all pdf files" | "search for pdf files" => Some(CommandPlan::new(
            "find . -type f -name '*.pdf'",
            "Find PDF files",
            CommandSource::Tier1,
        )),

        // 4. Find Python files
        "find python files" | "find all python files" | "search for python files" => {
            Some(CommandPlan::new(
                "find . -type f -name '*.py'",
                "Find Python files",
                CommandSource::Tier1,
            ))
        }

        // Unrecognized requests are rejected cleanly without error or execution
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalization_whitespace_and_case() {
        assert_eq!(
            normalize_request("   FIND   ALL   PDF   FILES   "),
            "find all pdf files"
        );
        assert_eq!(normalize_request("\tLIST \n  FILES  "), "list files");
        assert_eq!(normalize_request(""), "");
        assert_eq!(normalize_request("   "), "");
    }

    #[test]
    fn test_list_files_phrasings() {
        let phrasings = ["list files", "show files", "list all files"];
        for phrasing in phrasings {
            let plan =
                match_intent(phrasing).unwrap_or_else(|| panic!("failed to match: {}", phrasing));
            assert_eq!(plan.command, "ls");
            assert_eq!(plan.explanation, "List files");
            assert_eq!(plan.source, CommandSource::Tier1);
        }
    }

    #[test]
    fn test_git_status_phrasings() {
        let phrasings = ["show git status", "check git status", "git status"];
        for phrasing in phrasings {
            let plan =
                match_intent(phrasing).unwrap_or_else(|| panic!("failed to match: {}", phrasing));
            assert_eq!(plan.command, "git status");
            assert_eq!(plan.explanation, "Git status");
            assert_eq!(plan.source, CommandSource::Tier1);
        }
    }

    #[test]
    fn test_find_pdf_files_phrasings() {
        let phrasings = [
            "find pdf files",
            "find all pdf files",
            "search for pdf files",
        ];
        for phrasing in phrasings {
            let plan =
                match_intent(phrasing).unwrap_or_else(|| panic!("failed to match: {}", phrasing));
            assert_eq!(plan.command, "find . -type f -name '*.pdf'");
            assert_eq!(plan.explanation, "Find PDF files");
            assert_eq!(plan.source, CommandSource::Tier1);
        }
    }

    #[test]
    fn test_find_python_files_phrasings() {
        let phrasings = [
            "find python files",
            "find all python files",
            "search for python files",
        ];
        for phrasing in phrasings {
            let plan =
                match_intent(phrasing).unwrap_or_else(|| panic!("failed to match: {}", phrasing));
            assert_eq!(plan.command, "find . -type f -name '*.py'");
            assert_eq!(plan.explanation, "Find Python files");
            assert_eq!(plan.source, CommandSource::Tier1);
        }
    }

    #[test]
    fn test_uppercase_input() {
        assert!(match_intent("FIND ALL PDF FILES").is_some());
        assert!(match_intent("SHOW GIT STATUS").is_some());
        assert!(match_intent("LIST FILES").is_some());
        assert!(match_intent("FIND PYTHON FILES").is_some());
    }

    #[test]
    fn test_extra_whitespace_tolerance() {
        assert!(match_intent("   find   all   pdf   files   ").is_some());
        assert!(match_intent("  show \t git   status \n ").is_some());
        assert!(match_intent(" \t list   files   ").is_some());
    }

    #[test]
    fn test_unknown_requests() {
        assert!(match_intent("find all files modified yesterday").is_none());
        assert!(match_intent("some unsupported request").is_none());
        assert!(match_intent("delete all files").is_none());
        assert!(match_intent("whoami").is_none());
    }

    #[test]
    fn test_empty_requests() {
        assert!(match_intent("").is_none());
        assert!(match_intent("   ").is_none());
    }

    #[test]
    fn test_command_source_display() {
        assert_eq!(format!("{}", CommandSource::Tier1), "Tier-1");
        assert_eq!(format!("{}", CommandSource::Ollama), "Ollama");
    }
}
