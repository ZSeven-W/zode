#!/usr/bin/env bash
# Run every language SDK's test suite. Each SDK also runs a live end-to-end
# contract against a real `zode server` when ZODE_BIN points at the binary;
# without it, the e2e cases skip themselves.
#
# Usage: scripts/test-sdks.sh [--strict]
#   --strict  A missing toolchain (pnpm / python3 / go / gradle) is a failure,
#             not a skip. CI runs strict so a silently-absent runtime cannot
#             hide a broken SDK; local runs default to lenient skipping.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

STRICT=0
for arg in "$@"; do
  case "$arg" in
    --strict) STRICT=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

# ZODE_BIN, when set, is exported so each SDK's e2e test can spawn the server.
if [ -n "${ZODE_BIN:-}" ]; then
  export ZODE_BIN
  echo "SDK e2e enabled: ZODE_BIN=$ZODE_BIN" >&2
else
  echo "SDK e2e disabled: ZODE_BIN unset (e2e cases will skip)" >&2
fi

# missing <toolchain> <name>: honor --strict (fail) vs lenient (skip). Returns
# non-zero in lenient mode so the caller's `|| true` swallows the skip.
missing() {
  local tool="$1" name="$2"
  if [ "$STRICT" -eq 1 ]; then
    echo "ERROR: $name SDK cannot run: $tool is not installed (--strict)" >&2
    exit 1
  fi
  echo "Skipping $name SDK tests: $tool not installed" >&2
  return 1
}

# --- Rust (always available: it's the workspace toolchain) ---
cargo test -p zode-sdk-rust

# --- TypeScript ---
if command -v pnpm >/dev/null 2>&1; then
  if [ ! -d "$ROOT/sdk/typescript/node_modules" ]; then
    pnpm --dir "$ROOT/sdk/typescript" install --frozen-lockfile
  fi
  pnpm --dir "$ROOT/sdk/typescript" test
else
  missing pnpm TypeScript || true
fi

# --- Python (stdlib unittest, no third-party deps) ---
if command -v python3 >/dev/null 2>&1; then
  PYTHONPATH="$ROOT/sdk/python/src" python3 -m unittest discover -s "$ROOT/sdk/python/tests"
else
  missing python3 Python || true
fi

# --- Go ---
if command -v go >/dev/null 2>&1; then
  (cd "$ROOT/sdk/go" && go test ./...)
else
  missing go Go || true
fi

# --- Kotlin ---
if [ -x "$ROOT/sdk/kotlin/gradlew" ]; then
  (cd "$ROOT/sdk/kotlin" && ./gradlew --no-daemon test)
elif command -v gradle >/dev/null 2>&1; then
  (cd "$ROOT/sdk/kotlin" && gradle --no-daemon test)
else
  missing gradle Kotlin || true
fi

echo "All available SDK suites passed." >&2
