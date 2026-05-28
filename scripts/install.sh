#!/bin/sh
# dscode — one-line install script
# Usage: curl -fsSL https://dscode.org/install.sh | sh

set -eu

REPO="Hmbown/dscode"
BIN_NAME="dscode"
INSTALL_DIR="${DSCODE_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
    x86_64|amd64)  ARCH="amd64"  ;;
    aarch64|arm64) ARCH="arm64"  ;;
    *)             echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

# Print banner
cat <<'EOF'
  ____  _    ____ ___  _  __
 / ___|| |__/ ___/ _ \| |/ /
 \___ \| '_ \___ | | | | ' / 
  ___) | | | |__| | |_| | . \ 
 |____/|_| |_|___|\___/|_|\_\
                              terminal AI agent
EOF

echo ""
echo "  Installing dscode ($OS/$ARCH)..."
echo ""

# Determine install method
USE_CARGO=false
if command -v cargo >/dev/null 2>&1; then
    USE_CARGO=true
fi

if [ "$USE_CARGO" = true ]; then
    echo "  → Using cargo install"
    cargo install dscode --root "$INSTALL_DIR/.." 2>&1 | tail -3
else
    # Fallback: download pre-built binary from GitHub releases
    VERSION="${DSCODE_VERSION:-latest}"
    if [ "$VERSION" = "latest" ]; then
        # Get latest release tag
        VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
    fi
    
    URL="https://github.com/$REPO/releases/download/$VERSION/dscode-$OS-$ARCH"
    
    echo "  → Downloading $URL"
    mkdir -p "$INSTALL_DIR"
    curl -fsSL "$URL" -o "$INSTALL_DIR/$BIN_NAME"
    chmod +x "$INSTALL_DIR/$BIN_NAME"
    
    echo ""
    echo "  ✓ Installed to $INSTALL_DIR/$BIN_NAME"
    echo "  ✓ Binary size: $(wc -c < "$INSTALL_DIR/$BIN_NAME" | numfmt --to=iec 2>/dev/null || echo '?')"
fi

# Verify installation
if command -v dscode >/dev/null 2>&1 || [ -f "$INSTALL_DIR/$BIN_NAME" ]; then
    echo ""
    echo "  ✓ dscode installed successfully!"
    echo ""
    echo "  Next steps:"
    echo "    1. dscode auth login     # Set your DeepSeek API key"
    echo "    2. dscode chat           # Start chatting"
    echo ""
    echo "  Docs: https://dscode.org"
else
    echo ""
    echo "  ✗ Installation may have failed. Check your PATH."
    echo "    The binary is at: $INSTALL_DIR/$BIN_NAME"
    echo "    Add to PATH: export PATH=\"\$PATH:$INSTALL_DIR\""
    exit 1
fi
