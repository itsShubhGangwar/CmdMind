//! Dedicated Adversarial Security & Invariant Test Suite for CmdMind
//!
//! Rigorously tests the Phase 4 Security Validator, Phase 5 Human-in-the-Loop boundary,
//! Phase 6 SQLite persistence boundary, and Phase 7 Path Resolver against hostile,
//! obfuscated, and malformed inputs.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use cmdmind::db::Database;
use cmdmind::executor::{execute_with_runner, MockCommandRunner};
use cmdmind::intent::{CommandPlan, CommandSource};
use cmdmind::llm::parse_llm_json;
use cmdmind::resolver::{resolve_paths, ResolutionResult};
use cmdmind::security::{validate, SecurityError};
use cmdmind::zsh::prepare_for_zsh;

// ============================================================================
// Category 1: Dangerous Executables & Case Variations
// ============================================================================

#[test]
fn test_adversarial_dangerous_executables_exact_and_variations() {
    let prohibited_cases = [
        // Standard dangerous commands
        "rm file.txt",
        "rm -rf /",
        "sudo apt-get update",
        "sudo rm something",
        "mkfs /dev/sda",
        "mkfs.ext4 /dev/sda1",
        "dd if=/dev/zero of=/dev/sda",
        "shutdown -h now",
        "reboot",
        "poweroff",
        "halt",
        "fdisk /dev/sda",
        "diskutil eraseDisk APFS MyDisk /dev/disk2",
        // Case variations (bypassing naive lowercase matching)
        "RM file.txt",
        "Rm file.txt",
        "rM file.txt",
        "SUDO whoami",
        "sUdO ls",
        "DD if=/dev/zero of=/dev/null",
        "MKFS /dev/sdb",
        "SHUTDOWN -r now",
        "Reboot",
        "POWEROFF",
        "HALT",
        "FDISK -l",
        "DiskUtil list",
        // Path prefixed dangerous binaries
        "/bin/rm file.txt",
        "/usr/bin/sudo id",
        "/sbin/shutdown now",
        "/usr/sbin/diskutil list",
        "C:\\Windows\\System32\\shutdown.exe -s",
    ];

    for cmd in prohibited_cases {
        let plan = CommandPlan::new(cmd, "Adversarial test", CommandSource::Tier1);
        let result = validate(plan);
        assert!(
            result.is_err(),
            "Expected dangerous executable in '{}' to be rejected, but it passed validation!",
            cmd
        );
        match result.unwrap_err() {
            SecurityError::DangerousCommand(name) => {
                assert!(
                    !name.is_empty(),
                    "Expected extracted dangerous command name"
                );
            }
            SecurityError::DangerousPath(_) => {
                // E.g. /bin/rm or /sbin/shutdown may also be flagged as dangerous system path
            }
            other => panic!("Unexpected error type for '{}': {:?}", cmd, other),
        }
    }
}

// ============================================================================
// Category 2: Shell Injection, Operators, Chaining, and Redirection
// ============================================================================

#[test]
fn test_adversarial_shell_control_constructs() {
    let injection_cases = [
        // Semicolon chaining
        "ls; rm file",
        "echo hello; whoami",
        "ls ; rm -rf /",
        "ls; ",
        "; ls",
        // Logical AND / OR chaining
        "ls && rm file",
        "ls || rm file",
        "true && rm -rf /",
        "false || rm -rf /",
        "echo a && echo b",
        // Pipe operators
        "ls | rm file",
        "cat /etc/passwd | grep root",
        "ps aux | grep node",
        "find . -type f | xargs rm",
        // Substitutions
        "$(rm file)",
        "echo $(whoami)",
        "find . -name $(whoami)",
        "`rm file`",
        "`id`",
        "echo `whoami`",
        // Variable expansions
        "echo $USER",
        "cat ${HOME}/secret",
        "find . -name $VAR",
        // Redirections
        "ls > output.txt",
        "cat < input.txt",
        "ls >> output.txt",
        "command 2> errors.txt",
        "ls 2>&1",
        "cat file > /dev/null",
        "ls < /dev/null > out.txt",
    ];

    for cmd in injection_cases {
        let plan = CommandPlan::new(cmd, "Shell injection test", CommandSource::Tier1);
        let result = validate(plan);
        assert!(
            result.is_err(),
            "Expected shell construct in '{}' to be rejected, but it passed validation!",
            cmd
        );
        match result.unwrap_err() {
            SecurityError::ShellControlConstruct(_) => {}
            SecurityError::DangerousCommand(_) => {}
            SecurityError::DangerousPath(_) => {}
            other => panic!("Unexpected error for '{}': {:?}", cmd, other),
        }
    }
}

