# CmdMind

**CmdMind** is a local-first natural-language command-line interface (CLI) for macOS zsh designed to convert natural-language developer requests into safe shell commands and execute them directly upon pressing Enter.

---

## Architecture: Direct Natural-Language Command Execution

CmdMind transitions from a command-suggestion / buffer-insertion model to **direct natural-language command execution from the terminal**.

### System Pipeline

```text
User Natural-Language Input (e.g. "find all pdf files")
                      ↓
              [ User presses Enter ]
                      ↓
             macOS zsh Interceptor
                      ↓
        CmdMind Binary (`cmdmind`)
                      ↓
        Tier-1 Engine (Deterministic)
                      ↓
                 Recognized?
                 /        \
              Yes          No
               ↓            ↓
          CommandPlan     Ollama (Local LLM HTTP API)
                            ↓
                      CommandPlan (UNTRUSTED)
                            ↓
                  🔐 Security Validator
                            ↓
                    ┌───────────────┐
                    │               │
                   SAFE           UNSAFE
                    │               │
                    ↓               ↓
              ValidatedPlan     Reject (SecurityError) ───[ STOP / NO EXECUTION ]
                    ↓
              Path Resolver
                    ↓
         ┌──────────────────────┐
         │                      │
     No correction          Correction
      needed                suggested
         │                      │
         │                      ↓
         │             New CommandPlan (UNTRUSTED)
         │                      ↓
         │             Security Validator AGAIN
         │                      ↓
         │             ValidatedPlan (or Reject)
         │                      │
         └──────────┬───────────┘
                    ↓
            Direct Executor
                    ↓
        Structured Tokenization (`program` + `args[]`)
                    ↓
           Direct OS Process (`execve` / `CreateProcessW`)
                    ↓
             Terminal Output (Live stdout / stderr)
                    ↓
       SQLite History Persistence (`status`: executed / failed)
```

---

## Critical Security Principles

### 1. The Type-State Security Boundary

CmdMind enforces execution safety using Rust's compile-time type system:

```text
CommandPlan (untrusted data)
     ↓
validate()
     ↓
ValidatedPlan (immutable proof of validation)
     ↓
executor::execute(&ValidatedPlan)
```

- **Compile-Time Guarantee**: `executor::execute()` accepts **ONLY** `&ValidatedPlan`.
- Passing `CommandPlan`, `String`, or `&str` directly to the execution API is a **compile-time error**.
- There is no `is_valid: bool` flag that can be manipulated or bypassed.
- No generated command from Tier-1 or Ollama can reach the executor without passing `validate()`.

### 2. No Shell-String Evaluation (`eval` / `sh -c` Prohibited)

CmdMind **never** executes commands through a shell interpreter:
- **PROHIBITED**: `sh -c`, `bash -c`, `zsh -c`, `eval`.
- **IMPLEMENTED**: The executor uses a quote-aware tokenizer to parse the validated command into structured tokens:
  ```text
  find . -type f -name '*.pdf'
  ↓
  program: "find"
  args: [".", "-type", "f", "-name", "*.pdf"]
  ```
- Execution uses `std::process::Command::new(program).args(args)`, invoking the operating system's process launcher directly (`execve` on Unix, `CreateProcessW` on Windows).
- Shell-control constructs (`;`, `&&`, `||`, `|`, `$`, `$(...)`, `` `...` ``, `>`, `>>`, `<`, `2>`) are rejected at the security boundary before execution can ever be considered.
- If a command cannot be safely tokenized, execution fails closed with `StructuredParseError`.

> [!CAUTION]
> **Important Security Clarification**:
> Direct process execution via `std::process::Command` does not by itself make arbitrary command strings safe. Safety arises from the strict composition of:
> 1. Constrained deterministic and structured LLM command generation,
> 2. Conservative security validation against prohibited executables and paths,
> 3. Zero shell-string interpretation (no shell metacharacters or subshell expansions),
> 4. Structured argument tokenization, and
> 5. Compile-time `ValidatedPlan` type-state boundary enforcement.

