#!/usr/bin/env python3
"""Extra capability benchmarks for the Zode harness (LLM-driven).

Fills the gaps the other suites don't cover — these tasks stress the
HARNESS, not just the model:

  - toolchain : long tool chains (write -> read -> edit -> test -> fix),
                multi-file refactors, pipelines — the multi-round loop's
                stability and state keeping.
  - recovery  : the FIRST attempt fails (sandbox denial, wrong API, missing
                module); the agent must read the failure and change course
                instead of repeating it.
  - exploration: fuzzy prompts answered from the REAL repo (dynamic
                graders computed at run time — no stale fixtures).
  - zh-ops    : Chinese instructions and Chinese output (the common
                DeepSeek usage).

Scoring: hidden graders run in isolated subprocesses; tool traces are
parsed from zode's stderr like instructions.py.

Usage:
  python3 benchmarks/harness_extra.py [--track zode|claude] [-jN]
Env: ZODE_BIN (default target/debug/zode)
"""
import argparse
import concurrent.futures
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
ZODE_BIN = os.environ.get("ZODE_BIN", str(ROOT / "target" / "debug" / "zode"))
ZODE_TIMEOUT = 240
GRADE_TIMEOUT = 20
TOOL_RE = re.compile(r"^· (\S+)", re.MULTILINE)

WORK = tempfile.mkdtemp(prefix="zode-extra-")


# ---------------------------------------------------------------- tasks

def _seed(wd: Path, files: dict[str, str]) -> None:
    for name, body in files.items():
        p = wd / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(body)


TOOLCHAIN = [
    {
        "id": "tc_buggy_calc",
        "dim": "toolchain",
        "seed": {
            "calc.py": (
                "def calculate(op, a, b):\n"
                "    if op == 'add':\n"
                "        return a + b\n"
                "    if op == 'sub':\n"
                "        return a - b\n"
                "    if op == 'mul':\n"
                "        return a * b\n"
                "    if op == 'div':\n"
                "        return a // b\n"
                "    return None\n"
            ),
            "test_calc.py": (
                "import calc\n"
                "assert calc.calculate('add', 2, 3) == 5\n"
                "assert calc.calculate('div', 7, 2) == 3\n"
                "assert calc.calculate('mul', 0, 5) == 0\n"
                "assert calc.calculate('div', 5, 0) is None, 'div by zero must return None'\n"
            ),
        },
        "prompt": (
            "In this directory, calc.py has a bug and test_calc.py fails. "
            "Read both files, fix calc.py so ALL asserts in test_calc.py pass "
            "(run the test to verify — do not just guess), then leave the "
            "fixed calc.py in place."
        ),
        "check": lambda wd: _py_test(wd, "test_calc.py"),
    },
    {
        "id": "tc_pipeline",
        "dim": "toolchain",
        "seed": {
            "input.csv": "name,score\nada,10\nturing,20\nhopper,30\n",
            "pipeline.py": (
                "import csv, json\n"
                "\n"
                "def load(path):\n"
                "    with open(path) as f:\n"
                "        return list(csv.DictReader(f))\n"
                "\n"
                "def best(rows):\n"
                "    raise NotImplementedError('TODO')\n"
                "\n"
                "def save(rows, path):\n"
                "    with open(path, 'w') as f:\n"
                "        json.dump(rows, f)\n"
                "\n"
                "if __name__ == '__main__':\n"
                "    rows = load('input.csv')\n"
                "    save(best(rows), 'best.json')\n"
            ),
        },
        "prompt": (
            "Implement `best(rows)` in pipeline.py: it must return a list of "
            "the dicts whose score is the maximum (all ties). Then RUN "
            "pipeline.py, verify best.json exists and contains exactly the "
            "winning row(s), and report the winner's name. Do not modify "
            "input.csv."
        ),
        "check": lambda wd: _check(wd, [
            (["python3", "pipeline.py"], 0),
            (["python3", "-c",
              "import json; d=json.load(open('best.json'));"
              "assert [r['name'] for r in d] == ['hopper'], d; print('OK')"], 0),
        ]),
    },
    {
        "id": "tc_refactor_dedup",
        "dim": "toolchain",
        "seed": {
            "a.py": (
                "def fmt_a(items):\n"
                "    out = []\n"
                "    for i in items:\n"
                "        s = str(i).strip()\n"
                "        if s:\n"
                "            out.append(s.upper())\n"
                "    return out\n"
            ),
            "b.py": (
                "def fmt_b(items):\n"
                "    out = []\n"
                "    for i in items:\n"
                "        s = str(i).strip()\n"
                "        if s:\n"
                "            out.append(s.upper())\n"
                "    return out\n"
            ),
            "test_ab.py": (
                "import a, b\n"
                "assert a.fmt_a([' x ', '', 'y']) == ['X', 'Y']\n"
                "assert b.fmt_b([1, '', 2]) == ['1', '2']\n"
                "assert a.fmt_a([''] ) == []\n"
                "print('OK')\n"
            ),
        },
        "prompt": (
            "a.py and b.py contain IDENTICAL duplicated logic. Refactor: "
            "move the shared formatting into one helper in a new file "
            "shared.py, and make BOTH a.py and b.py import and use it, "
            "keeping their public function names. Run test_ab.py to verify "
            "the refactor kept behavior identical."
        ),
        "check": lambda wd: _check(wd, [
            (["python3", "test_ab.py"], 0),
            # the helper must actually be shared (a.py may not inline it)
            (["python3", "-c",
              "import a, b; assert 'shared' in open('a.py').read();"
              "assert 'shared' in open('b.py').read(); print('OK')"], 0),
        ]),
    },
]

