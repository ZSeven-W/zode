#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CI="$ROOT/.github/workflows/ci.yml"
RELEASE="$ROOT/.github/workflows/release.yml"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || fail "$file is missing: $text"
}

require_text "$CI" "workflow_dispatch:"
require_text "$CI" "update_snapshots:"
require_text "$CI" "cargo test --locked -p zode-app --test snapshots"
require_text "$CI" "ZODE_UPDATE_SNAPSHOTS: \"1\""
require_text "$CI" "inputs.update_snapshots && '1' || '0'"
require_text "$CI" "crates/zode-app/tests/snapshots/"
require_text "$CI" "aarch64-apple-ios"
require_text "$CI" "aarch64-linux-android"
for runner in ubuntu-24.04 macos-15 windows-latest; do
  require_text "$CI" "$runner"
done

require_text "$RELEASE" "build-app-release.sh"
require_text "$RELEASE" "zode-desktop-"
for target in aarch64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  require_text "$RELEASE" "$target"
done

echo "desktop workflow contracts passed"