### 3. Crucial Conceptual Distinctions

```text
Valid JSON  ≠  Safe command
Safe command  ≠  Automatically trusted
ValidatedPlan  ≠  Guaranteed desired behavior
```

1. **Valid JSON**: Just because Ollama returns valid JSON with a `"command"` field does not mean the command is safe. It remains strictly an untrusted `CommandPlan`.
2. **Security validation**: Confirms that a command adheres to strict structural policies (no dangerous binaries like `rm`, `sudo`, `dd`, `mkfs`, `shutdown`; no critical system directory targets like `/etc`, `/System`, `/dev`; no shell control operators).
3. **Path correction**: Detects typos in referenced paths using Levenshtein distance (e.g. `srcc` → `src`). The suggested replacement command is **re-validated from scratch** before being offered or executed.
4. **Execution**: Runs the validated command directly via OS process creation, streaming live stdout and stderr to the terminal.
5. **Stored history**: Records an audit trail of executed or failed commands in local SQLite. Commands from history are never re-executed automatically.

---

## macOS + zsh "Press Enter Once" Integration

CmdMind integrates directly with the macOS Zsh Line Editor (ZLE) via [`zsh/cmdmind.zsh`](file:///C:/Users/shubh/.gemini/antigravity/scratch/cmdmind/zsh/cmdmind.zsh).

### User Experience

```bash
% find all pdf files
```
*Developer presses Enter once.*

**Output:**
```text
CmdMind
────────────────────

Request: find all pdf files

Intent: Find PDF files

Generated command:
find . -type f -name '*.pdf'

Source: Tier-1
Security: Validated
Path: No correction needed

Executing...

./docs/manual.pdf
./assets/spec.pdf
```

### Conservative Natural-Language Detection
To ensure standard shell commands continue to function with zero interference:
1. **Normal Commands Pass Through**: Built-in commands, functions, and standard executables (`git status`, `ls -la`, `cd src`, `cargo test`, `python script.py`) run normally in zsh without invoking CmdMind.
2. **Natural-Language Detection**: Multi-word input whose first token is not a recognized executable, or whose arguments consist of natural-language keywords (e.g. `find all pdf files`) rather than flags and existing files, is routed to CmdMind.
3. **Escape Mechanism**: Prefixing any line with a backslash (e.g. `\find`) or a leading space explicitly bypasses CmdMind and forces standard zsh execution.
4. **Recursion Guard**: Subprocesses spawned by CmdMind execute via `execve` directly and never pass back through the ZLE buffer.

---

## Modes & Configuration

### Direct Execution Mode (Default)
By default, validated commands execute directly when the user presses Enter.
- Exit codes from the underlying process are captured and preserved.
- Execution status (`executed` or `failed`) is recorded in SQLite.

### Dry-Run Mode / Manual Review
To inspect a generated command without executing it:
```bash
# Via CLI flag:
cmdmind --dry-run "find all pdf files"

# Or via environment variable:
export CMDMIND_AUTO_EXECUTE=false
```
**Output:**
```text
CmdMind
────────────────────

Request: find all pdf files

Intent: Find PDF files

Generated command:
find . -type f -name '*.pdf'

Source: Tier-1
Security: Validated
Path: No correction needed

Execution: Skipped (dry-run / auto-execute disabled)
zsh: Ready for human review
```

> [!NOTE]
> Setting `CMDMIND_AUTO_EXECUTE=false` disables execution, but **never disables security validation**.
> There is **NO** `--unsafe` or `--skip-validation` flag. Bypassing validation is architecturally impossible.

---

---

## Deployment D2: macOS Production Guide & Real zsh Integration

### 1. Supported Targets & Platform Requirements
- **Primary Production Target**: `aarch64-apple-darwin` (Apple Silicon: M1/M2/M3/M4)
- **Secondary Target**: `x86_64-apple-darwin` (Intel Macs)
- **Operating System**: macOS 12 (Monterey) or later
- **Interactive Shell**: zsh 5.8+ (default login shell on macOS)
- **Local AI Provider**: Ollama (optional; Tier-1 runs 100% offline without Ollama)
- **C Toolchain**: Apple Clang (`xcode-select --install`) for bundled SQLite compilation

### 2. Building on macOS
On a native macOS terminal:
```bash
# Clone or navigate to the repository
cd cmdmind

# Add the target if not already present
rustup target add aarch64-apple-darwin

# Build the optimized release binary
cargo build --release

# The compiled binary is located at:
# target/release/cmdmind
```
Verify the resulting binary architecture:
```bash
file target/release/cmdmind
# Expected output on Apple Silicon: Mach-O 64-bit executable arm64
```

### 3. Installation (User-Local, Non-Root)
CmdMind requires no root or `sudo` privileges. Install the binary into a directory on your `$PATH` (e.g. `~/.local/bin` or `~/.cargo/bin`):
```bash
mkdir -p ~/.local/bin
cp target/release/cmdmind ~/.local/bin/
chmod +x ~/.local/bin/cmdmind

# Verify availability:
which cmdmind
cmdmind --help
```

### 4. Setting Up the macOS zsh Integration
To enable the **Enter once** natural-language command experience:

#### Interactive Testing:
```bash
source /path/to/cmdmind/zsh/cmdmind.zsh
```

#### Persistent Configuration:
Add this line to your `~/.zshrc`:
```bash
# Enable CmdMind natural-language command execution
[[ -f "$HOME/.local/bin/cmdmind" ]] && source "/path/to/cmdmind/zsh/cmdmind.zsh"
```

To cleanly unload the integration at any time without restarting zsh:
```bash
cmdmind-unload
```

### 5. Ollama Setup & Configuration on macOS
CmdMind uses local Ollama when an English request falls outside Tier-1 deterministic patterns:
```bash
# Install Ollama on macOS (if not already installed)
brew install ollama

# Start the Ollama server
ollama serve

# Pull the default lightweight model
ollama pull llama3.2:3b
```

#### Supported Environment Variables:
- `OLLAMA_BASE_URL`: Base URL for the Ollama daemon (defaults to `http://localhost:11434`).
- `OLLAMA_MODEL`: Model name for natural language parsing (defaults to `llama3.2:3b`).
- `CMDMIND_AUTO_EXECUTE`: Set to `false` or `0` to disable automatic execution (reverts to dry-run mode).
- `CMDMIND_DB_PATH`: Custom path for SQLite history database (defaults to `~/Library/Application Support/cmdmind/cmdmind.db`).

### 6. Verification Test Matrix (Tests A through J)

| Test | Request / Input | Expected Behavior | Status |
| :--- | :--- | :--- | :--- |
| **Test A: Tier-1 Natural Language** | `find all pdf files` | Generates `find . -type f -name '*.pdf'`, validates, executes directly | Verified (Windows Dev) / Ready for macOS |
| **Test B: Ollama Fallback** | `find python files modified recently` | Fallback to Ollama, produces `find . -type f -name '*.py' -mtime -2`, validates, executes | Verified with Ollama / Ready for macOS |
| **Test C: Normal Shell Command** | `git status` | Pass straight through to native zsh without interception | Verified in `zsh/cmdmind.zsh` / Ready for macOS |
| **Test D: Dangerous Command** | `delete all files in root directory` | Stopped at Security Validator with `Reason: 'rm' is prohibited`; executor never called | **VERIFIED** (0 executor calls) |
| **Test E: Path Correction** | `find ./srcc -type f` | Detects typo, suggests `./src`, re-validates, executes corrected command | **VERIFIED** |
| **Test F: Dry-Run Mode** | `cmdmind --dry-run "find all pdf files"` | Generates & validates command, reports `Execution: Skipped (dry-run)` | **VERIFIED** |
| **Test G: Non-Zero Exit Code** | Command that fails in child process | Preserves exact child exit code, records `Status: failed` in SQLite | **VERIFIED** |
| **Test H: Missing Executable** | Command referencing non-existent binary | Reports structured `Command not found` error, records failure | **VERIFIED** |
| **Test I: Normal zsh Behavior** | `ls -la`, `cd src`, `cargo test` | Passed through to zsh untouched; escape prefix (`\command`) bypasses CmdMind | **VERIFIED** in integration logic |
| **Test J: Enter-Once UX** | Natural language + Enter once | Intercepted by ZLE, executed live, prompt cleanly redrawn | Ready for macOS native terminal |

### 7. Troubleshooting on macOS
- **`cmdmind: command not found`**: Ensure `~/.local/bin` or `~/.cargo/bin` is in your `PATH` in `~/.zshrc`:
  ```bash
  export PATH="$HOME/.local/bin:$PATH"
  ```
- **Ollama Connection Refused**: Start the Ollama daemon via `ollama serve` or open the Ollama macOS app.
- **Normal Command Accidentally Intercepted**: Prefix with a backslash (e.g. `\mycommand`) or a leading space to bypass CmdMind.

---

## Development vs Production Environment

- **Current Development Environment (Windows x86_64)**:
  - Compiles the full CmdMind binary and release targets.
  - Runs all unit tests, structured tokenization tests, Levenshtein distance tests, and SQLite persistence tests.
  - Tests execution safely using `MockCommandRunner` to verify program and argument lists without invoking arbitrary external binaries.
- **Production Target (macOS + zsh)**:
  - The ZLE hook ([`zsh/cmdmind.zsh`](file:///C:/Users/shubh/.gemini/antigravity/scratch/cmdmind/zsh/cmdmind.zsh)) is designed specifically for macOS zsh.
  - Real interactive ZLE Enter interception, Ollama communication on macOS, and full end-to-end macOS shell execution must be verified on a macOS host.


---

## How to Build, Test, and Verify

```bash
# Check standard formatting
cargo fmt --check

# Compile check across all targets
cargo check --all-targets

# Run the complete test suite (97 tests: 79 unit + 18 integration tests)
cargo test

# Build optimized production binary
cargo build --release

# Run benchmarks
cargo bench
```

---

## Live CLI Examples

### 1. Direct Execution of Safe Command
```bash
target/release/cmdmind "git status"
```
**Output:**
```text
CmdMind
────────────────────

Request: git status

Intent: Git status

Generated command:
git status

Source: Tier-1
Security: Validated

Executing...

On branch main
Your branch is up to date with 'origin/main'.
nothing to commit, working tree clean
```

### 2. Auto-Healing Path Correction
```bash
target/release/cmdmind "find all files inside directory srcc"
```
**Output:**
```text
CmdMind
────────────────────

Request: find all files inside directory srcc

Intent: Find all files in the specified directory and its subdirectories

Generated command:
find ./srcc -type f

Source: Ollama
Security: Validated

Path correction suggested:

Original:
find ./srcc -type f

Suggested:
find ./src -type f

Reason:
"./srcc" does not exist; "./src" is the closest existing path.

The suggested command was re-validated successfully.

Executing...

./src/main.rs
./src/lib.rs
```

### 3. Prohibited Command Rejected (Never Reaches Executor)
```bash
target/release/cmdmind "delete all files in root directory"
```
**Output:**
```text
CmdMind
────────────────────

Request: delete all files in root directory

Security validation rejected the generated command.

Reason: Dangerous command detected: 'rm' is prohibited by security policy.

Command was NOT executed.
```

### 4. Audit SQLite History (`cmdmind history`)
```bash
target/release/cmdmind history
```
**Output:**
```text
CmdMind History
────────────────────────────

1. git status
   git status
   Source: Tier-1 | Status: executed
   2026-09-18T06:21:49.892840+00:00

2. find all pdf files
   find . -type f -name '*.pdf'
   Source: Tier-1 | Status: validated
   2026-09-18T06:21:35.537914200+00:00
```
*(Notice: Dangerous rejected commands are never stored in history).*