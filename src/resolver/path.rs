use std::fs;
use std::path::{Path, PathBuf};

use crate::intent::CommandPlan;
use crate::resolver::levenshtein::levenshtein;
use crate::security::validator::{validate, ValidatedPlan};

/// Maximum number of filesystem entries scanned in a candidate directory.
/// Ensures filesystem traversal remains strictly bounded and does not exhaust resources.
pub const MAX_DIRECTORY_ENTRIES: usize = 500;

/// Represents a validated path correction suggestion.
///
/// Type-State Security Invariant:
/// `validated_plan` is of type `ValidatedPlan`, proving that the corrected command
/// was independently subjected to security validation and passed before being
/// offered as a suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSuggestion {
    pub original_path: String,
    pub suggested_path: String,
    pub distance: usize,
    pub validated_plan: ValidatedPlan,
}

/// The result of running the auto-healing path resolver on a validated command plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionResult {
    /// The referenced path already exists on disk, or no path argument was present.
    NoCorrectionNeeded,
    /// A single close existing path was found and the corrected command was successfully validated.
    Suggestion(PathSuggestion),
    /// Multiple existing paths have identical minimum edit distance; ambiguous to resolve automatically.
    Ambiguous {
        original_path: String,
        candidates: Vec<String>,
        distance: usize,
    },
    /// The path does not exist, and no existing path met the similarity threshold.
    NoMatch { original_path: String },
    /// The command pattern is not one of the supported single-path forms, or cannot be confidently parsed.
    UnsupportedCommand,
    /// A candidate correction was found, but the resulting corrected command was rejected by security policy.
    CorrectionRejected {
        original_path: String,
        suggested_path: String,
        reason: String,
    },
}

/// Represents a parsed token with its byte span in the original command string.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandToken {
    text: String,
    span: (usize, usize),
    is_quoted: bool,
}

/// Identifies an extracted path target argument from a command.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExtractedPath {
    original_path: String,
    span: (usize, usize),
    is_quoted: bool,
}

/// Outcome of parsing a command for path target arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetExtraction {
    Path(ExtractedPath),
    ImplicitCurrentDir,
    Unsupported,
}

