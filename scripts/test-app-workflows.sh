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

require_count_at_least() {
  local file="$1"
  local text="$2"
  local minimum="$3"
  local count
  count="$(grep -Fc -- "$text" "$file" || true)"
  ((count >= minimum)) || fail "$file needs at least $minimum occurrences of: $text"
}

require_block_text() {
  local label="$1"
  local block="$2"
  local text="$3"
  grep -Fq -- "$text" <<<"$block" || fail "$label is missing: $text"
}

require_text "$CI" "workflow_dispatch:"
require_text "$CI" "update_snapshots:"
require_text "$CI" "cargo test --locked -p zode-app --test snapshots"
require_text "$CI" 'verify-snapshots-${{ matrix.platform }}'
require_text "$CI" "Verify checked-in desktop screenshot goldens"
require_count_at_least "$CI" "cargo test --locked -p zode-app --test snapshots" 2
require_text "$CI" "ZODE_UPDATE_SNAPSHOTS: \"1\""
require_text "$CI" "inputs.update_snapshots && '1' || '0'"
require_text "$CI" "crates/zode-app/tests/snapshots/"
require_text "$CI" "aarch64-apple-ios"
require_text "$CI" "aarch64-linux-android"
require_text "$CI" "rustup target add --toolchain 1.94.0"
require_text "$CI" "cargo +1.94.0 check --locked"
require_text "$CI" "./scripts/test-app-workflows.sh"
require_text "$CI" "./scripts/test-build-app-release.sh"
require_text "$CI" "verify native macOS packaging contract"
require_count_at_least "$CI" "./scripts/test-build-app-release.sh" 2
for runner in ubuntu-24.04 macos-15 windows-latest; do
  require_text "$CI" "$runner"
done

VERIFY_SNAPSHOTS="$(sed -n '/^  verify-snapshots:/,/^  portable-core:/p' "$CI")"
require_block_text "verify-snapshots job" "$VERIFY_SNAPSHOTS" 'name: verify-snapshots-${{ matrix.platform }}'
require_block_text "verify-snapshots job" "$VERIFY_SNAPSHOTS" "cargo test --locked -p zode-app --test snapshots"
require_block_text "verify-snapshots job" "$VERIFY_SNAPSHOTS" 'key: snapshot-verify-${{ matrix.platform }}'
for platform in linux macos windows; do
  require_block_text "verify-snapshots job" "$VERIFY_SNAPSHOTS" "platform: $platform"
done
if grep -Eq '^    if:' <<<"$VERIFY_SNAPSHOTS"; then
  fail "verify-snapshots job must run on normal push and pull_request events"
fi

require_text "$RELEASE" "build-app-release.sh"
require_text "$RELEASE" "zode-desktop-"
require_text "$RELEASE" "zode-desktop-*.sha256"
for target in aarch64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
  require_text "$RELEASE" "$target"
done

echo "desktop workflow contracts passed"
