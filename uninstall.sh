#!/usr/bin/env bash
# =============================================================================
# CmdMind — Safe Uninstaller
# =============================================================================
# Removes CmdMind binaries and zsh integration files installed for the current user.
# User command history database is preserved by default unless --purge-data is passed.
# =============================================================================

set -euo pipefail

BIN_DIR="${HOME}/.local/bin"
SHARE_DIR="${HOME}/.local/share/cmdmind"
BIN_FILE="${BIN_DIR}/cmdmind"
ZSH_FILE="${SHARE_DIR}/cmdmind.zsh"
DB_DIR_MACOS="${HOME}/Library/Application Support/cmdmind"
DB_DIR_LINUX="${HOME}/.local/share/cmdmind"

echo "=========================================="
echo "  CmdMind Uninstaller"
echo "=========================================="

REMOVED=0

# 1. Remove binary
if [[ -f "${BIN_FILE}" ]]; then
    echo "Removing binary: ${BIN_FILE}"
    rm -f "${BIN_FILE}"
    REMOVED=$((REMOVED + 1))
else
    echo "Binary not found at ${BIN_FILE} (already removed)."
fi

# 2. Remove zsh integration
if [[ -f "${ZSH_FILE}" ]]; then
    echo "Removing zsh integration: ${ZSH_FILE}"
    rm -f "${ZSH_FILE}"
    REMOVED=$((REMOVED + 1))
fi

if [[ -d "${SHARE_DIR}" ]]; then
    # Remove directory if empty or only contains cmdmind files
    rmdir "${SHARE_DIR}" 2>/dev/null || true
fi

# 3. Handle user SQLite data
PURGE_DATA=0
if [[ "${1:-}" == "--purge-data" ]]; then
    PURGE_DATA=1
fi

if [[ ${PURGE_DATA} -eq 1 ]]; then
    echo "Purging command history database as requested..."
    if [[ -d "${DB_DIR_MACOS}" ]]; then
        rm -rf "${DB_DIR_MACOS}"
        echo "Removed: ${DB_DIR_MACOS}"
    fi
    if [[ -d "${DB_DIR_LINUX}" && "${DB_DIR_LINUX}" != "${SHARE_DIR}" ]]; then
        rm -rf "${DB_DIR_LINUX}"
        echo "Removed: ${DB_DIR_LINUX}"
    fi
else
    echo ""
    echo "[i] User command history database was preserved at:"
    if [[ -d "${DB_DIR_MACOS}" ]]; then
        echo "    ${DB_DIR_MACOS}/cmdmind.db"
    elif [[ -d "${DB_DIR_LINUX}" ]]; then
        echo "    ${DB_DIR_LINUX}/cmdmind.db"
    else
        echo "    (No database directory found)"
    fi
    echo "    To delete history as well, run: ./uninstall.sh --purge-data"
fi

# 4. Clean up instructions for shell configuration
echo ""
echo "Notice: If you added CmdMind to your ~/.zshrc, please remove the following lines:"
echo '    export PATH="$HOME/.local/bin:$PATH"'
echo '    source "$HOME/.local/share/cmdmind/cmdmind.zsh"'
echo ""
echo "CmdMind uninstall complete."
echo "=========================================="