/// Inspects a `ValidatedPlan` and determines if referenced filesystem paths exist.
///
/// If a referenced path does not exist, computes Levenshtein distances against immediate
/// directory entries to detect likely typos and generates a validated correction suggestion.
///
/// # Security Guarantees
/// - Never mutates the incoming `ValidatedPlan`.
/// - Never automatically executes any command.
/// - The corrected command string is packaged into a new untrusted `CommandPlan` and
///   re-validated through `security::validator::validate()`.
/// - Traversal is strictly non-recursive and bounded to immediate candidate parent directories.
pub fn resolve_paths(plan: &ValidatedPlan, working_dir: Option<&Path>) -> ResolutionResult {
    let current_dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => PathBuf::from("."),
    };
    let base_dir = working_dir.unwrap_or(&current_dir);

    // 1. Tokenize command with byte spans
    let tokens = match tokenize_with_spans(plan.command()) {
        Some(t) if !t.is_empty() => t,
        _ => return ResolutionResult::UnsupportedCommand,
    };

    // 2. Extract path argument for supported command patterns
    let extracted = match extract_path_target(&tokens) {
        TargetExtraction::Path(target) => target,
        TargetExtraction::ImplicitCurrentDir => return ResolutionResult::NoCorrectionNeeded,
        TargetExtraction::Unsupported => return ResolutionResult::UnsupportedCommand,
    };

    let original_path = extracted.original_path.trim().to_string();
    if original_path.is_empty() || original_path == "." {
        return ResolutionResult::NoCorrectionNeeded;
    }

    // Do NOT scan whole filesystem roots for absolute paths (bounded scope)
    if Path::new(&original_path).is_absolute() {
        let direct_path = PathBuf::from(&original_path);
        if direct_path.exists() {
            return ResolutionResult::NoCorrectionNeeded;
        }
        return ResolutionResult::NoMatch { original_path };
    }

    // 3. Determine search directory and target filename
    let has_trailing_slash = original_path.ends_with('/') || original_path.ends_with('\\');
    let clean_path = original_path.trim_end_matches(['/', '\\']);
    let path_obj = Path::new(clean_path);

    let target_name = match path_obj.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => return ResolutionResult::NoMatch { original_path },
    };

    let (parent_prefix, search_dir) = match path_obj.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            let mut prefix = parent.to_string_lossy().to_string();
            if !prefix.ends_with('/') && !prefix.ends_with('\\') {
                prefix.push('/');
            }
            (prefix, base_dir.join(parent))
        }
        _ => {
            let prefix = if original_path.starts_with("./") {
                "./".to_string()
            } else if original_path.starts_with(".\\") {
                ".\\".to_string()
            } else {
                String::new()
            };
            (prefix, base_dir.to_path_buf())
        }
    };

    if !search_dir.is_dir() {
        return ResolutionResult::NoMatch { original_path };
    }

    // 4. Check if the path exists with exact casing
    let direct_path = base_dir.join(&original_path);
    if direct_path.exists() {
        // On case-insensitive filesystems (such as Windows and macOS default APFS),
        // direct_path.exists() can return true even if the case does not match the actual entry.
        // We verify whether an entry with the exact casing exists. If exact case matches,
        // no correction is needed. If casing differs, proceed to suggest the canonical case.
        if let Ok(rd) = fs::read_dir(&search_dir) {
            let exact_match = rd
                .filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy() == target_name);
            if exact_match {
                return ResolutionResult::NoCorrectionNeeded;
            }
        } else {
            return ResolutionResult::NoCorrectionNeeded;
        }
    }

    // 5. Read immediate directory entries up to MAX_DIRECTORY_ENTRIES (strictly bounded)
    let read_dir = match fs::read_dir(&search_dir) {
        Ok(rd) => rd,
        Err(_) => return ResolutionResult::NoMatch { original_path },
    };

    let target_len = target_name.chars().count();
    let threshold = if target_len <= 2 {
        1
    } else {
        (target_len / 3).max(2)
    };

    let mut candidates: Vec<(String, bool, usize)> = Vec::new();
    let mut scanned_count = 0;

    for entry_res in read_dir {
        if scanned_count >= MAX_DIRECTORY_ENTRIES {
            break;
        }
        scanned_count += 1;

        if let Ok(entry) = entry_res {
            let entry_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden entries unless user explicitly queried a hidden path
            if entry_name.starts_with('.') && !target_name.starts_with('.') {
                continue;
            }

            let dist = path_edit_distance(&target_name, &entry_name);
            if dist > 0 && dist <= threshold {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                candidates.push((entry_name, is_dir, dist));
            }
        }
    }

    if candidates.is_empty() {
        return ResolutionResult::NoMatch { original_path };
    }

    // 6. Find candidates with minimum edit distance
    let min_dist = candidates.iter().map(|(_, _, d)| *d).min().unwrap();
    let mut best_candidates: Vec<(String, bool)> = candidates
        .into_iter()
        .filter(|(_, _, d)| *d == min_dist)
        .map(|(name, is_dir, _)| (name, is_dir))
        .collect();

    best_candidates.sort_by(|a, b| a.0.cmp(&b.0));

    // Handle ambiguous matches with equal distance
    if best_candidates.len() > 1 {
        let candidate_names: Vec<String> = best_candidates.into_iter().map(|(n, _)| n).collect();
        return ResolutionResult::Ambiguous {
            original_path,
            candidates: candidate_names,
            distance: min_dist,
        };
    }

    // 7. Reconstruct suggested path
    let (chosen_name, is_dir) = &best_candidates[0];
    let mut suggested_path = format!("{}{}", parent_prefix, chosen_name);
    if has_trailing_slash || (*is_dir && original_path.ends_with('/')) {
        if !suggested_path.ends_with('/') {
            suggested_path.push('/');
        }
    }

    // 8. Construct corrected command string
    let corrected_command = replace_path_in_command(
        plan.command(),
        extracted.span,
        extracted.is_quoted,
        &suggested_path,
    );

    // 9. Re-validate the newly constructed command plan through Security Validator
    let corrected_plan = CommandPlan::new(
        corrected_command,
        format!(
            "{} (suggested path correction from '{}' to '{}')",
            plan.explanation(),
            original_path,
            suggested_path
        ),
        plan.source().clone(),
    );

    match validate(corrected_plan) {
        Ok(validated_plan) => ResolutionResult::Suggestion(PathSuggestion {
            original_path,
            suggested_path,
            distance: min_dist,
            validated_plan,
        }),
        Err(sec_err) => ResolutionResult::CorrectionRejected {
            original_path,
            suggested_path,
            reason: sec_err.to_string(),
        },
    }
}

