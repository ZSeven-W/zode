#!/usr/bin/env python3
"""Run Noema LOCOMO host-LLM tasks through zode headless.

Input is the JSONL emitted by:

  noema bench --locomo-answer-tasks-output ...
  noema bench --locomo-judge-tasks-output ...

Output is JSONL that Noema can read back with:

  noema bench --locomo-answer-results ...
  noema bench --locomo-judge-results ...
"""

import argparse
import concurrent.futures
import hashlib
import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
PROMPT_CHARS_PER_TOKEN_ESTIMATE = 4
JUDGE_LABELS = {"CORRECT", "WRONG"}
PROVIDER_BLOCKER_REASONS = {"http_402_payment_required"}


def default_zode_bin(root: Path = ROOT) -> Path:
    release_bin = root / "target" / "release" / "zode"
    if release_bin.exists():
        return release_bin
    debug_bin = root / "target" / "debug" / "zode"
    if debug_bin.exists():
        return debug_bin
    return release_bin


DEFAULT_ZODE_BIN = default_zode_bin()


def prompt_from_task(task: dict) -> str:
    messages = task.get("messages") or []
    if not messages:
        return str(task.get("prompt", ""))
    if len(messages) == 1 and messages[0].get("role") == "user":
        return str(messages[0].get("content", ""))
    parts = []
    for message in messages:
        role = str(message.get("role", "user")).strip().title() or "User"
        content = str(message.get("content", ""))
        parts.append(f"{role}:\n{content}")
    return "\n\n".join(parts)


def prompt_sha256(task: dict) -> str:
    return hashlib.sha256(prompt_from_task(task).encode("utf-8")).hexdigest()


def task_fingerprint(task: dict) -> str:
    payload = {
        "custom_id": task.get("custom_id"),
        "kind": task.get("kind"),
        "prompt_sha256": prompt_sha256(task),
    }
    encoded = json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def result_for_task(task: dict, stdout: str, stderr: str, secs: float) -> dict:
    kind = task.get("kind")
    result = {
        "custom_id": task["custom_id"],
        "kind": kind,
        "secs": secs,
        "prompt_sha256": prompt_sha256(task),
        "task_fingerprint": task_fingerprint(task),
    }
    if stderr.strip():
        result["stderr"] = stderr.strip()

    if kind == "locomo_judge":
        parsed = extract_json_object(stdout)
        if not isinstance(parsed, dict):
            result.update(
                {
                    "label": "WRONG",
                    "reasoning": "zode judge output did not contain a JSON object",
                    "raw": stdout.strip(),
                }
            )
            return result
        label = str(parsed.get("label", "")).strip().upper()
        reasoning = str(parsed.get("reasoning") or parsed.get("reason") or "").strip()
        if label not in JUDGE_LABELS:
            reasoning = reasoning or f"invalid judge label: {label or '(empty)'}"
            label = "WRONG"
        result.update({"label": label, "reasoning": reasoning, "raw": stdout.strip()})
        return result

    result["answer"] = stdout.strip()
    return result


def extract_json_object(text: str):
    decoder = json.JSONDecoder()
    for index, char in enumerate(text):
        if char != "{":
            continue
        try:
            value, _ = decoder.raw_decode(text[index:])
        except json.JSONDecodeError:
            continue
        return value
    return None


def load_tasks(path: Path, limit: int | None) -> list[dict]:
    tasks = []
    with path.open() as handle:
        for line_number, line in enumerate(handle, start=1):
            line = line.strip()
            if not line:
                continue
            task = json.loads(line)
            if "custom_id" not in task:
                raise ValueError(f"{path}:{line_number}: missing custom_id")
            if "kind" not in task:
                raise ValueError(f"{path}:{line_number}: missing kind")
            tasks.append(task)
            if limit is not None and len(tasks) >= limit:
                break
    return tasks


def existing_custom_ids(path: Path) -> set[str]:
    return completed_custom_ids(path, retry_empty=False, retry_failed=False)


def completed_custom_ids(path: Path, retry_empty: bool, retry_failed: bool = False) -> set[str]:
    if not path.exists():
        return set()
    _, latest, _ = latest_results(path)
    ids = set()
    for custom_id, value in latest.items():
        if retry_empty and retryable_result(value):
            continue
        if retry_failed and failed_result(value):
            continue
        ids.add(custom_id)
    return ids