RECOVERY = [
    {
        "id": "rc_outside_workspace",
        "dim": "recovery",
        "seed": {},
        "prompt": (
            "Write the text 'hello from zode' to /tmp/zode-extra-outside.txt. "
            "If the file system policy refuses that path, write the SAME text "
            "to a file named result.txt in the current directory instead, and "
            "confirm which file you actually wrote and why."
        ),
        # Any write path is fine (FileWrite into cwd, Bash into /tmp, …);
        # the grader verifies one of the two targets actually landed.
        "check": lambda wd: _check(wd, [
            (["python3", "-c",
              "import pathlib;"
              "r1 = pathlib.Path('result.txt');"
              "r2 = pathlib.Path('/tmp/zode-extra-outside.txt');"
              "ok1 = r1.exists() and 'hello from zode' in r1.read_text();"
              "ok2 = r2.exists() and 'hello from zode' in r2.read_text();"
              "assert ok1 or ok2, 'no target file written';"
              "print('OK')"], 0),
        ]),
    },
    {
        "id": "rc_bad_syntax",
        "dim": "recovery",
        "seed": {
            "broken.py": (
                "def total(items)\n"  # missing colon — guaranteed SyntaxError
                "    return sum(items)\n"
                "\n"
                "print(total([1, 2, 3]))\n"
            ),
        },
        "prompt": (
            "broken.py cannot run — running it fails immediately. Diagnose "
            "by RUNNING it, fix the syntax error, re-run, and confirm it "
            "prints the expected sum."
        ),
        "check": lambda wd: _check(wd, [
            (["python3", "-c",
              "import subprocess;"
              "r = subprocess.run(['python3','broken.py'],capture_output=True,text=True);"
              "assert r.returncode == 0, r.stderr;"
              "assert r.stdout.strip() == '6', r.stdout;"
              "print('OK')"], 0),
        ]),
        "require_tool": "Bash",
    },
    {
        "id": "rc_wrong_api",
        "dim": "recovery",
        "seed": {
            "greet.py": (
                "def greet(name):\n"
                "    # WRONG API on purpose: str.uppercase does not exist\n"
                "    return 'Hello, ' + name.uppercase() + '!'\n"
            ),
            "test_greet.py": (
                "import greet\n"
                "assert greet.greet('ada') == 'Hello, ADA!'\n"
                "print('OK')\n"
            ),
        },
        "prompt": (
            "test_greet.py fails because greet.py uses a non-existent "
            "string method. Diagnose by RUNNING the test, fix greet.py "
            "properly (the intent: uppercase the name), re-run until the "
            "test passes, and leave the fix in place."
        ),
        "check": lambda wd: _py_test(wd, "test_greet.py"),
        "require_tool": "Bash",
    },
    {
        "id": "rc_missing_module",
        "dim": "recovery",
        "seed": {
            "report.py": (
                "import totals  # does not exist yet\n"
                "\n"
                "def main():\n"
                "    print(totals.sum([1, 2, 3, 4]))\n"
                "\n"
                "main()\n"
            ),
        },
        "prompt": (
            "report.py fails to run because it imports a module `totals` "
            "that does not exist. Do NOT install anything. Read report.py, "
            "figure out what `totals.sum` is supposed to do from its usage, "
            "and create totals.py providing it. Run report.py and confirm it "
            "prints the expected number."
        ),
        "check": lambda wd: _check(wd, [
            (["python3", "-c",
              "import totals; assert totals.sum([1,2,3,4]) == 10, totals.sum([1,2,3,4]); print('OK')"], 0),
        ]),
    },
]

