#!/usr/bin/env bash
# Pack the zode bridge extension and print its stable extension id.
set -euo pipefail

dir="$(cd "$(dirname "$0")" && pwd)"
default_pem="$dir/zode-bridge.pem"
legacy_pem="$dir/../zode-bridge.pem"
if [ -n "${ZODE_EXT_PEM:-}" ]; then
  pem="$ZODE_EXT_PEM"
elif [ -f "$default_pem" ]; then
  pem="$default_pem"
elif [ -f "$legacy_pem" ]; then
  pem="$legacy_pem"
else
  pem="$default_pem"
fi
pem="$(cd "$(dirname "$pem")" && pwd)/$(basename "$pem")"
crx="$dir/zode-bridge.crx"

find_chrome() {
  if [ -n "${CHROME:-}" ]; then
    printf '%s\n' "$CHROME"
    return
  fi
  for candidate in \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    google-chrome \
    chromium \
    chromium-browser \
    chrome; do
    if command -v "$candidate" >/dev/null 2>&1 || [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return
    fi
  done
  return 1
}

if [ ! -f "$pem" ]; then
  openssl genrsa 2048 >"$pem"
  chmod 600 "$pem"
fi

pub_der="$(mktemp)"
pack_dir="$(mktemp -d)"
trap 'rm -f "$pub_der"; rm -rf "$pack_dir" "$pack_dir.crx"' EXIT
openssl rsa -in "$pem" -pubout -outform DER -out "$pub_der" 2>/dev/null

if base64 --help 2>&1 | grep -q -- '-w'; then
  pub_der_b64="$(base64 -w0 "$pub_der")"
else
  pub_der_b64="$(base64 <"$pub_der" | tr -d '\n')"
fi
id="$(openssl dgst -sha256 -binary "$pub_der" | head -c16 | xxd -p -c256 | tr '0-9a-f' 'a-p')"

echo "key: $pub_der_b64"
echo "extension id: $id"

chrome="$(find_chrome || true)"
if [ -z "$chrome" ]; then
  echo "Chrome not found; set CHROME=/path/to/chrome to pack the extension" >&2
  exit 1
fi

cp "$dir/manifest.json" "$dir/background.js" "$dir/popup.html" "$dir/popup.js" "$pack_dir/"
if [ -d "$dir/icons" ]; then
  cp -R "$dir/icons" "$pack_dir/"
fi

rm -f "$crx" "$pack_dir.crx"
"$chrome" --pack-extension="$pack_dir" --pack-extension-key="$pem" >/tmp/zode-bridge-pack.log 2>&1 || {
  cat /tmp/zode-bridge-pack.log >&2
  exit 1
}
if [ -f "$pack_dir.crx" ]; then
  mv "$pack_dir.crx" "$crx"
fi
if [ ! -f "$crx" ]; then
  cat /tmp/zode-bridge-pack.log >&2
  echo "Chrome did not produce $crx" >&2
  exit 1
fi
echo "packed: $crx"
