#!/usr/bin/env sh
# Memex Universal Unix Installer (Linux & macOS)
# Usage: curl -fsSL https://raw.githubusercontent.com/garnizeh/memex/main/install.sh | sh

set -eu

REPO="garnizeh/memex"
INSTALL_DIR="${MEMEX_INSTALL_DIR:-$HOME/.local/bin}"

main() {
    echo "⚡ Installing Memex..."

    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)
            case "$ARCH" in
                x86_64|amd64)
                    TARGET="linux-x86_64"
                    ;;
                *)
                    echo "Error: Unsupported Linux architecture: $ARCH" >&2
                    exit 1
                    ;;
            esac
            ;;
        Darwin)
            case "$ARCH" in
                arm64|aarch64)
                    TARGET="macos-arm64"
                    ;;
                *)
                    echo "Error: Unsupported macOS architecture ($ARCH). Memex supports Apple Silicon (arm64)." >&2
                    exit 1
                    ;;
            esac
            ;;
        *)
            echo "Error: Unsupported operating system: $OS. On Windows, use install.ps1" >&2
            exit 1
            ;;
    esac

    echo "🔍 Resolving latest release for $TARGET..."

    LATEST_TAG="$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"
    if [ -z "$LATEST_TAG" ]; then
        echo "Error: Could not resolve latest release tag from GitHub API." >&2
        exit 1
    fi

    ARTIFACT_NAME="memex-${TARGET}.tar.gz"
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$ARTIFACT_NAME"

    echo "⬇️  Downloading Memex ($LATEST_TAG) from $DOWNLOAD_URL..."

    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    curl -sSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ARTIFACT_NAME"

    mkdir -p "$INSTALL_DIR"
    tar -xzf "$TMP_DIR/$ARTIFACT_NAME" -C "$TMP_DIR"
    cp "$TMP_DIR/memex" "$INSTALL_DIR/memex"
    chmod +x "$INSTALL_DIR/memex"

    echo "✓ Installed memex binary to $INSTALL_DIR/memex"

    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo ""
            echo "⚠️  Note: $INSTALL_DIR is not currently in your PATH."
            echo "Add it to your shell configuration (e.g. ~/.bashrc or ~/.zshrc):"
            echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
            echo ""
            ;;
    esac

    # Run auto-registration with local agents if in interactive terminal
    if [ -t 0 ]; then
        echo "🤖 Auto-registering Memex with installed AI coding agents..."
        "$INSTALL_DIR/memex" install || true
    fi

    echo "✨ Memex installation complete! Run 'memex --help' to get started."
}

main "$@"
