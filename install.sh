#!/bin/sh
# fswatchd installer
# Usage: curl -fsSL https://raw.githubusercontent.com/altinok/fswatchd/main/install.sh | sh

set -e

REPO="altinok/fswatchd"
BINARY_NAME="fswatchd"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "darwin" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)       echo "unknown" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64) echo "x64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)            echo "unknown" ;;
    esac
}

# Get the download URL for the latest release
get_download_url() {
    local os=$1
    local arch=$2
    local target=""
    local ext=""

    case "$os-$arch" in
        darwin-arm64) target="aarch64-apple-darwin"; ext="tar.gz" ;;
        darwin-x64)   target="x86_64-apple-darwin"; ext="tar.gz" ;;
        linux-x64)    target="x86_64-unknown-linux-gnu"; ext="tar.gz" ;;
        windows-x64)  target="x86_64-pc-windows-msvc"; ext="zip" ;;
        windows-arm64) target="aarch64-pc-windows-msvc"; ext="zip" ;;
        *)
            echo "Unsupported platform: $os-$arch" >&2
            exit 1
            ;;
    esac

    # Get latest release tag
    local latest_tag
    latest_tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$latest_tag" ]; then
        echo "Failed to get latest release" >&2
        exit 1
    fi

    echo "https://github.com/$REPO/releases/download/$latest_tag/fswatchd-$target.$ext"
}

main() {
    local os=$(detect_os)
    local arch=$(detect_arch)

    echo "Detected: $os-$arch"

    if [ "$os" = "unknown" ] || [ "$arch" = "unknown" ]; then
        echo "Unsupported platform" >&2
        exit 1
    fi

    local url=$(get_download_url "$os" "$arch")
    local tmp_dir=$(mktemp -d)
    local archive_name="fswatchd-archive"

    echo "Downloading from: $url"

    # Download
    if command -v curl > /dev/null; then
        curl -fsSL "$url" -o "$tmp_dir/$archive_name"
    elif command -v wget > /dev/null; then
        wget -q "$url" -O "$tmp_dir/$archive_name"
    else
        echo "Error: curl or wget required" >&2
        exit 1
    fi

    # Extract
    cd "$tmp_dir"
    case "$url" in
        *.tar.gz)
            tar -xzf "$archive_name"
            ;;
        *.zip)
            unzip -q "$archive_name"
            ;;
    esac

    # Install
    mkdir -p "$INSTALL_DIR"

    if [ "$os" = "windows" ]; then
        mv "$BINARY_NAME.exe" "$INSTALL_DIR/"
    else
        mv "$BINARY_NAME" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/$BINARY_NAME"
    fi

    # Cleanup
    rm -rf "$tmp_dir"

    echo ""
    echo "fswatchd installed to: $INSTALL_DIR/$BINARY_NAME"
    echo ""

    # Check if in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo "Add to your PATH:"
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
    fi

    echo "Run 'fswatchd --help' to get started"
}

main
