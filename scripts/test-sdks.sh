#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo test -p zode-sdk-rust

if command -v pnpm >/dev/null 2>&1; then
  if [ ! -d "$ROOT/sdk/typescript/node_modules" ]; then
    pnpm --dir "$ROOT/sdk/typescript" install
  fi
  pnpm --dir "$ROOT/sdk/typescript" test
else
  echo "Skipping TypeScript SDK tests: pnpm not installed" >&2
fi

PYTHONPATH="$ROOT/sdk/python/src" python3 -m unittest discover -s "$ROOT/sdk/python/tests"

if [ -x /usr/local/go/bin/go ]; then
  (cd "$ROOT/sdk/go" && GOROOT=/usr/local/go /usr/local/go/bin/go test ./...)
else
  (cd "$ROOT/sdk/go" && go test ./...)
fi

if [ -x "$ROOT/sdk/kotlin/gradlew" ]; then
  (cd "$ROOT/sdk/kotlin" && ./gradlew test)
elif command -v gradle >/dev/null 2>&1; then
  (cd "$ROOT/sdk/kotlin" && gradle test)
else
  echo "Skipping Kotlin SDK tests: Gradle is not installed" >&2
fi
