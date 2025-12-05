#!/bin/sh
# Nexus installer script
# Usage: curl -fsSL https://raw.githubusercontent.com/sheldon-lewis/nexus/main/scripts/install.sh | sh
#
# Options:
#   --global         Install to /usr/local/bin (requires sudo)
#   --version <ver>  Install specific version (e.g., 1.0.0)
#   --help           Show this help message

set -e

REPO="zeddy89/nexus"
BINARY_NAME="nexus"
DEFAULT_INSTALL_DIR="$HOME/.local/bin"
GLOBAL_INSTALL_DIR="/usr/local/bin"

# Colors (only if terminal supports them)
if [ -t 1 ]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    BOLD='\033[1m'
    NC='\033[0m'
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    BOLD=''
    NC=''
fi

info() {
    printf "${BLUE}==>${NC} ${BOLD}%s${NC}\n" "$1"
}

success() {
    printf "${GREEN}==>${NC} ${BOLD}%s${NC}\n" "$1"
}

warn() {
    printf "${YELLOW}Warning:${NC} %s\n" "$1"
}

error() {
    printf "${RED}Error:${NC} %s\n" "$1" >&2
    exit 1
}

usage() {
    cat <<EOF
Nexus Installer

Usage: $0 [OPTIONS]

Options:
    --global         Install to /usr/local/bin (requires sudo)
    --version <ver>  Install specific version (e.g., 1.0.0)
    --help           Show this help message

Examples:
    $0                      # Install latest to ~/.local/bin
    $0 --global             # Install latest to /usr/local/bin
    $0 --version 1.0.0      # Install specific version
EOF
    exit 0
}

detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        *)       error "Unsupported operating system: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             error "Unsupported architecture: $(uname -m)" ;;
    esac
}

get_latest_version() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
            grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/'
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | \
            grep '"tag_name":' | sed -E 's/.*"v([^"]+)".*/\1/'
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

download() {
    url="$1"
    output="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$url" -O "$output"
    else
        error "Neither curl nor wget found. Please install one of them."
    fi
}

verify_checksum() {
    file="$1"
    expected="$2"

    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$file" | cut -d' ' -f1)
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    else
        warn "Neither sha256sum nor shasum found. Skipping checksum verification."
        return 0
    fi

    if [ "$actual" != "$expected" ]; then
        error "Checksum verification failed!\nExpected: $expected\nActual:   $actual"
    fi
}

main() {
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    VERSION=""
    USE_SUDO=""

    # Parse arguments
    while [ $# -gt 0 ]; do
        case "$1" in
            --global)
                INSTALL_DIR="$GLOBAL_INSTALL_DIR"
                USE_SUDO="sudo"
                shift
                ;;
            --version)
                VERSION="$2"
                shift 2
                ;;
            --help|-h)
                usage
                ;;
            *)
                error "Unknown option: $1\nRun '$0 --help' for usage."
                ;;
        esac
    done

    OS=$(detect_os)
    ARCH=$(detect_arch)

    info "Detected OS: $OS, Architecture: $ARCH"

    # Get version
    if [ -z "$VERSION" ]; then
        info "Fetching latest version..."
        VERSION=$(get_latest_version)
        if [ -z "$VERSION" ]; then
            error "Failed to fetch latest version. Please specify a version with --version"
        fi
    fi

    info "Installing Nexus v${VERSION}"

    # Construct download URL
    TARBALL="nexus-${VERSION}-${OS}-${ARCH}.tar.gz"
    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/v${VERSION}/${TARBALL}"
    CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

    # Create temp directory
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    # Download files
    info "Downloading ${TARBALL}..."
    download "$DOWNLOAD_URL" "$TMP_DIR/$TARBALL" || error "Failed to download $TARBALL"

    info "Downloading checksum..."
    download "$CHECKSUM_URL" "$TMP_DIR/${TARBALL}.sha256" || error "Failed to download checksum"

    # Verify checksum
    info "Verifying checksum..."
    EXPECTED_CHECKSUM=$(cut -d' ' -f1 "$TMP_DIR/${TARBALL}.sha256")
    verify_checksum "$TMP_DIR/$TARBALL" "$EXPECTED_CHECKSUM"
    success "Checksum verified"

    # Extract
    info "Extracting..."
    tar -xzf "$TMP_DIR/$TARBALL" -C "$TMP_DIR"

    # Install
    info "Installing to ${INSTALL_DIR}..."
    mkdir -p "$INSTALL_DIR"
    $USE_SUDO install -m 755 "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"

    success "Nexus v${VERSION} installed successfully!"

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo ""
            warn "$INSTALL_DIR is not in your PATH."
            echo "Add the following to your shell configuration file:"
            echo ""
            echo "    export PATH=\"\$PATH:$INSTALL_DIR\""
            echo ""
            ;;
    esac

    echo ""
    echo "Run 'nexus --help' to get started."
}

main "$@"