def latest_results(path: Path) -> tuple[int, dict[str, dict], int]:
    rows = 0
    malformed_rows = 0
    latest = {}
    if not path.exists():
        return rows, latest, malformed_rows
    with path.open() as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            rows += 1
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                malformed_rows += 1
                continue
            custom_id = value.get("custom_id")
            if isinstance(custom_id, str):
                latest[custom_id] = value
    return rows, latest, malformed_rows


def pending_tasks(tasks: list[dict], done_ids: set[str]) -> list[dict]:
    return [task for task in tasks if task["custom_id"] not in done_ids]


def reusable_result_for_task(task: dict, sources: list[Path]) -> tuple[dict | None, Path | None]:
    fingerprint = task_fingerprint(task)
    custom_id = task["custom_id"]
    for source in sources:
        _, latest, _ = latest_results(source)
        result = latest.get(custom_id)
        if not result:
            continue
        if result.get("task_fingerprint") != fingerprint:
            continue
        if retryable_result(result) or failed_result(result):
            continue
        return result, source
    return None, None


def reuse_completed_results(
    tasks: list[dict],
    output: Path,
    sources: list[Path],
) -> tuple[list[dict], int]:
    if not sources:
        return tasks, 0

    remaining = []
    reused = 0
    for task in tasks:
        result, source = reusable_result_for_task(task, sources)
        if result is None or source is None:
            remaining.append(task)
            continue
        copied = dict(result)
        copied["reused"] = True
        copied["reused_from"] = str(source)
        append_result(output, copied)
        reused += 1
    return remaining, reused