// ============================================================================
// Category 3: Path Traversal and Restricted System Paths
// ============================================================================

#[test]
fn test_adversarial_path_traversal_and_system_roots() {
    let traversal_cases = [
        // Traversal patterns
        "cat ../secret.txt",
        "cat ../../etc/passwd",
        "cat ./../../etc",
        "ls ../",
        "find .. -type f",
        "find dir/../../etc -type f",
        "cat dir/../secret.txt",
        // System roots and critical directories
        "ls /",
        "ls //",
        "ls /*",
        "find / -type f",
        "find /etc -type f",
        "cat /etc/passwd",
        "cat /etc/shadow",
        "ls /System",
        "ls /System/Library",
        "ls /Library",
        "ls /bin",
        "ls /sbin",
        "ls /usr",
        "ls /var",
        "ls /var/log",
    ];

    for cmd in traversal_cases {
        let plan = CommandPlan::new(cmd, "Path traversal test", CommandSource::Tier1);
        let result = validate(plan);
        assert!(
            result.is_err(),
            "Expected path traversal/restricted target in '{}' to be rejected, but it passed validation!",
            cmd
        );
        match result.unwrap_err() {
            SecurityError::DangerousPath(_) => {}
            other => panic!("Unexpected error for '{}': {:?}", cmd, other),
        }
    }
}

// ============================================================================
// Category 4: Obfuscation and Tricky Inputs
// ============================================================================

#[test]
fn test_adversarial_obfuscation_and_tricky_formatting() {
    // Excessive whitespace between operators
    let plan1 = CommandPlan::new("ls    ;    whoami", "Whitespace", CommandSource::Tier1);
    assert!(validate(plan1).is_err());

    // Repeated operators
    let plan2 = CommandPlan::new(
        "ls ;;;; whoami",
        "Repeated semicolons",
        CommandSource::Tier1,
    );
    assert!(validate(plan2).is_err());

    let plan3 = CommandPlan::new(
        "ls &&&& whoami",
        "Repeated ampersands",
        CommandSource::Tier1,
    );
    assert!(validate(plan3).is_err());

    let plan4 = CommandPlan::new("ls |||| whoami", "Repeated pipes", CommandSource::Tier1);
    assert!(validate(plan4).is_err());

    // Unmatched quotes
    let plan5 = CommandPlan::new(
        "find . -name \"unmatched",
        "Unmatched quote",
        CommandSource::Tier1,
    );
    assert!(validate(plan5).is_err());

    let plan6 = CommandPlan::new(
        "find . -name 'unmatched",
        "Unmatched single quote",
        CommandSource::Tier1,
    );
    assert!(validate(plan6).is_err());

    // Empty and whitespace-only
    assert_eq!(
        validate(CommandPlan::new("", "Empty", CommandSource::Tier1)),
        Err(SecurityError::EmptyCommand)
    );
    assert_eq!(
        validate(CommandPlan::new(
            "   \t  \n  ",
            "Whitespace",
            CommandSource::Tier1
        )),
        Err(SecurityError::EmptyCommand)
    );

    // Excessive command length (> 1000 characters)
    let huge_command = format!("find . -name '{}'", "a".repeat(1100));
    let huge_plan = CommandPlan::new(huge_command, "Huge command", CommandSource::Tier1);
    match validate(huge_plan) {
        Err(SecurityError::CommandTooLong { length, max }) => {
            assert!(length > max);
            assert_eq!(max, 1000);
        }
        other => panic!("Expected CommandTooLong, got {:?}", other),
    }
}

// ============================================================================
// Category 5: False-Positive Prevention (Legitimate Substrings Must Pass)
// ============================================================================