EXPLORATION = [
    {
        "id": "ex_lite_constant",
        "dim": "exploration",
        "prompt": (
            "In the zode repo: which file and line defines the constant "
            "LITE_VISIBLE_TOOLS, and how many tool names does it list? "
            "Verify with tools before answering. Answer with the file:line "
            "and the count."
        ),
        # dynamic: the repo is the grader
        "check": lambda wd: _check(wd, [
            (["python3", "-c",
              "import re, pathlib;"
              "t = pathlib.Path('crates/zode-core/src/engine.rs').read_text();"
              "m = re.search(r'LITE_VISIBLE_TOOLS: &\\[&str\\] = &\\[(.*?)\\];', t, re.S);"
              "assert m, 'constant not found';"
              "n = len(re.findall(r'\"', m.group(1))) // 2;"
              "assert n >= 5, n;"
              "print('COUNT=' + str(n)); print('OK')"], 0),
        ]),
        "expect_re": r"LITE_VISIBLE_TOOLS|engine\.rs",
        "require_tool": "Grep",
    },
    {
        "id": "ex_mcp_prefix",
        "dim": "exploration",
        "prompt": (
            "How does Zode name MCP tools (the full tool-name format), and "
            "in which source file is that naming helper defined? Find it in "
            "the repo and answer with the exact format and the file path."
        ),
        "expect_re": r"mcp__.*__|prefixed_tool_name",
        "require_tool": "Grep",
    },
    {
        "id": "ex_compact_knobs",
        "dim": "exploration",
        "prompt": (
            "Zode's auto-compaction has several tuning constants in "
            "vendor/agent/crates/agent/src/compact/auto.rs. Report the "
            "values of AUTOCOMPACT_BUFFER_TOKENS and MANUAL_COMPACT_BUFFER_TOKENS."
        ),
        "expect_re": r"13_000|3_000",
        "require_tool": "Grep",
    },
]

ZH_OPS = [
    {
        "id": "zh_script",
        "dim": "zh-ops",
        "seed": {},
        "prompt": (
            "写一个 Python 脚本 even_squares.py：计算 1 到 100 之间所有偶数的"
            "平方和，并把结果打印出来（只要数字）。写完后运行它验证输出。"
        ),
        "check": lambda wd: _check(wd, [
            (["python3", "-c",
              "import subprocess;"
              "r = subprocess.run(['python3','even_squares.py'],capture_output=True,text=True);"
              "assert r.returncode == 0, r.stderr;"
              "expected = sum(i*i for i in range(2,101,2));"
              "assert str(expected) in r.stdout, (r.stdout, expected);"
              "print('OK')"], 0),
        ]),
        # RunCheck (zode's test-runner tool) is an equally valid way to run
        # the script; either counts as executing it.
        "require_tool": "Bash|RunCheck",
    },
    {
        "id": "zh_read_summary",
        "dim": "zh-ops",
        "prompt": (
            "阅读 vendor/agent/crates/agent/src/compact/microcompact.rs 的模块级"
            "注释（文件开头的 //! 文档），用中文总结：microcompact 的策略是什么、"
            "为什么这样做。回答不超过 150 字，用中文。"
        ),
        "expect_re": r"[\u4e00-\u9fff]",
        "require_tool": "FileRead",
    },
]

ALL_TASKS = TOOLCHAIN + RECOVERY + EXPLORATION + ZH_OPS


# ---------------------------------------------------------------- grading

def _py_test(wd: Path, test: str) -> tuple[bool, str]:
    return _check(wd, [(["python3", test], 0)])


