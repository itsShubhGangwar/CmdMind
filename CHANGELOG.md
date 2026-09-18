# Changelog

All notable changes to the CmdMind project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-09-18

### Added
- **Direct Natural-Language Command Execution**: Single-keypress Enter execution in macOS zsh.
- **Type-State Security Boundary**: `ValidatedPlan` compile-time type requirement for the `executor` module.
- **Structured Process Execution**: Native `std::process::Command` execution with discrete arguments and zero shell evaluation (`sh -c` and `eval` prohibited).
- **Tier-1 Deterministic Intent Engine**: High-speed, microsecond matching for frequent developer requests (`find all pdf files`, `git status`, `list files`).
- **Ollama Local LLM Fallback**: Local AI fallback for arbitrary requests without internet access.
- **Auto-Healing Path Resolver**: Conservative Levenshtein distance typo correction with mandatory re-validation.
- **SQLite History Persistence**: Local command audit log with status tracking (`executed`, `failed`, `validated`).
- **macOS zsh / ZLE Integration**: `zsh/cmdmind.zsh` with `accept-line` interception, normal shell command passthrough, and recursion prevention.
- **Standalone Installer & Uninstaller**: Non-root `install.sh` and `uninstall.sh`.
- **Release Automation**: `scripts/release.sh`, `scripts/release.ps1`, and GitHub Actions workflow.
- **Adversarial & Benchmark Test Suite**: 97 automated tests covering adversarial inputs, type-state enforcement, and micro-benchmarks.