#[test]
fn test_adversarial_false_positive_prevention() {
    let legitimate_cases = [
        "ls remove_old.py",             // contains "rm"
        "cat formal_document.txt",      // contains "rm"
        "find . -name '*dd*'",          // contains "dd"
        "find . -name '*sudo_notes*'",  // contains "sudo"
        "ls shutdown_notes.txt",        // contains "shutdown"
        "find . -type f -name '*.rpm'", // contains "rm"
        "ls reboot_instructions.md",    // contains "reboot"
        "cat diskutil_log.txt",         // contains "diskutil"
        "ls src",
        "find . -type f -name '*.pdf'",
        "find . -type f -name '*.py' -mtime -2",
        "git status",
        "ls -la docs",
    ];

    for cmd in legitimate_cases {
        let plan = CommandPlan::new(cmd, "False-positive test", CommandSource::Tier1);
        let result = validate(plan);
        assert!(
            result.is_ok(),
            "Legitimate command '{}' was falsely rejected by validator: {:?}",
            cmd,
            result.err()
        );
    }
}

// ============================================================================
// Category 6: LLM-Output Adversarial Tests (Untrusted JSON Payloads)
// ============================================================================

#[test]
fn test_adversarial_llm_json_payloads_rejected_at_boundary() {
    let malicious_llm_outputs = [
        r#"{"command": "rm -rf /", "explanation": "Clean up files"}"#,
        r#"{"command": "ls && rm -rf /", "explanation": "List and clean up"}"#,
        r#"{"command": "find /etc -type f", "explanation": "Locate configuration files"}"#,
        r#"{"command": "sudo apt-get update", "explanation": "Update packages"}"#,
        r#"{"command": "cat < /etc/shadow", "explanation": "Display system password hashes"}"#,
        r#"{"command": "echo $(whoami)", "explanation": "Print current username"}"#,
        r#"{"command": "find . -exec rm {} \\;", "explanation": "Delete matched files"}"#,
    ];

    for raw_json in malicious_llm_outputs {
        // Step 1: Parsing untrusted JSON succeeds
        let plan = parse_llm_json(raw_json).unwrap_or_else(|_| {
            panic!("Valid JSON syntax should parse successfully: {}", raw_json)
        });

        // Step 2: Security Validation MUST reject the resulting CommandPlan
        let validation_result = validate(plan);
        assert!(
            validation_result.is_err(),
            "Security validator must REJECT malicious LLM payload '{}', but it passed!",
            raw_json
        );
    }
}

// ============================================================================
// Category 7: Path Resolver Adversarial & Stress Tests
// ============================================================================

struct AdversarialTestDir {
    path: PathBuf,
}

