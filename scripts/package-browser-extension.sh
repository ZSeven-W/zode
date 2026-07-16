#!/usr/bin/env bash
# Package the compiled Chrome extension as a clean, unpacked ZIP.
# Usage: package-browser-extension.sh <output.zip>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXTENSION_DIR="$ROOT/extensions/chrome"
OUTPUT="${1:?usage: package-browser-extension.sh <output.zip>}"

if [[ "$OUTPUT" != /* ]]; then
  OUTPUT="$ROOT/$OUTPUT"
fi

required_build=(
  "$EXTENSION_DIR/dist/sidepanel-react.css"
  "$EXTENSION_DIR/dist/sidepanel-react.js"
)
for file in "${required_build[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "missing compiled extension asset: $file" >&2
    echo "run npm --prefix extensions/chrome ci && npm --prefix extensions/chrome run build" >&2
    exit 1
  fi
done

runtime_files=(
  manifest.json
  background.js
  popup.html
  popup.js
  offscreen.html
  offscreen.js
  sidepanel.html
  sidepanel-state.js
  sidepanel.js
)

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

for file in "${runtime_files[@]}"; do
  cp "$EXTENSION_DIR/$file" "$stage/"
done
cp -R "$EXTENSION_DIR/dist" "$stage/dist"
cp -R "$EXTENSION_DIR/icons" "$stage/icons"

mkdir -p "$(dirname "$OUTPUT")"
rm -f "$OUTPUT"
(
  cd "$stage"
  zip -q -r "$OUTPUT" .
)

echo "packaged $OUTPUT"
