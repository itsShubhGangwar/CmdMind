use cmdmind::db::Database;
use cmdmind::executor::execute;
use cmdmind::intent::match_intent;
use cmdmind::llm::call_ollama;
use cmdmind::resolver::{resolve_paths, ResolutionResult};
use cmdmind::security::validate;
use cmdmind::zsh::prepare_for_zsh;
use std::env;
use std::process;

#[tokio::main]
async fn main() {
    // 1. Collect command-line arguments.
    let raw_args: Vec<String> = env::args().collect();

    // 2. Skip the executable path (at index 0).
    // If no arguments were supplied (raw_args.len() <= 1), display the usage guide.
    if raw_args.len() <= 1 {
        print_usage();
        return;
    }

    let mut dry_run = false;
    let mut tokens: Vec<String> = Vec::new();

    for arg in &raw_args[1..] {
        if arg == "--dry-run" {
            dry_run = true;
        } else {
            tokens.push(arg.clone());
        }
    }

    // 3. Convert command-line tokens into the final natural-language request.
    let request = tokens.join(" ").trim().to_string();

    // Gracefully handle the case where the user entered only whitespace or only --dry-run.
    if request.is_empty() {
        print_usage();
        return;
    }

    // 4. Check for 'history', '--version', or '--help' commands:
    if request.eq_ignore_ascii_case("history") {
        display_history();
        return;
    }

    if request == "--version" || request == "-V" || request == "-v" {
        println!("cmdmind {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if request == "--help" || request == "-h" || request == "help" {
        print_usage();
        return;
    }

    // Check auto-execution environment configuration
    let auto_execute = if dry_run {
        false
    } else {
        match env::var("CMDMIND_AUTO_EXECUTE") {
            Ok(val) => val != "0" && !val.eq_ignore_ascii_case("false"),
            Err(_) => true,
        }
    };

    // Display the initial header and received natural-language request.
    println!("CmdMind");
    println!("────────────────────\n");
    println!("Request: {}\n", request);

    // 5. Generate untrusted CommandPlan.
    // Flow: Tier-1 Deterministic Engine ──► (if None) ──► Local Ollama Fallback
    let plan = if let Some(tier1_plan) = match_intent(&request) {
        tier1_plan
    } else {
        match call_ollama(&request).await {
            Ok(ollama_plan) => ollama_plan,
            Err(err) => {
                eprintln!(
                    "Tier-1 engine did not recognize this request, and Ollama fallback failed:\n"
                );
                eprintln!("Error: {}", err);
                process::exit(1);
            }
        }
    };

    // 6. Security Validation Boundary
    // Both Tier-1 and Ollama outputs MUST pass through validate() to produce a ValidatedPlan.
    let validated = match validate(plan) {
        Ok(v) => v,
        Err(err) => {
            println!("Security validation rejected the generated command.\n");
            println!("Reason: {}\n", err);
            println!("Command was NOT executed.");
            process::exit(1);
        }
    };

    // 7. Auto-Healing Path Resolver
    // Detects potential filesystem typos using Levenshtein distance and suggests corrections.
    // Invariant: If a correction is suggested, the suggested command has been revalidated
    // via validate() and only that revalidated ValidatedPlan may be executed.
    let resolver_result = resolve_paths(&validated, None);
    let plan_to_execute = match &resolver_result {
        ResolutionResult::NoCorrectionNeeded => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated");
            println!("Path: No correction needed\n");
            validated
        }
        ResolutionResult::Suggestion(suggestion) => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated\n");
            println!("Path correction suggested:\n");
            println!("Original:\n{}\n", validated.command());
            println!("Suggested:\n{}\n", suggestion.validated_plan.command());
            println!(
                "Reason:\n\"{}\" does not exist; \"{}\" is the closest existing path.\n",
                suggestion.original_path, suggestion.suggested_path
            );
            println!("The suggested command was re-validated successfully.\n");
            suggestion.validated_plan.clone()
        }
        ResolutionResult::CorrectionRejected {
            original_path,
            suggested_path,
            reason,
        } => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated\n");
            println!("Path correction rejected:\n");
            println!(
                "Candidate path \"{}\" (for \"{}\") was rejected by security validation: {}.\n",
                suggested_path, original_path, reason
            );
            println!("Command was NOT executed.");
            process::exit(1);
        }
        ResolutionResult::Ambiguous {
            original_path,
            candidates,
            ..
        } => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated\n");
            println!("Path notice:\n");
            println!(
                "Path \"{}\" does not exist. Multiple similar paths were found: {}.\n",
                original_path,
                candidates.join(", ")
            );
            validated
        }
        ResolutionResult::NoMatch { original_path } => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated\n");
            println!("Path notice:\n");
            println!(
                "Path \"{}\" does not exist (no close candidate found).\n",
                original_path
            );
            validated
        }
        ResolutionResult::UnsupportedCommand => {
            println!("Intent: {}\n", validated.explanation());
            println!("Generated command:\n{}\n", validated.command());
            println!("Source: {}", validated.source());
            println!("Security: Validated\n");
            validated
        }
    };

    // 8. Execution or Dry-Run
    if !auto_execute {
        let zsh_cmd = prepare_for_zsh(plan_to_execute.clone());
        println!("Execution: Skipped (dry-run / auto-execute disabled)");
        println!("zsh: {}\n", zsh_cmd.review_status());

        // Save to SQLite history as validated
        if let Ok(db) = Database::open_default() {
            let _ = db.save_command_with_status(&request, &plan_to_execute, "validated");
        }
        return;
    }

    println!("Executing...\n");

    // Execute the validated plan directly via the executor (program + structured arguments)
    // Never passes command string to sh -c or eval.
    match execute(&plan_to_execute) {
        Ok(exec_result) => {
            let status_str = if exec_result.success {
                "executed"
            } else {
                "failed"
            };

            // Save execution status to SQLite history
            if let Ok(db) = Database::open_default() {
                let _ = db.save_command_with_status(&request, &plan_to_execute, status_str);
            }

            if !exec_result.success {
                let code = exec_result.exit_code.unwrap_or(1);
                process::exit(code);
            }
        }
        Err(err) => {
            eprintln!("\nExecution failed: {}", err);

            // Save failure to SQLite history
            if let Ok(db) = Database::open_default() {
                let _ = db.save_command_with_status(&request, &plan_to_execute, "failed");
            }
            process::exit(1);
        }
    }
}

