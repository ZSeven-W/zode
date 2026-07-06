#!/usr/bin/env python3
"""Deterministic regression checks for Zode harness accuracy.

This suite captures failure modes seen in real Zode usage logs:
  - completion without fresh verification evidence
  - exports losing full tool-result detail
  - foreground Bash timeout being mistaken for a background shell_id
  - broad `git add -A` staging through Bash
  - FileEdit failures lacking recovery guidance
  - headless output hiding failed tool details and nonzero shell exits
  - compaction meta-instructions leaking into durable memory
  - sub-agents running without a bounded iteration budget
  - provider prompt + completion budgets exceeding the context window
  - Grep returning a whole minified line into model context
  - prompt recall history leaking across sessions/tabs
  - file mutation tool results hiding the resolved path/cwd from the user
  - shell commands inheriting the TUI controlling tty and drawing password prompts

Usage:
  python3 benchmarks/harness_regressions.py
  python3 benchmarks/harness_regressions.py --list
"""

import argparse
import errno
import os
import pty
import select
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
TARGET_DIR = "/tmp/zode-target-harness-regressions"

CHECKS = [
    {
        "id": "run_check_records_verification",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "verification::tests",
        ],
    },
    {
        "id": "goal_complete_requires_fresh_verification",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "goal::tests::call_sets_the_shared_flag_only_after_a_fresh_passed_verification",
        ],
    },
    {
        "id": "full_tool_trace_export",
        "cmd": ["cargo", "test", "-p", "zode-core", "tool_trace::tests"],
    },
    {
        "id": "markdown_export_references_tool_trace",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "export::tests::export_can_reference_full_tool_trace_file",
        ],
    },
    {
        "id": "compact_memory_noise_filter",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "compact_memory::tests::drops_compaction_meta_instructions_from_memory_sink",
        ],
    },
    {
        "id": "goal_prompt_mentions_run_check",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "engine::tests::goal_and_effort_inject_into_system_prompt",
        ],
    },
    {
        "id": "pre_turn_context_budget_reserves_completion",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-core",
            "engine::tests::pre_turn_compact_reserves_completion_budget",
        ],
    },
    {
        "id": "subagent_iteration_budget",
        "cmd": ["cargo", "test", "-p", "zode-core", "task_factory::tests"],
    },
    {
        "id": "bash_timeout_recovery_hint",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "--features",
            "shell,bash-async",
            "bash_timeout_kills_long_running_command",
        ],
    },
    {
        "id": "bash_detects_broad_git_staging",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "--features",
            "shell,bash-async",
            "broad_git_add_detection_blocks_only_unscoped_staging",
        ],
    },
    {
        "id": "bash_blocks_broad_git_staging",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "--features",
            "shell,bash-async",
            "bash_rejects_broad_git_staging",
        ],
    },
    {
        "id": "file_edit_recovery_hints",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "file_edit_",
        ],
    },
    {
        "id": "grep_truncates_minified_lines",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "grep_truncates_very_long_match_lines",
        ],
    },
    {
        "id": "prompt_history_is_session_scoped",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-tui",
            "prompt_history_is_isolated_between_session_tabs",
        ],
    },
    {
        "id": "tui_file_tool_result_location",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-tui",
            "file_tool_result_line_shows_resolved_path_and_active_cwd",
        ],
    },
    {
        "id": "headless_file_tool_result_location",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode",
            "tool_result_line_surfaces_file_result_location_and_cwd",
        ],
    },
    {
        "id": "headless_tool_result_summaries",
        "cmd": ["cargo", "test", "-p", "zode", "headless::tests"],
    },
    {
        "id": "bash_detaches_controlling_tty",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "--features",
            "shell",
            "bash_runs_without_a_controlling_tty",
            "--",
            "--nocapture",
        ],
        "pty": True,
    },
    {
        "id": "bash_run_detaches_controlling_tty",
        "cmd": [
            "cargo",
            "test",
            "--manifest-path",
            "vendor/agent/crates/agent-tools-code/Cargo.toml",
            "--features",
            "shell,bash-async",
            "run_starts_without_a_controlling_tty",
            "--",
            "--nocapture",
        ],
        "pty": True,
    },
    {
        "id": "local_shell_detaches_controlling_tty",
        "cmd": [
            "cargo",
            "test",
            "-p",
            "zode-tui",
            "local_shell_runs_without_a_controlling_tty",
            "--",
            "--nocapture",
        ],
        "pty": True,
    },
]


def run_pty(cmd: list[str], env: dict[str, str]) -> int:
    if os.name != "posix":
        return subprocess.run(cmd, cwd=ROOT, env=env, text=True).returncode

    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(ROOT)
        os.execvpe(cmd[0], cmd, env)

    try:
        while True:
            ready, _, _ = select.select([fd], [], [])
            if fd not in ready:
                continue
            try:
                data = os.read(fd, 8192)
            except OSError as err:
                if err.errno == errno.EIO:
                    break
                raise
            if not data:
                break
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
    finally:
        try:
            os.close(fd)
        except OSError:
            pass

    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status)


def run_check(check: dict) -> bool:
    env = dict(os.environ, CARGO_TARGET_DIR=TARGET_DIR)
    print(f"--- {check['id']} ---")
    if check.get("pty"):
        rc = run_pty(check["cmd"], env)
    else:
        rc = subprocess.run(check["cmd"], cwd=ROOT, env=env, text=True).returncode
    ok = rc == 0
    print(f"{'PASS' if ok else 'FAIL'} {check['id']}\n")
    return ok


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--list", action="store_true", help="list check ids and exit")
    args = parser.parse_args()
    if args.list:
        for check in CHECKS:
            print(check["id"])
        return 0

    passed = 0
    for check in CHECKS:
        passed += int(run_check(check))
    total = len(CHECKS)
    print(f"harness regression checks: {passed}/{total} passed")
    return 0 if passed == total else 1


if __name__ == "__main__":
    sys.exit(main())