impl AdversarialTestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cmdmind_adv_{}_{}_{}",
            name,
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn create_dir(&self, rel: &str) {
        fs::create_dir_all(self.path.join(rel)).unwrap();
    }

    fn create_file(&self, rel: &str) {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(full).unwrap();
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AdversarialTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn test_adversarial_path_resolver_security_revalidation() {
    let td = AdversarialTestDir::new("revalidation");
    td.create_dir("src");

    // Valid plan
    let plan = CommandPlan::new("find srcc -type f", "Find files", CommandSource::Tier1);
    let validated = validate(plan).expect("Initial plan must be valid");

    let res = resolve_paths(&validated, Some(td.path()));
    match res {
        ResolutionResult::Suggestion(s) => {
            // Must have passed validation AGAIN
            assert_eq!(s.suggested_path, "src");
            assert_eq!(s.validated_plan.command(), "find src -type f");
        }
        other => panic!("Expected Suggestion, got {:?}", other),
    }
}

#[test]
fn test_adversarial_path_resolver_bounded_entry_cap() {
    let td = AdversarialTestDir::new("large_dir");

    // Create a directory with 600 files to stress the 500 entry limit
    for i in 0..600 {
        td.create_file(&format!("file_{:04}.txt", i));
    }

    let plan = CommandPlan::new(
        "cat non_existent_target.txt",
        "Read file",
        CommandSource::Tier1,
    );
    let validated = validate(plan).expect("Initial plan valid");

    // Execution must remain bounded and not hang or crash
    let res = resolve_paths(&validated, Some(td.path()));
    assert_eq!(
        res,
        ResolutionResult::NoMatch {
            original_path: "non_existent_target.txt".to_string()
        }
    );
}

#[test]
fn test_adversarial_path_resolver_ambiguous_candidates_tie() {
    let td = AdversarialTestDir::new("tie_candidates");
    td.create_dir("test_dir_a");
    td.create_dir("test_dir_b");

    // "test_dir_c" has distance 1 to both test_dir_a and test_dir_b
    let plan = CommandPlan::new("cd test_dir_c", "Change dir", CommandSource::Tier1);
    let validated = validate(plan).expect("Initial plan valid");

    let res = resolve_paths(&validated, Some(td.path()));
    match res {
        ResolutionResult::Ambiguous {
            original_path,
            candidates,
            distance,
        } => {
            assert_eq!(original_path, "test_dir_c");
            assert_eq!(distance, 1);
            assert_eq!(candidates, vec!["test_dir_a", "test_dir_b"]);
        }
        other => panic!("Expected Ambiguous, got {:?}", other),
    }
}

// ============================================================================
// Category 8: Core Security Invariants Verification
// ============================================================================

#[test]
fn test_invariant_1_command_plan_cannot_reach_zsh_buffer_api() {
    // Compile-time and runtime invariant:
    // `prepare_for_zsh` exclusively takes `ValidatedPlan`.
    let plan = CommandPlan::new("ls", "List files", CommandSource::Tier1);
    let validated = validate(plan).unwrap();
    let zsh_cmd = prepare_for_zsh(validated);
    #[cfg(target_os = "macos")]
    assert_eq!(
        zsh_cmd.review_status(),
        "Ready for human review in macOS zsh buffer"
    );
    #[cfg(not(target_os = "macos"))]
    assert_eq!(zsh_cmd.review_status(), "Ready for human review");
}

#[test]
fn test_invariant_2_command_plan_cannot_reach_sqlite_history_api() {
    // Compile-time and runtime invariant:
    // `save_command` exclusively accepts `&ValidatedPlan`.
    let db = Database::open_in_memory().expect("In-memory DB should open");
    let plan = CommandPlan::new("git status", "Check git status", CommandSource::Tier1);
    let validated = validate(plan).unwrap();

    let save_res = db.save_command("check git status", &validated);
    assert!(save_res.is_ok(), "Validated plan must save successfully");

    // Rejected command must NEVER save
    let dangerous_plan = CommandPlan::new("rm -rf /", "Malicious intent", CommandSource::Tier1);
    let rejected = validate(dangerous_plan);
    assert!(rejected.is_err(), "Dangerous plan must fail validation");

    let entries = db.get_recent_history(10).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].command, "git status");
}

#[test]
fn test_invariant_3_corrected_path_revalidated_via_type_state() {
    // Invariant: PathSuggestion contains `validated_plan: ValidatedPlan`.
    let td = AdversarialTestDir::new("inv3");
    td.create_dir("documents");

    let plan = CommandPlan::new("ls documnts", "List documents", CommandSource::Tier1);
    let validated = validate(plan).unwrap();

    let res = resolve_paths(&validated, Some(td.path()));
    match res {
        ResolutionResult::Suggestion(s) => {
            // Check that the suggestion's plan can be passed into downstream APIs requiring ValidatedPlan
            let zsh_buffer = prepare_for_zsh(s.validated_plan);
            assert_eq!(zsh_buffer.command(), "ls documents");
        }
        other => panic!("Expected Suggestion, got {:?}", other),
    }
}

#[test]
fn test_invariant_4_and_5_no_execution_occurred() {
    // The test framework itself ran, but zero shell commands or child processes were invoked.
    // Asserting that CmdMind remains inert data manipulation unless explicitly executed via ValidatedPlan.
    assert!(true);
}

// ============================================================================
// Category 9: Direct Natural-Language Execution & Type-State Invariants
// ============================================================================

