#!/usr/bin/env bash
# Build the Chrome Web Store upload ZIP for the zode bridge extension.
#
# Differences from pack.sh (the local CRX pack):
# - The manifest's `key` field is stripped: the Web Store mints its own key
#   pair and ID on first upload and rejects developer-keyed manifests.
# - Output is a plain ZIP (the dashboard takes ZIPs, not CRXs).
#
# After the first publish, the store shows the NEW extension id — zode
# accepts it via `browser.extensionIds` in ~/.zode/config.json (see
# store/PUBLISHING notes in README.md).
set -euo pipefail

dir="$(cd "$(dirname "$0")" && pwd)"
out="$dir/zode-bridge-store.zip"

if [ ! -f "$dir/dist/sidepanel-react.js" ] || [ ! -f "$dir/dist/sidepanel-react.css" ]; then
  echo "React side panel build is missing; run npm --prefix extensions/chrome install && npm --prefix extensions/chrome run build" >&2
  exit 1
fi

pack_dir="$(mktemp -d)"
trap 'rm -rf "$pack_dir"' EXIT

cp "$dir/background.js" "$dir/popup.html" "$dir/popup.js" "$dir/offscreen.html" "$dir/offscreen.js" "$dir/sidepanel.html" "$dir/sidepanel-state.js" "$dir/sidepanel.js" "$pack_dir/"
cp -R "$dir/dist" "$pack_dir/"
if [ -d "$dir/icons" ]; then
  cp -R "$dir/icons" "$pack_dir/"
fi

# Strip the developer `key` from the manifest for the store upload.
node -e '
  const fs = require("fs");
  const m = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  delete m.key;
  fs.writeFileSync(process.argv[2], JSON.stringify(m, null, 2) + "\n");
' "$dir/manifest.json" "$pack_dir/manifest.json"

rm -f "$out"
(cd "$pack_dir" && zip -qr "$out" .)
echo "store upload zip: $out"
echo "upload at https://chrome.google.com/webstore/devconsole — listing text lives in store/LISTING.md"