def _check(wd: Path, steps: list[tuple[list[str], int]]) -> tuple[bool, str]:
    # Exit-code based: self-verifying python -c steps assert internally and
    # fail with a non-zero exit; plain runs just need to complete cleanly.
    for argv, want in steps:
        try:
            r = subprocess.run(argv, capture_output=True, text=True,
                               timeout=GRADE_TIMEOUT, cwd=wd)
        except subprocess.TimeoutExpired:
            return False, "grader timeout"
        if r.returncode != want:
            err = (r.stderr or r.stdout).strip().splitlines()
            return False, (err[-1] if err else f"exit {r.returncode}")
    return True, ""


def run_zode_task(task: dict) -> dict:
    wd = Path(tempfile.mkdtemp(prefix="zode-extra-t-"))
    _seed(wd, task.get("seed", {}))
    # Exploration tasks need the real repo as context; recovery/toolchain/zh
    # run in the seeded temp dir.
    cwd = ROOT if task["dim"] == "exploration" else wd
    t0 = time.time()
    try:
        r = subprocess.run([ZODE_BIN, "-p", task["prompt"], "--yolo"],
                           capture_output=True, text=True, timeout=ZODE_TIMEOUT,
                           cwd=cwd)
    except subprocess.TimeoutExpired:
        return {"id": task["id"], "dim": task["dim"], "passed": False,
                "detail": "timeout", "tools": [], "secs": ZODE_TIMEOUT}
    secs = round(time.time() - t0, 1)
    out, err = r.stdout, r.stderr
    tools = sorted(set(TOOL_RE.findall(err)))
    # 1) hidden grader when present (run in the task's cwd: seeded temp dir
    # for toolchain/recovery/zh, the real repo for exploration)
    if "check" in task:
        ok, detail = task["check"](wd if task["dim"] != "exploration" else ROOT)
        if not ok:
            return {"id": task["id"], "dim": task["dim"], "passed": False,
                    "detail": f"grader: {detail}", "tools": tools, "secs": secs}
    # 2) answer-text expectations
    if "expect_re" in task and not re.search(task["expect_re"], out, re.I):
        return {"id": task["id"], "dim": task["dim"], "passed": False,
                "detail": f"answer missing {task['expect_re']!r}", "tools": tools,
                "secs": secs}
    # 3) required tool actually used (alternatives separated by `|`)
    if "require_tool" in task:
        needed = set(task["require_tool"].split("|"))
        if not needed.intersection(tools):
            return {"id": task["id"], "dim": task["dim"], "passed": False,
                    "detail": f"did not use {task['require_tool']} (tools: {tools or 'none'})",
                    "tools": tools, "secs": secs}
    return {"id": task["id"], "dim": task["dim"], "passed": True,
            "detail": "", "tools": tools, "secs": secs}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("-j", "--jobs", type=int, default=3)
    ap.add_argument("--track", default="zode")
    args = ap.parse_args()
    print(f"{len(ALL_TASKS)} extra harness tasks across "
          f"{sorted({t['dim'] for t in ALL_TASKS})}\n")
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as ex:
        futures = [ex.submit(run_zode_task, t) for t in ALL_TASKS]
        for f in concurrent.futures.as_completed(futures):
            res = f.result()
            results.append(res)
            print(f"  [{'PASS' if res['passed'] else 'FAIL'}] {res['dim']:<12} "
                  f"{res['id']:<22} {res.get('secs', 0):>5}s"
                  + (f"  tools: {','.join(res['tools'])}" if res['tools'] else "")
                  + (f"  ({res['detail'][:110]})" if not res['passed'] else ""))
    results.sort(key=lambda r: r["id"])
    by_dim: dict[str, list[bool]] = {}
    for r in results:
        by_dim.setdefault(r["dim"], []).append(r["passed"])
    print("\nby dim:", {d: [sum(v), len(v)] for d, v in by_dim.items()})
    passed = sum(1 for r in results if r["passed"])
    print(f"Zode: {passed}/{len(results)} ({passed * 100 // len(results)}%)")
    out = ROOT / "benchmarks" / "harness_extra_results.json"
    out.write_text(json.dumps({"total": [passed, len(results)],
                               "by_dim": {d: [sum(v), len(v)] for d, v in by_dim.items()},
                               "results": results}, indent=1))


if __name__ == "__main__":
    main()
