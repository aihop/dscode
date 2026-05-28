#!/bin/sh
# dscode cross-compile script
# Builds dscode for multiple targets in one shot.
# Prerequisites (one-time):
#   rustup target add aarch64-unknown-linux-gnu
#   rustup target add x86_64-unknown-linux-gnu
#   rustup target add aarch64-linux-android
#   (for macOS cross): brew install FiloSottile/musl-cross/musl-cross

set -eu

VERSION="${1:-$(git describe --tags --always 2>/dev/null || echo "0.1.0")}"
OUTPUT_DIR="dist"

echo "==> dscode cross-compile v$VERSION"
echo ""

BUILD_DIR="$OUTPUT_DIR/dscode-$VERSION"
mkdir -p "$BUILD_DIR"

build_target() {
    local TARGET="$1"
    local SUFFIX="${2:-}"
    
    echo "  Building for $TARGET..."
    
    if cargo build --release --target "$TARGET" -p dscode 2>/dev/null; then
        local SRC="target/$TARGET/release/dscode"
        local DST="$BUILD_DIR/dscode-$TARGET$SUFFIX"
        cp "$SRC" "$DST"
        local SIZE=$(wc -c < "$DST" | tr -d ' ')
        echo "  ✓ $TARGET — ${SIZE}B"
    else
        echo "  ✗ $TARGET — build failed (missing target?)"
    fi
}

echo "=== macOS ==="
build_target "aarch64-apple-darwin"
build_target "x86_64-apple-darwin"

echo ""
echo "=== Linux ==="
build_target "aarch64-unknown-linux-gnu"
build_target "x86_64-unknown-linux-gnu"
build_target "aarch64-unknown-linux-musl"
build_target "x86_64-unknown-linux-musl"

echo ""
echo "=== Android ==="
build_target "aarch64-linux-android"

echo ""
echo "=== Summary ==="
ls -lh "$BUILD_DIR/" 2>/dev/null || echo "  (no binaries built)"
echo ""
echo "Output: $BUILD_DIR/"
