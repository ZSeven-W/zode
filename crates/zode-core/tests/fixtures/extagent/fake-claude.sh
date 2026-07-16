#!/bin/sh
# fake claude CLI: consume stdin fully, then emit the claude stream fixture
cat > /dev/null
cat "$(dirname "$0")/claude-stream.jsonl"