def append_result(path: Path, result: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a") as handle:
        handle.write(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
        handle.write("\n")
        handle.flush()


def progress_message(completed: int, total: int, skipped: int, result: dict) -> str:
    return (
        f"[{completed}/{total} skipped={skipped}] "
        f"{result['custom_id']} {result.get('secs', 0)}s"
    )


def retryable_result(result: dict) -> bool:
    kind = result.get("kind")
    if kind == "locomo_answer_generation":
        return not str(result.get("answer", "")).strip() or failed_result(result)
    if kind == "locomo_judge":
        return (
            result.get("label") == "WRONG"
            and result.get("reasoning") == "zode judge output did not contain a JSON object"
        )
    return False


def failed_result(result: dict) -> bool:
    answer = str(result.get("answer", "")).strip()
    if answer.startswith("zode exited with status"):
        return True
    if answer == "zode task timed out":
        return True
    return False


def summarize_results(path: Path) -> dict:
    rows, latest, malformed_rows = latest_results(path)
    answers = {
        "total": 0,
        "valid": 0,
        "empty": 0,
        "failed": 0,
        "retryable": 0,
        "failure_reasons": {},
    }
    judges = {
        "total": 0,
        "valid": 0,
        "correct": 0,
        "wrong": 0,
        "retryable": 0,
        "failure_reasons": {},
    }
    for result in latest.values():
        kind = result.get("kind")
        if kind == "locomo_answer_generation":
            answers["total"] += 1
            if failed_result(result):
                answers["failed"] += 1
                answers["retryable"] += 1
                reason = failure_reason(result)
                answers["failure_reasons"][reason] = (
                    answers["failure_reasons"].get(reason, 0) + 1
                )
            elif not str(result.get("answer", "")).strip():
                answers["empty"] += 1
                answers["retryable"] += 1
            else:
                answers["valid"] += 1
        elif kind == "locomo_judge":
            judges["total"] += 1
            if retryable_result(result):
                judges["retryable"] += 1
                reason = failure_reason(result)
                judges["failure_reasons"][reason] = (
                    judges["failure_reasons"].get(reason, 0) + 1
                )
            elif result.get("label") in JUDGE_LABELS:
                judges["valid"] += 1
                if result.get("label") == "CORRECT":
                    judges["correct"] += 1
                else:
                    judges["wrong"] += 1
    return {
        "rows": rows,
        "unique": len(latest),
        "malformed_rows": malformed_rows,
        "answers": answers,
        "judges": judges,
    }


def empty_summary() -> dict:
    return {
        "rows": 0,
        "unique": 0,
        "malformed_rows": 0,
        "answers": {
            "total": 0,
            "valid": 0,
            "empty": 0,
            "failed": 0,
            "retryable": 0,
            "failure_reasons": {},
        },
        "judges": {
            "total": 0,
            "valid": 0,
            "correct": 0,
            "wrong": 0,
            "retryable": 0,
            "failure_reasons": {},
        },
    }


def failure_reason(result: dict) -> str:
    answer = str(result.get("answer", "")).strip()
    stderr = str(result.get("stderr", ""))
    text = f"{answer}\n{stderr}"
    if answer == "zode task timed out":
        return "timeout"
    if "HTTP 402" in text or "Insufficient Balance" in text:
        return "http_402_payment_required"
    if answer.startswith("zode exited with status"):
        return "zode_nonzero_exit"
    return "unknown_failure"


def provider_blocker_reason(result: dict) -> str | None:
    kind = result.get("kind")
    if kind == "locomo_answer_generation":
        if not failed_result(result):
            return None
    elif kind == "locomo_judge":
        if not retryable_result(result):
            return None
    else:
        return None
    reason = failure_reason(result)
    if reason in PROVIDER_BLOCKER_REASONS:
        return reason
    return None


def summary_line(path: Path, summary: dict) -> str:
    return (
        f"summary {path} rows={summary['rows']} unique={summary['unique']} "
        f"answer_retryable={summary['answers']['retryable']} "
        f"judge_retryable={summary['judges']['retryable']}"
    )


def summary_retryable_count(summary: dict) -> int:
    return int(summary["answers"]["retryable"]) + int(summary["judges"]["retryable"])


def summary_provider_blocker_reason(summary: dict) -> str | None:
    for section in ("answers", "judges"):
        reasons = summary.get(section, {}).get("failure_reasons", {})
        for reason in sorted(PROVIDER_BLOCKER_REASONS):
            if int(reasons.get(reason, 0)) > 0:
                return reason
    return None


def task_input_stats(path: Path, tasks: list[dict]) -> dict:
    lengths = [len(prompt_from_task(task)) for task in tasks]
    return {
        "task_file_bytes": path.stat().st_size,
        "tasks_loaded": len(tasks),
        "prompt_chars": prompt_char_stats(lengths),
        "estimated_prompt_tokens": estimated_prompt_token_stats(lengths),
        "noema_prompt_stats": noema_prompt_stats(tasks),
    }


def prompt_char_stats(lengths: list[int]) -> dict:
    if not lengths:
        return {
            "total": 0,
            "mean": 0.0,
            "p50": 0,
            "p95": 0,
            "max": 0,
        }
    ordered = sorted(lengths)
    return {
        "total": sum(lengths),
        "mean": sum(lengths) / len(lengths),
        "p50": percentile(ordered, 0.50),
        "p95": percentile(ordered, 0.95),
        "max": ordered[-1],
    }


def estimated_prompt_token_stats(lengths: list[int]) -> dict:
    estimates = [
        math.ceil(length / PROMPT_CHARS_PER_TOKEN_ESTIMATE) if length > 0 else 0
        for length in lengths
    ]
    return {
        "chars_per_token": PROMPT_CHARS_PER_TOKEN_ESTIMATE,
        "method": "ceil(prompt_chars / chars_per_token) per task",
        **prompt_char_stats(estimates),
    }


def noema_prompt_stats(tasks: list[dict]) -> dict | None:
    stats = [task.get("prompt_stats") for task in tasks if isinstance(task.get("prompt_stats"), dict)]
    if not stats:
        return None

    def int_values(field: str) -> list[int]:
        values = []
        for item in stats:
            value = item.get(field)
            if isinstance(value, int):
                values.append(value)
        return values

    budgets = sorted(
        {
            item.get("prompt_char_budget")
            for item in stats
            if isinstance(item.get("prompt_char_budget"), int)
        }
    )
    retrieval_in_prompt = int_values("retrieval_results_in_prompt")
    omitted = int_values("omitted_retrieval_results")
    truncated = int_values("truncated_memories")
    return {
        "tasks_with_prompt_stats": len(stats),
        "prompt_char_budgets": budgets,
        "retrieval_results_in_prompt": prompt_char_stats(retrieval_in_prompt),
        "omitted_retrieval_results": prompt_char_stats(omitted),
        "omitted_retrieval_tasks": sum(1 for value in omitted if value > 0),
        "truncated_memories": prompt_char_stats(truncated),
        "truncated_memory_tasks": sum(1 for value in truncated if value > 0),
    }


def percentile(ordered: list[int], value: float) -> int:
    index = math.ceil((len(ordered) - 1) * value)
    return ordered[index]


def manifest_from_args(
    args: argparse.Namespace,
    summary: dict,
    mode: str,
    tasks_total: int,
    run_count: int,
    skipped: int,
    reused_count: int = 0,
    input_stats: dict | None = None,
    provider_blocker: str | None = None,
    pending_before_run: int | None = None,
) -> dict:
    retryable_total = summary_retryable_count(summary)
    provider_blocker = provider_blocker or summary_provider_blocker_reason(summary)
    pending = run_count if pending_before_run is None else pending_before_run
    unrun_due_to_provider_blocker = max(0, pending - run_count) if provider_blocker else 0
    return {
        "runner": "zode",
        "mode": mode,
        "provider": args.provider,
        "model": args.model,
        "zode_bin": str(args.zode_bin),
        "paths": {
            "tasks": str(args.tasks) if args.tasks else None,
            "output": str(args.output) if args.output else None,
            "summary_output": str(args.summary_output) if args.summary_output else None,
            "manifest_output": str(args.manifest_output) if args.manifest_output else None,
            "zode_config_dir": str(args.zode_config_dir) if args.zode_config_dir else None,
            "reuse_results_from": [str(path) for path in args.reuse_results_from],
            "cwd": str(args.cwd) if args.cwd else None,
        },
        "execution": {
            "jobs": args.jobs,
            "limit": args.limit,
            "timeout": args.timeout,
            "no_sandbox": args.no_sandbox,
            "resume": args.resume,
            "retry_empty": args.retry_empty,
            "retry_failed": args.retry_failed,
            "retries": args.retries,
            "fail_on_retryable": args.fail_on_retryable,
            "stop_on_provider_blocker": args.stop_on_provider_blocker,
            "tasks_total": tasks_total,
            "pending_before_run": pending,
            "run": run_count,
            "skipped": skipped,
            "reused": reused_count,
            "unrun_due_to_provider_blocker": unrun_due_to_provider_blocker,
        },
        "input": input_stats,
        "summary": summary,
        "retryable_total": retryable_total,
        "retryable_clean": retryable_total == 0,
        "provider_blocked": provider_blocker is not None,
        "provider_blocker_reason": provider_blocker,
    }


def write_json(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")


def run_zode_task(task: dict, args: argparse.Namespace) -> dict:
    prompt = prompt_from_task(task)
    command = [args.zode_bin, "-p", prompt, "--yolo"]
    if args.provider:
        command.extend(["--provider", args.provider])
    if args.model:
        command.extend(["--model", args.model])
    if args.no_sandbox:
        command.append("--no-sandbox")
    if args.cwd:
        command.extend(["--cwd", args.cwd])

    attempts = max(1, args.retries + 1)
    last_result = None
    for attempt in range(1, attempts + 1):
        started = time.time()
        try:
            env = None
            if args.zode_config_dir:
                env = os.environ.copy()
                env["ZODE_CONFIG_DIR"] = str(args.zode_config_dir)
            proc = subprocess.run(
                command,
                capture_output=True,
                text=True,
                timeout=args.timeout,
                cwd=args.cwd or None,
                env=env,
            )
            stdout = proc.stdout
            stderr = proc.stderr
            if proc.returncode != 0 and not stdout.strip():
                stdout = f"zode exited with status {proc.returncode}"
        except subprocess.TimeoutExpired as exc:
            stdout = "zode task timed out"
            stderr = (exc.stderr or "") if isinstance(exc.stderr, str) else ""
        secs = round(time.time() - started, 3)
        last_result = result_for_task(task, stdout, stderr, secs)
        last_result["attempts"] = attempt
        if not retryable_result(last_result):
            return last_result
    return last_result


def record_result(result: dict, args: argparse.Namespace, completed: int, total: int) -> None:
    append_result(args.output, result)
    print(progress_message(completed, total, args.skipped, result), file=sys.stderr, flush=True)


def run_tasks(tasks: list[dict], args: argparse.Namespace) -> tuple[int, str | None]:
    total = len(tasks)
    if args.jobs <= 1:
        count = 0
        for task in tasks:
            count += 1
            result = run_zode_task(task, args)
            record_result(result, args, count, total)
            if args.stop_on_provider_blocker:
                blocker = provider_blocker_reason(result)
                if blocker:
                    print(
                        f"provider_blocker={blocker} stopping after {count}/{total}",
                        file=sys.stderr,
                        flush=True,
                    )
                    return count, blocker
        return count, None
    count = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as executor:
        futures = [executor.submit(run_zode_task, task, args) for task in tasks]
        for future in concurrent.futures.as_completed(futures):
            count += 1
            record_result(future.result(), args, count, total)
    return count, None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tasks", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--zode-bin", default=os.environ.get("ZODE_BIN", str(DEFAULT_ZODE_BIN)))
    parser.add_argument("--zode-config-dir", type=Path)
    parser.add_argument("--provider")
    parser.add_argument("--model")
    parser.add_argument("--cwd")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--no-sandbox", action="store_true")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--retry-empty", action="store_true")
    parser.add_argument("--retry-failed", action="store_true")
    parser.add_argument("--summary-output", type=Path)
    parser.add_argument("--summary-only", action="store_true")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--fail-on-retryable", action="store_true")
    parser.add_argument("--stop-on-provider-blocker", action="store_true")
    parser.add_argument("--manifest-output", type=Path)
    parser.add_argument("--retries", type=int, default=1)
    parser.add_argument("--reuse-results-from", type=Path, action="append", default=[])
    args = parser.parse_args()

    if args.jobs < 1:
        parser.error("--jobs must be >= 1")
    if args.retries < 0:
        parser.error("--retries must be >= 0")
    if args.stop_on_provider_blocker and args.jobs != 1:
        parser.error("--stop-on-provider-blocker requires --jobs 1")
    if args.dry_run:
        if args.tasks is None:
            parser.error("--tasks is required with --dry-run")
        tasks = load_tasks(args.tasks, args.limit)
        input_stats = task_input_stats(args.tasks, tasks)
        summary = empty_summary()
        if args.summary_output:
            write_json(args.summary_output, summary)
        if args.manifest_output:
            write_json(
                args.manifest_output,
                manifest_from_args(
                    args,
                    summary,
                    mode="dry_run",
                    tasks_total=len(tasks),
                    run_count=0,
                    skipped=0,
                    input_stats=input_stats,
                ),
            )
        prompt_chars = input_stats["prompt_chars"]
        print(
            "dry-run "
            f"tasks={len(tasks)} "
            f"task_file_bytes={input_stats['task_file_bytes']} "
            f"prompt_chars_total={prompt_chars['total']}"
        )
        return 0
    if args.summary_only:
        if args.output is None:
            parser.error("--output is required with --summary-only")
        input_stats = None
        task_count = 0
        if args.tasks:
            tasks = load_tasks(args.tasks, args.limit)
            task_count = len(tasks)
            input_stats = task_input_stats(args.tasks, tasks)
        summary = summarize_results(args.output)
        if args.summary_output:
            write_json(args.summary_output, summary)
        if args.manifest_output:
            write_json(
                args.manifest_output,
                manifest_from_args(
                    args,
                    summary,
                    mode="summary_only",
                    tasks_total=task_count,
                    run_count=0,
                    skipped=0,
                    input_stats=input_stats,
                ),
            )
        print(summary_line(args.output, summary))
        if args.fail_on_retryable and summary_retryable_count(summary) > 0:
            if summary_provider_blocker_reason(summary):
                return 3
            return 2
        return 0
    if args.tasks is None:
        parser.error("--tasks is required unless --summary-only is set")
    if args.output is None:
        parser.error("--output is required unless --dry-run is set")
    tasks = load_tasks(args.tasks, args.limit)
    input_total = len(tasks)
    input_stats = task_input_stats(args.tasks, tasks)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    skipped = 0
    if args.resume:
        done_ids = completed_custom_ids(
            args.output,
            retry_empty=args.retry_empty,
            retry_failed=args.retry_failed,
        )
        skipped = len([task for task in tasks if task["custom_id"] in done_ids])
        tasks = pending_tasks(tasks, done_ids)
    else:
        args.output.write_text("")
    args.skipped = skipped
    tasks, reused = reuse_completed_results(tasks, args.output, args.reuse_results_from)
    pending_before_run = len(tasks)
    count, provider_blocker = run_tasks(tasks, args)
    print(f"wrote {args.output} results={count} skipped={skipped}")
    summary = summarize_results(args.output)
    if args.summary_output:
        write_json(args.summary_output, summary)
    if args.manifest_output:
        write_json(
            args.manifest_output,
            manifest_from_args(
                args,
                summary,
                mode="run",
                tasks_total=input_total,
                run_count=count,
                skipped=skipped,
                reused_count=reused,
                input_stats=input_stats,
                provider_blocker=provider_blocker,
                pending_before_run=pending_before_run,
            ),
        )
    print(summary_line(args.output, summary))
    if provider_blocker:
        return 3
    if args.fail_on_retryable and summary_retryable_count(summary) > 0:
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
