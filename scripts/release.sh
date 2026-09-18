#!/usr/bin/env bash
# =============================================================================
# CmdMind — Release Packaging Script (POSIX / macOS / Linux)
# =============================================================================
# Builds release binaries, generates archives, and produces SHA256SUMS.
# Fails clearly if any step fails.
# =============================================================================

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

# Extract version from Cargo.toml
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f 2)
if [[ -z "${VERSION}" ]]; then
    echo "Error: Could not extract version from Cargo.toml." >&2
    exit 1
fi

echo "=========================================="
echo " Packaging CmdMind v${VERSION}"
echo "=========================================="

RELEASE_DIR="${ROOT_DIR}/release"
mkdir -p "${RELEASE_DIR}"

# Detect Host OS
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

echo "Host OS: ${OS} (${ARCH})"

# Function to package a tar.gz release artifact
package_tar_gz() {
    local target_arch="$1"
    local bin_path="$2"
    local archive_name="cmdmind-${VERSION}-${target_arch}.tar.gz"
    local stage_dir="${RELEASE_DIR}/stage_${target_arch}"

    if [[ ! -f "${bin_path}" ]]; then
        echo "Error: Binary not found at ${bin_path}" >&2
        return 1
    fi

    echo "Packaging ${archive_name}..."
    rm -rf "${stage_dir}"
    mkdir -p "${stage_dir}/zsh"

    cp "${bin_path}" "${stage_dir}/cmdmind"
    chmod +x "${stage_dir}/cmdmind"
    cp "zsh/cmdmind.zsh" "${stage_dir}/zsh/cmdmind.zsh"
    cp "install.sh" "${stage_dir}/install.sh"
    chmod +x "${stage_dir}/install.sh"
    cp "uninstall.sh" "${stage_dir}/uninstall.sh"
    chmod +x "${stage_dir}/uninstall.sh"
    cp "README.md" "${stage_dir}/README.md"
    cp "LICENSE" "${stage_dir}/LICENSE"

    tar -czf "${RELEASE_DIR}/${archive_name}" -C "${stage_dir}" .
    rm -rf "${stage_dir}"
    echo "Created: ${RELEASE_DIR}/${archive_name}"
}

if [[ "${OS}" == "darwin" ]]; then
    echo "Detected macOS environment. Building native macOS release artifacts..."

    # Determine targets to build
    TARGETS=()
    if [[ "${ARCH}" == "arm64" ]]; then
        TARGETS+=("aarch64-apple-darwin")
    elif [[ "${ARCH}" == "x86_64" ]]; then
        TARGETS+=("x86_64-apple-darwin")
    fi

    for target in "${TARGETS[@]}"; do
        echo "Building release for ${target}..."
        rustup target add "${target}" 2>/dev/null || true
        cargo build --release --target "${target}"
        
        local_arch="macos-aarch64"
        if [[ "${target}" == *"x86_64"* ]]; then
            local_arch="macos-x86_64"
        fi

        package_tar_gz "${local_arch}" "target/${target}/release/cmdmind"
    done
else
    echo "Notice: Native macOS builds must be compiled on macOS hosts."
    echo "Building current host release..."
    cargo build --release
    package_tar_gz "${OS}-${ARCH}" "target/release/cmdmind"
fi

# Generate SHA-256 Checksums
echo "Generating SHA256SUMS..."
cd "${RELEASE_DIR}"
rm -f SHA256SUMS

for file in *.tar.gz *.zip; do
    if [[ -f "${file}" ]]; then
        if command -v shasum >/dev/null 2>&1; then
            shasum -a 256 "${file}" >> SHA256SUMS
        elif command -v sha256sum >/dev/null 2>&1; then
            sha256sum "${file}" >> SHA256SUMS
        fi
    fi
done

echo ""
echo "=========================================="
echo " Release Packaging Complete!"
echo " Output files in ${RELEASE_DIR}:"
ls -lh "${RELEASE_DIR}"
echo "=========================================="
