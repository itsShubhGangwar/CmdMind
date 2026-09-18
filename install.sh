#!/usr/bin/env bash
# =============================================================================
# CmdMind — Standalone macOS/Linux Installer
# =============================================================================
# Installs CmdMind and its zsh Line Editor (ZLE) integration for the current user.
# Does NOT require root or sudo privileges.
# =============================================================================

set -euo pipefail

# Configuration: User-local target paths
BIN_DIR="${HOME}/.local/bin"
SHARE_DIR="${HOME}/.local/share/cmdmind"
BIN_NAME="cmdmind"
ZSH_SCRIPT_NAME="cmdmind.zsh"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=========================================="
echo "  CmdMind Installer"
echo "=========================================="

# Locate source binary
SOURCE_BIN=""
if [[ -f "${SCRIPT_DIR}/${BIN_NAME}" ]]; then
    SOURCE_BIN="${SCRIPT_DIR}/${BIN_NAME}"
elif [[ -f "${SCRIPT_DIR}/target/release/${BIN_NAME}" ]]; then
    SOURCE_BIN="${SCRIPT_DIR}/target/release/${BIN_NAME}"
elif [[ -f "${SCRIPT_DIR}/bin/${BIN_NAME}" ]]; then
    SOURCE_BIN="${SCRIPT_DIR}/bin/${BIN_NAME}"
fi

if [[ -z "${SOURCE_BIN}" ]]; then
    echo "Error: Could not locate ${BIN_NAME} binary in ${SCRIPT_DIR}." >&2
    echo "If building from source, please run 'cargo build --release' first." >&2
    exit 1
fi

# Locate zsh integration script
SOURCE_ZSH=""
if [[ -f "${SCRIPT_DIR}/zsh/${ZSH_SCRIPT_NAME}" ]]; then
    SOURCE_ZSH="${SCRIPT_DIR}/zsh/${ZSH_SCRIPT_NAME}"
elif [[ -f "${SCRIPT_DIR}/${ZSH_SCRIPT_NAME}" ]]; then
    SOURCE_ZSH="${SCRIPT_DIR}/${ZSH_SCRIPT_NAME}"
fi

if [[ -z "${SOURCE_ZSH}" ]]; then
    echo "Error: Could not locate ${ZSH_SCRIPT_NAME} in ${SCRIPT_DIR}." >&2
    exit 1
fi

# 1. Create target directories
echo "Creating user directories..."
mkdir -p "${BIN_DIR}"
mkdir -p "${SHARE_DIR}"

# 2. Install binary
echo "Installing ${BIN_NAME} to ${BIN_DIR}/${BIN_NAME}..."
cp "${SOURCE_BIN}" "${BIN_DIR}/${BIN_NAME}"
chmod +x "${BIN_DIR}/${BIN_NAME}"

# 3. Install zsh integration
echo "Installing zsh integration to ${SHARE_DIR}/${ZSH_SCRIPT_NAME}..."
cp "${SOURCE_ZSH}" "${SHARE_DIR}/${ZSH_SCRIPT_NAME}"

echo ""
echo "Installation complete!"
echo "------------------------------------------"

# 4. PATH Verification
if [[ ":$PATH:" != *":${BIN_DIR}:"* ]]; then
    echo ""
    echo "[!] Notice: ${BIN_DIR} is not currently in your PATH."
    echo "    To make 'cmdmind' available in your terminal, add the following to ~/.zshrc:"
    echo "        export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# 5. zsh Integration Instructions
ZSHRC="${HOME}/.zshrc"
SOURCE_LINE="[[ -f \"\$HOME/.local/share/cmdmind/cmdmind.zsh\" ]] && source \"\$HOME/.local/share/cmdmind/cmdmind.zsh\""

echo ""
echo "To enable natural-language command execution on Enter in zsh:"
echo "Add the following line to your ~/.zshrc:"
echo ""
echo "    ${SOURCE_LINE}"
echo ""

# Optional automatic configuration if requested
if [[ "${1:-}" == "--auto-zshrc" ]]; then
    if [[ -f "${ZSHRC}" ]] && grep -Fq "cmdmind.zsh" "${ZSHRC}"; then
        echo "zsh integration is already present in ${ZSHRC}."
    else
        echo "Adding CmdMind zsh integration to ${ZSHRC}..."
        echo "" >> "${ZSHRC}"
        echo "# CmdMind natural-language zsh integration" >> "${ZSHRC}"
        echo "${SOURCE_LINE}" >> "${ZSHRC}"
        echo "Added to ${ZSHRC}."
    fi
else
    echo "To have the installer configure ~/.zshrc automatically, re-run with: ./install.sh --auto-zshrc"
fi

echo ""
echo "To verify the installation:"
echo "    ${BIN_DIR}/cmdmind --version"
echo "    ${BIN_DIR}/cmdmind --help"
echo "=========================================="
