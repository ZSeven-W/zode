#!/usr/bin/env python3
"""Run the Zode capability benchmark.

Two tracks, scored by the SAME hidden tests:
  - zode    : `zode -p "<prompt>" --yolo` driving DeepSeek (deepseek-v4-pro)
  - claude  : Claude's direct solutions (benchmarks/baseline_claude.py)

Each candidate's code + test runs in an isolated subprocess with a timeout,
so a hang or crash fails just that task. Usage/cache stats are parsed from
zode's stderr cost report when present.

Usage:
  python3 benchmarks/run.py --validate          # only score the Claude baseline (checks tests)
  python3 benchmarks/run.py --track claude       # score Claude baseline, write results
  python3 benchmarks/run.py --track zode [-jN]    # run Zode+DeepSeek, score, write results
  python3 benchmarks/run.py --track both          # both, then a comparison table
Environment: ZODE_BIN (default target/release/zode), ZODE_CONFIG_DIR (DeepSeek config).
"""
import argparse
import concurrent.futures
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
from suite import TASKS, by_dimension  # noqa: E402
from baseline_claude import SOLUTIONS  # noqa: E402

_zbin = os.environ.get("ZODE_BIN", str(ROOT / "target" / "release" / "zode"))
# Resolve to an absolute path: each zode call runs in a temp cwd, and a relative
# executable would otherwise be looked up against that temp dir.
ZODE_BIN = str((ROOT / _zbin).resolve() if not os.path.isabs(_zbin) else Path(_zbin))
TEST_TIMEOUT = 10
ZODE_TIMEOUT = 180

CODE_BLOCK = re.compile(r"```(?:python|py)?\s*\n(.*?)```", re.DOTALL)


def extract_code(text: str) -> str:
    """Pull the largest fenced code block; fall back to the raw text."""
    blocks = CODE_BLOCK.findall(text)
    if blocks:
        return max(blocks, key=len)
    return text


def score(code: str, test: str) -> tuple[bool, str]:
    """Run candidate code + test in an isolated subprocess. (passed, detail)."""
    program = code + "\n\n" + test + "\nprint('__BENCH_OK__')\n"
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as f:
        f.write(program)
        path = f.name
    try:
        r = subprocess.run(
            [sys.executable, path],
            capture_output=True, text=True, timeout=TEST_TIMEOUT,
        )
        if r.returncode == 0 and "__BENCH_OK__" in r.stdout:
            return True, ""
        err = (r.stderr or r.stdout).strip().splitlines()
        return False, (err[-1] if err else f"exit {r.returncode}")
    except subprocess.TimeoutExpired:
        return False, "timeout"
    finally:
        os.unlink(path)


USAGE_RE = re.compile(r"[↑^](\d+)\s*[↓v](\d+)")
CACHE_RE = re.compile(r"cache[^\d]*(\d+)", re.IGNORECASE)


# One shared, empty work dir for ALL zode calls. Keeps --yolo tool writes out
# of the repo (no pollution / stray project instructions) while holding cwd
# constant, so the system-prompt+tools prefix is byte-identical across tasks —
# which is what lets DeepSeek's prefix cache hit (mirrors a real session).
_WORK = tempfile.mkdtemp(prefix="zode-bench-")


def run_zode(task: dict) -> dict:
    """Invoke zode headless on the task prompt; return code + stats."""
    t0 = time.time()
    try:
        r = subprocess.run(
            [ZODE_BIN, "-p", task["prompt"], "--yolo"],
            capture_output=True, text=True, timeout=ZODE_TIMEOUT,
            cwd=_WORK,
        )
        out, err = r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return {"code": "", "raw": "", "stderr": "timeout", "secs": ZODE_TIMEOUT}
    except FileNotFoundError:
        sys.exit(f"zode binary not found at {ZODE_BIN} (set ZODE_BIN or build it)")
    secs = round(time.time() - t0, 1)
    return {"code": extract_code(out), "raw": out, "stderr": err, "secs": secs}


def parse_usage(stderr: str) -> dict:
    m = USAGE_RE.search(stderr)
    inp = int(m.group(1)) if m else 0
    out = int(m.group(2)) if m else 0
    cm = CACHE_RE.search(stderr)
    cache = int(cm.group(1)) if cm else 0
    return {"input_tokens": inp, "output_tokens": out, "cache_read": cache}


def run_track(track: str, jobs: int) -> list[dict]:
    results = []
    if track == "claude":
        for task in TASKS:
            passed, detail = score(SOLUTIONS.get(task["id"], ""), task["test"])
            results.append({"id": task["id"], "dim": task["dim"], "passed": passed, "detail": detail})
            print(f"  [{'PASS' if passed else 'FAIL'}] claude  {task['id']}"
                  + (f"  ({detail})" if not passed else ""))
        return results

    # zode track — parallelize the (network-bound) zode calls
    def one(task):
        z = run_zode(task)
        passed, detail = score(z["code"], task["test"])
        usage = parse_usage(z["stderr"])
        return {"id": task["id"], "dim": task["dim"], "passed": passed,
                "detail": detail, "secs": z["secs"], **usage}

    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as ex:
        futs = {ex.submit(one, t): t for t in TASKS}
        for fut in concurrent.futures.as_completed(futs):
            res = fut.result()
            results.append(res)
            print(f"  [{'PASS' if res['passed'] else 'FAIL'}] zode    {res['id']}  {res['secs']}s"
                  + (f"  ({res['detail']})" if not res["passed"] else ""))
    results.sort(key=lambda r: [t["id"] for t in TASKS].index(r["id"]))
    return results


def summarize(track: str, results: list[dict]) -> dict:
    dims = {}
    for r in results:
        d = dims.setdefault(r["dim"], [0, 0])
        d[1] += 1
        if r["passed"]:
            d[0] += 1
    total_pass = sum(1 for r in results if r["passed"])
    return {
        "track": track,
        "total": [total_pass, len(results)],
        "by_dim": dims,
        "tokens_in": sum(r.get("input_tokens", 0) for r in results),
        "tokens_out": sum(r.get("output_tokens", 0) for r in results),
        "cache_read": sum(r.get("cache_read", 0) for r in results),
        "results": results,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--validate", action="store_true")
    ap.add_argument("--track", choices=["zode", "claude", "both"], default="both")
    ap.add_argument("-j", "--jobs", type=int, default=4)
    args = ap.parse_args()

    print(f"{len(TASKS)} tasks across {len(by_dimension())} dimensions\n")

    if args.validate:
        res = run_track("claude", args.jobs)
        s = summarize("claude", res)
        p, n = s["total"]
        print(f"\nbaseline (claude): {p}/{n} pass")
        if p != n:
            print("WARNING: a correct baseline failed — a test is likely buggy.")
            sys.exit(1)
        return

    out = {}
    for track in (["claude", "zode"] if args.track == "both" else [args.track]):
        print(f"--- track: {track} ---")
        res = run_track(track, args.jobs)
        out[track] = summarize(track, res)
        print()

    (HERE / "results.json").write_text(json.dumps(out, indent=2))
    print("wrote benchmarks/results.json")
    for track, s in out.items():
        p, n = s["total"]
        print(f"{track}: {p}/{n} ({100*p//n}%)  tokens ↑{s['tokens_in']} ↓{s['tokens_out']} cache {s['cache_read']}")


if __name__ == "__main__":
    main()
