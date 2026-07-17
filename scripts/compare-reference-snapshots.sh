#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <approved-reference-dir> <actual-snapshot-dir> <artifact-root>" >&2
  exit 64
fi

reference_root=$1
actual_root=$2
output_root=$3

if [[ ! -d "$reference_root" ]]; then
  echo "reference directory does not exist: $reference_root" >&2
  exit 66
fi
if [[ ! -d "$actual_root" ]]; then
  echo "actual snapshot directory does not exist: $actual_root" >&2
  exit 66
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
workspace_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
required_output_root="$workspace_root/target/zode-app-snapshot-artifacts"
if [[ "$output_root" == "target/zode-app-snapshot-artifacts" && "$PWD" == "$workspace_root" ]]; then
  output_root=$required_output_root
elif [[ "$output_root" != "$required_output_root" ]]; then
  echo "artifact root must be $required_output_root" >&2
  exit 64
fi
mkdir -p "$output_root"

ZODE_REFERENCE_ROOT=$(CDPATH= cd -- "$reference_root" && pwd) \
ZODE_ACTUAL_ROOT=$(CDPATH= cd -- "$actual_root" && pwd) \
ZODE_REFERENCE_DIFF_ROOT=$(CDPATH= cd -- "$output_root" && pwd) \
  cargo +1.94 test \
    --manifest-path "$workspace_root/Cargo.toml" \
    -p zode-app \
    --test snapshots \
    compare_reference_snapshots \
    -- --ignored --exact --nocapture

echo "reference diagnostics written below $(CDPATH= cd -- "$output_root" && pwd)/<platform>/<scene>"
