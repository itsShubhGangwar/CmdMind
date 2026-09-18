# CmdMind 0.1.0 — Release Notes

**CmdMind** is a local-first natural-language command-line interface (CLI) for macOS zsh that converts natural-language developer requests into safe shell commands and executes them directly when the user presses Enter.

---

## Release Highlights

- **Direct Natural-Language Execution**: Type an English request (e.g. `find all pdf files`) and press Enter once in zsh. CmdMind interprets, validates, and executes the resulting command directly without manual copy-pasting or secondary confirmation.
- **Type-State Security Boundary**: Execution is strictly restricted to `ValidatedPlan` instances. Raw `CommandPlan` or arbitrary strings cannot be passed to the executor at compile time.
- **Zero Shell-String Evaluation**: CmdMind **never** uses `eval`, `sh -c`, `bash -c`, or `zsh -c`. Commands are parsed into structured arguments (`program` + `args[]`) and executed directly via OS process creation (`execve` / `CreateProcessW`).
- **Two-Tier Engine**: Instant deterministic matching for common commands via Tier-1, backed by a local Ollama LLM fallback for free-form requests.
- **Auto-Healing Path Resolver**: Detects path typos using Levenshtein distance and suggests corrections that are independently re-validated before execution.
- **Local Persistence & Audit Trail**: Persists executed, failed, and dry-run commands to a local SQLite database (`~/Library/Application Support/cmdmind/cmdmind.db`).
- **Seamless zsh/ZLE Compatibility**: Built-in shell commands (`git status`, `ls -la`, `cd src`, `cargo test`, `python3 script.py`) remain completely untouched and run through normal zsh.

---

## Supported Platforms

| Platform | Architecture | Artifact | Status |
| :--- | :--- | :--- | :--- |
| **macOS** | Apple Silicon (`aarch64-apple-darwin`) | `cmdmind-0.1.0-macos-aarch64.tar.gz` | Prepared (Requires native macOS build) |
| **macOS** | Intel (`x86_64-apple-darwin`) | `cmdmind-0.1.0-macos-x86_64.tar.gz` | Prepared (Requires native macOS build) |
| **Windows** | x86_64 (`x86_64-pc-windows-gnu`) | `cmdmind-0.1.0-windows-x86_64.zip` | **BUILT & VERIFIED** |

---

## Installation

### macOS (Standalone Installer)
```bash
# 1. Download and extract release archive
tar -xzf cmdmind-0.1.0-macos-aarch64.tar.gz
cd cmdmind-0.1.0-macos-aarch64

# 2. Run the non-root user installer
./install.sh

# 3. Add to ~/.zshrc (or use ./install.sh --auto-zshrc)
export PATH="$HOME/.local/bin:$PATH"
source "$HOME/.local/share/cmdmind/cmdmind.zsh"
```

### Windows
Extract `cmdmind-0.1.0-windows-x86_64.zip` and place `cmdmind.exe` in your system `PATH`.

---

## Requirements

- **macOS**: macOS 12 (Monterey) or later with zsh 5.8+.
- **Ollama** (Optional): For requests outside Tier-1 deterministic patterns (`brew install ollama && ollama serve`).
- **Permissions**: Standard user permissions (no `sudo` or root required).

---

## Security Model & Known Limitations

1. **Strict Structural Validation**: Prohibited executables (`rm`, `sudo`, `dd`, `mkfs`, `shutdown`, `reboot`, `diskutil`) and critical paths (`/`, `/etc`, `/System`, `/Library`, `/bin`, etc.) are rejected before execution.
2. **No Shell Metacharacters**: Chaining (`;`, `&&`, `||`), pipes (`|`), redirections (`>`, `<`), and variable substitutions (`$`, `$()`) are blocked at the security boundary.
3. **Local-First**: Zero telemetry, zero cloud dependencies, zero external network calls.
