#!/usr/bin/env bash
# Build a release `zode` binary for one target and package it into dist/.
# Usage: build-release.sh <target-triple> <package-name>
# Example: build-release.sh aarch64-apple-darwin zode-0.2.0-arm64-mac
#
# Linux musl targets cross-compile via cargo-zigbuild (install it + ziglang
# first); everything else uses plain cargo. Windows packages a .zip, Unix a
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
  ( cd "$BIN_DIR" && zip -q "../../../$OUT_DIR/$PKG.zip" "zode.exe" )
else
  tar -C "$BIN_DIR" -czf "$OUT_DIR/$PKG.tar.gz" "zode"
fi

echo "packaged $OUT_DIR/$PKG"
