#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:?usage: package-rust-sdk.sh <version> <output.tar.gz>}"
OUTPUT="${2:?usage: package-rust-sdk.sh <version> <output.tar.gz>}"

ACTUAL="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
if [ "$ACTUAL" != "$VERSION" ]; then
  echo "Rust SDK package version mismatch: expected $VERSION, workspace is $ACTUAL" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
OUTPUT="$(cd "$(dirname "$OUTPUT")" && pwd)/$(basename "$OUTPUT")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

BUNDLE="zode-sdk-rust-$VERSION"
mkdir -p "$TMP/$BUNDLE/crates" "$TMP/$BUNDLE/sdk"
cp -R "$ROOT/crates/zode-app-server-protocol" "$TMP/$BUNDLE/crates/"
cp -R "$ROOT/sdk/rust" "$TMP/$BUNDLE/sdk/"
cp -R "$ROOT/sdk/fixtures" "$TMP/$BUNDLE/sdk/"
cp "$ROOT/LICENSE" "$TMP/$BUNDLE/"
sed "s/@VERSION@/$VERSION/g" "$ROOT/scripts/rust-sdk-workspace.toml" \
  > "$TMP/$BUNDLE/Cargo.toml"

find "$TMP/$BUNDLE" -name '.DS_Store' -delete
COPYFILE_DISABLE=1 tar -czf "$OUTPUT" -C "$TMP" "$BUNDLE"
echo "$OUTPUT"
