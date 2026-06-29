#!/usr/bin/env bash
# Build a release `zode` binary for one target and package it into dist/.
# Usage: build-release.sh <target-triple> <package-name>
# Example: build-release.sh aarch64-apple-darwin zode-0.2.0-arm64-mac
#
# The release CI builds every target on a NATIVE runner (incl. native arm64
# Linux/Windows), so plain `cargo build` works everywhere. The `*-linux-musl`
# branch keeps cargo-zigbuild around for local static builds. Windows packages
# a .zip (PowerShell Compress-Archive — always present, unlike `zip`), Unix a
# .tar.gz. Shared by local use and the release CI workflow.
set -euo pipefail

TARGET="${1:?usage: build-release.sh <target-triple> <package-name>}"
PKG="${2:?usage: build-release.sh <target-triple> <package-name>}"
OUT_DIR="dist"

mkdir -p "$OUT_DIR"

case "$TARGET" in
  *linux*musl*)
    cargo zigbuild --release -p zode --target "$TARGET"
    ;;
  *)
    cargo build --release -p zode --target "$TARGET"
    ;;
esac

BIN_DIR="target/$TARGET/release"
if [[ "$TARGET" == *windows* ]]; then
  powershell -NoProfile -Command \
    "Compress-Archive -Path '$BIN_DIR/zode.exe' -DestinationPath '$OUT_DIR/$PKG.zip' -Force"
else
  tar -C "$BIN_DIR" -czf "$OUT_DIR/$PKG.tar.gz" "zode"
fi

echo "packaged $OUT_DIR/$PKG"