/// Computes edit distance between two path segment names, accounting for case differences.
pub fn path_edit_distance(target: &str, candidate: &str) -> usize {
    let exact = levenshtein(target, candidate);
    let target_lower = target.to_lowercase();
    let candidate_lower = candidate.to_lowercase();
    let lower = levenshtein(&target_lower, &candidate_lower);

    if target_lower == candidate_lower {
        if target == candidate {
            0
        } else {
            1 // Case difference counts as 1 edit operation
        }
    } else {
        let case_penalty = if target != candidate
            && (target.chars().any(|c| c.is_uppercase())
                || candidate.chars().any(|c| c.is_uppercase()))
        {
            1
        } else {
            0
        };
        exact.min(lower + case_penalty)
    }
}

/// Tokenizes a command into individual tokens along with their exact byte spans in `cmd`.
fn tokenize_with_spans(cmd: &str) -> Option<Vec<CommandToken>> {
    let mut tokens = Vec::new();
    let chars: Vec<(usize, char)> = cmd.char_indices().collect();
    let mut i = 0;
    let len = chars.len();

    while i < len {
        // Skip whitespace
        while i < len && chars[i].1.is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        let start_byte = chars[i].0;
        let mut text = String::new();
        let mut is_quoted = false;

        if chars[i].1 == '\'' {
            is_quoted = true;
            i += 1;
            while i < len && chars[i].1 != '\'' {
                text.push(chars[i].1);
                i += 1;
            }
            if i >= len {
                return None; // Unmatched quote
            }
            let end_byte = if i + 1 < len {
                chars[i + 1].0
            } else {
                cmd.len()
            };
            i += 1;
            tokens.push(CommandToken {
                text,
                span: (start_byte, end_byte),
                is_quoted,
            });
        } else if chars[i].1 == '"' {
            is_quoted = true;
            i += 1;
            while i < len && chars[i].1 != '"' {
                text.push(chars[i].1);
                i += 1;
            }
            if i >= len {
                return None; // Unmatched quote
            }
            let end_byte = if i + 1 < len {
                chars[i + 1].0
            } else {
                cmd.len()
            };
            i += 1;
            tokens.push(CommandToken {
                text,
                span: (start_byte, end_byte),
                is_quoted,
            });
        } else {
            while i < len && !chars[i].1.is_whitespace() {
                text.push(chars[i].1);
                i += 1;
            }
            let end_byte = if i < len { chars[i].0 } else { cmd.len() };
            tokens.push(CommandToken {
                text,
                span: (start_byte, end_byte),
                is_quoted,
            });
        }
    }

    Some(tokens)
}

/// Identifies the single candidate path argument token for supported commands.
fn extract_path_target(tokens: &[CommandToken]) -> TargetExtraction {
    if tokens.is_empty() {
        return TargetExtraction::Unsupported;
    }

    let exe = tokens[0].text.to_lowercase();

    match exe.as_str() {
        "find" => {
            // In `find <path> [predicates...]`, the path argument is the first token after `find`
            // that does not start with an option hyphen '-'
            if tokens.len() > 1 && !tokens[1].text.starts_with('-') {
                TargetExtraction::Path(ExtractedPath {
                    original_path: tokens[1].text.clone(),
                    span: tokens[1].span,
                    is_quoted: tokens[1].is_quoted,
                })
            } else {
                TargetExtraction::ImplicitCurrentDir
            }
        }
        "ls" => {
            // In `ls [flags...] <path>`, look for a single non-flag argument
            let non_flags: Vec<&CommandToken> = tokens[1..]
                .iter()
                .filter(|t| !t.text.starts_with('-'))
                .collect();

            if non_flags.len() == 1 {
                TargetExtraction::Path(ExtractedPath {
                    original_path: non_flags[0].text.clone(),
                    span: non_flags[0].span,
                    is_quoted: non_flags[0].is_quoted,
                })
            } else if non_flags.is_empty() {
                TargetExtraction::ImplicitCurrentDir
            } else {
                TargetExtraction::Unsupported
            }
        }
        "cd" | "cat" | "head" | "tail" => {
            let non_flags: Vec<&CommandToken> = tokens[1..]
                .iter()
                .filter(|t| !t.text.starts_with('-'))
                .collect();

            if non_flags.len() == 1 {
                TargetExtraction::Path(ExtractedPath {
                    original_path: non_flags[0].text.clone(),
                    span: non_flags[0].span,
                    is_quoted: non_flags[0].is_quoted,
                })
            } else if non_flags.is_empty() {
                TargetExtraction::ImplicitCurrentDir
            } else {
                TargetExtraction::Unsupported
            }
        }
        _ => TargetExtraction::Unsupported,
    }
}

