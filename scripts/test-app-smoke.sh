#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
snapshot="${TMPDIR:-/tmp}/zode-app-smoke.png"

cd "$ROOT"
rm -f "$snapshot"

cargo +1.94 test --locked -p zode-app --test end_to_end -- --nocapture
cargo +1.94 test --locked -p zode-app --test snapshots
cargo +1.94 run --locked -p zode-app -- --render-snapshot "$snapshot"

test -s "$snapshot"
echo "zode-app smoke snapshot: $snapshot"