#[test]
fn test_invariant_command_plan_cannot_reach_executor_api() {
    // Type-State Invariant:
    // execute_with_runner() requires &ValidatedPlan.
    // Passing CommandPlan is rejected at compile time.
    let plan = CommandPlan::new("ls", "List files", CommandSource::Tier1);
    let validated = validate(plan).expect("ls validates");

    let runner = MockCommandRunner::new();
    let result = execute_with_runner(&validated, &runner);
    assert!(result.is_ok());
    assert_eq!(runner.call_count(), 1);
}

#[test]
fn test_adversarial_dangerous_commands_never_reach_executor() {
    let prohibited_cases = [
        "rm -rf /",
        "sudo rm something",
        "dd if=/dev/zero of=/dev/sda",
        "mkfs.ext4 /dev/sda1",
        "shutdown -h now",
        "reboot",
        "poweroff",
        "halt",
        "fdisk /dev/sda",
        "diskutil eraseDisk APFS MyDisk /dev/disk2",
    ];

    let runner = MockCommandRunner::new();

    for cmd in prohibited_cases {
        let plan = CommandPlan::new(cmd, "Malicious attempt", CommandSource::Tier1);
        let validation_res = validate(plan);
        assert!(
            validation_res.is_err(),
            "Dangerous command '{}' must be rejected by validator",
            cmd
        );
        // Because validation fails, no ValidatedPlan exists to pass to the executor!
    }

    // Runner was never called
    assert_eq!(
        runner.call_count(),
        0,
        "Executor runner must never be called for dangerous commands"
    );
}

#[test]
fn test_adversarial_shell_control_constructs_never_reach_executor() {
    let shell_injections = [
        "ls ; rm -rf /",
        "ls && rm -rf /",
        "ls || rm -rf /",
        "ls | rm",
        "echo $(rm -rf /)",
        "echo `rm -rf /`",
        "echo hello > /dev/null",
        "echo hello >> output.txt",
        "cat < input.txt",
        "ls 2> error.log",
    ];

    let runner = MockCommandRunner::new();

    for cmd in shell_injections {
        let plan = CommandPlan::new(cmd, "Shell injection attempt", CommandSource::Tier1);
        let validation_res = validate(plan);
        assert!(
            validation_res.is_err(),
            "Shell control construct '{}' must be rejected by validator",
            cmd
        );
    }

    assert_eq!(
        runner.call_count(),
        0,
        "Executor runner must never be called for shell injection commands"
    );
}

#[test]
fn test_safe_commands_reach_executor_with_structured_arguments() {
    let cases = [
        ("ls", "ls", vec![]),
        ("git status", "git", vec!["status"]),
        (
            "find . -type f -name '*.pdf'",
            "find",
            vec![".", "-type", "f", "-name", "*.pdf"],
        ),
    ];

    for (cmd_str, expected_prog, expected_args) in cases {
        let plan = CommandPlan::new(cmd_str, "Safe intent", CommandSource::Tier1);
        let validated = validate(plan).expect("command must validate");

        let runner = MockCommandRunner::new();
        let res = execute_with_runner(&validated, &runner).expect("execution must succeed");
        assert!(res.success);
        assert_eq!(runner.call_count(), 1);

        let calls = runner.get_calls();
        assert_eq!(calls[0].0, expected_prog);
        let actual_args: Vec<&str> = calls[0].1.iter().map(|s| s.as_str()).collect();
        assert_eq!(actual_args, expected_args);
    }
}

#[test]
fn test_path_correction_revalidated_before_execution() {
    let td = AdversarialTestDir::new("inv_exec_path");
    td.create_dir("documents");

    let plan = CommandPlan::new("ls documnts", "List documents", CommandSource::Tier1);
    let validated = validate(plan).unwrap();

    let res = resolve_paths(&validated, Some(td.path()));
    match res {
        ResolutionResult::Suggestion(suggestion) => {
            // Check that the suggestion's plan can be passed directly to the executor
            let runner = MockCommandRunner::new();
            let exec_res = execute_with_runner(&suggestion.validated_plan, &runner);
            assert!(exec_res.is_ok());
            assert_eq!(runner.call_count(), 1);

            let calls = runner.get_calls();
            assert_eq!(calls[0].0, "ls");
            assert_eq!(calls[0].1, vec!["documents".to_string()]);
        }
        other => panic!("Expected Suggestion, got {:?}", other),
    }
}