/// Displays recent command history retrieved from the local SQLite database.
/// History is display-only; historical commands are never executed.
fn display_history() {
    println!("CmdMind History");
    println!("────────────────────────────\n");

    match Database::open_default() {
        Ok(db) => match db.get_recent_history(20) {
            Ok(entries) => {
                if entries.is_empty() {
                    println!("No historical commands found.");
                    return;
                }
                for (i, entry) in entries.iter().enumerate() {
                    println!("{}. {}", i + 1, entry.request);
                    println!("   {}", entry.command);
                    println!("   Source: {} | Status: {}", entry.source, entry.status);
                    println!("   {}\n", entry.created_at);
                }
            }
            Err(e) => eprintln!("Failed to retrieve history records: {}", e),
        },
        Err(e) => eprintln!("Failed to connect to history database: {}", e),
    }
}

/// Displays a helpful usage guide when the user provides no request or requests help.
fn print_usage() {
    println!(
        "CmdMind {} - Natural-Language CLI for Shell Commands\n",
        env!("CARGO_PKG_VERSION")
    );
    println!("Usage:");
    println!("    cmdmind \"<natural language request>\"");
    println!("    cmdmind <natural language request>");
    println!("    cmdmind --dry-run \"<natural language request>\"");
    println!("    cmdmind history");
    println!("    cmdmind --version");
    println!("    cmdmind --help\n");
    println!("Options:");
    println!("    --dry-run    Generate and validate command without executing");
    println!("    --version    Show version information");
    println!("    --help       Show this help message\n");
    println!("Examples:");
    println!("    cmdmind \"find all pdf files\"");
    println!("    cmdmind \"find Python files modified in the last two days\"");
    println!("    cmdmind git status");
    println!("    cmdmind history\n");
    println!("Note: Please provide a natural-language request to continue.");
}