/// Replaces the target path argument within the original command string.
fn replace_path_in_command(
    original_cmd: &str,
    span: (usize, usize),
    was_quoted: bool,
    suggested_path: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&original_cmd[..span.0]);

    if suggested_path.contains(' ') || was_quoted {
        out.push('"');
        out.push_str(suggested_path);
        out.push('"');
    } else {
        out.push_str(suggested_path);
    }

    out.push_str(&original_cmd[span.1..]);
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::intent::{CommandPlan, CommandSource};
    use std::fs::{self, File};

    /// Helper struct creating a temporary test directory that cleans up on drop.
    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "cmdmind_test_{}_{}_{}",
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

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn make_validated_plan(command: &str) -> ValidatedPlan {
        let plan = CommandPlan::new(command, "Test intent", CommandSource::Tier1);
        validate(plan).expect("Initial test command plan should pass validation")
    }

    #[test]
    fn test_existing_path_needs_no_correction() {
        let td = TestDir::new("existing_path");
        td.create_dir("src");

        let plan = make_validated_plan("find src -type f");
        let res = resolve_paths(&plan, Some(td.path()));
        assert_eq!(res, ResolutionResult::NoCorrectionNeeded);

        let plan_dot = make_validated_plan("find . -type f -name '*.pdf'");
        let res_dot = resolve_paths(&plan_dot, Some(td.path()));
        assert_eq!(res_dot, ResolutionResult::NoCorrectionNeeded);
    }

    #[test]
    fn test_typo_srcc_suggests_src() {
        let td = TestDir::new("typo_srcc");
        td.create_dir("src");
        td.create_dir("docs");
        td.create_dir("tests");
        td.create_file("README.md");

        let plan = make_validated_plan("find srcc -type f");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.original_path, "srcc");
                assert_eq!(s.suggested_path, "src");
                assert_eq!(s.distance, 1);
                assert_eq!(s.validated_plan.command(), "find src -type f");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_typo_documnts_suggests_documents() {
        let td = TestDir::new("typo_documnts");
        td.create_dir("documents");

        let plan = make_validated_plan("ls -la documnts");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.original_path, "documnts");
                assert_eq!(s.suggested_path, "documents");
                assert_eq!(s.distance, 1);
                assert_eq!(s.validated_plan.command(), "ls -la documents");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_unrelated_path_gives_no_match() {
        let td = TestDir::new("unrelated_path");
        td.create_dir("src");
        td.create_dir("docs");

        let plan = make_validated_plan("find totally_unrelated_xyz_123 -type f");
        let res = resolve_paths(&plan, Some(td.path()));
        assert_eq!(
            res,
            ResolutionResult::NoMatch {
                original_path: "totally_unrelated_xyz_123".to_string()
            }
        );
    }

    #[test]
    fn test_ambiguous_candidates_reported() {
        let td = TestDir::new("ambiguous");
        td.create_dir("src");
        td.create_dir("src2");

        // "src1" has distance 1 to "src" and distance 1 to "src2"
        let plan = make_validated_plan("find src1 -type f");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Ambiguous {
                original_path,
                candidates,
                distance,
            } => {
                assert_eq!(original_path, "src1");
                assert_eq!(distance, 1);
                assert_eq!(candidates, vec!["src", "src2"]);
            }
            other => panic!("Expected Ambiguous, got {:?}", other),
        }
    }

    #[test]
    fn test_bounded_traversal_only_searches_target_parent() {
        let td = TestDir::new("bounded");
        td.create_dir("parent_a");
        td.create_file("parent_a/target_file.txt");
        td.create_dir("parent_b");
        td.create_file("parent_b/other_file.txt");

        // Target typo inside parent_a should ONLY search parent_a
        let plan = make_validated_plan("cat parent_a/target_fiel.txt");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "parent_a/target_file.txt");
                assert_eq!(s.validated_plan.command(), "cat parent_a/target_file.txt");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_correction_re_validation_rejects_dangerous_path() {
        // Test that if a suggestion were to produce a dangerous path (e.g. /etc),
        // validate() is called again and rejects it.
        let dangerous_correction = CommandPlan::new(
            "find /etc -type f",
            "Dangerous intent",
            CommandSource::Tier1,
        );

        let validation_res = validate(dangerous_correction);
        assert!(validation_res.is_err());
    }

    #[test]
    fn test_spaces_in_path_names_quoted() {
        let td = TestDir::new("spaces_path");
        td.create_dir("my documents");

        let plan = make_validated_plan("find my_documents -type f");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "my documents");
                assert_eq!(s.validated_plan.command(), "find \"my documents\" -type f");
            }
            other => panic!("Expected Suggestion with quotes, got {:?}", other),
        }
    }

    #[test]
    fn test_hyphens_and_underscores() {
        let td = TestDir::new("hyphen_underscore");
        td.create_dir("my_module");

        let plan = make_validated_plan("cd my-module");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "my_module");
                assert_eq!(s.validated_plan.command(), "cd my_module");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_unicode_filenames() {
        let td = TestDir::new("unicode_path");
        td.create_dir("crème");

        let plan = make_validated_plan("ls creme");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "crème");
                assert_eq!(s.validated_plan.command(), "ls crème");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_case_difference_resolution() {
        let td = TestDir::new("case_path");
        td.create_dir("src");

        let plan = make_validated_plan("find SRC -type f");
        let res = resolve_paths(&plan, Some(td.path()));

        match res {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "src");
                assert_eq!(s.validated_plan.command(), "find src -type f");
            }
            other => panic!("Expected Suggestion, got {:?}", other),
        }
    }

    #[test]
    fn test_filenames_containing_dangerous_substrings() {
        let td = TestDir::new("dangerous_words_path");
        td.create_file("remove.txt");
        td.create_file("sudo_notes.md");
        td.create_file("shutdown.txt");

        let plan_rm = make_validated_plan("cat remov.txt");
        let res_rm = resolve_paths(&plan_rm, Some(td.path()));
        match res_rm {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "remove.txt");
                assert_eq!(s.validated_plan.command(), "cat remove.txt");
            }
            other => panic!("Expected Suggestion for remov.txt, got {:?}", other),
        }

        let plan_sudo = make_validated_plan("cat sudo_notess.md");
        let res_sudo = resolve_paths(&plan_sudo, Some(td.path()));
        match res_sudo {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "sudo_notes.md");
                assert_eq!(s.validated_plan.command(), "cat sudo_notes.md");
            }
            other => panic!("Expected Suggestion for sudo_notes, got {:?}", other),
        }
    }

    #[test]
    fn test_dot_slash_prefix_and_trailing_slash() {
        let td = TestDir::new("prefixes");
        td.create_dir("src");

        let plan_prefix = make_validated_plan("find ./srcc -type f");
        let res_prefix = resolve_paths(&plan_prefix, Some(td.path()));
        match res_prefix {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "./src");
                assert_eq!(s.validated_plan.command(), "find ./src -type f");
            }
            other => panic!("Expected Suggestion with ./ prefix, got {:?}", other),
        }

        let plan_slash = make_validated_plan("cd srcc/");
        let res_slash = resolve_paths(&plan_slash, Some(td.path()));
        match res_slash {
            ResolutionResult::Suggestion(s) => {
                assert_eq!(s.suggested_path, "src/");
                assert_eq!(s.validated_plan.command(), "cd src/");
            }
            other => panic!("Expected Suggestion with trailing slash, got {:?}", other),
        }
    }

    #[test]
    fn test_unsupported_command_patterns() {
        let plan = make_validated_plan("git status");
        let res = resolve_paths(&plan, None);
        assert_eq!(res, ResolutionResult::UnsupportedCommand);
    }
}
