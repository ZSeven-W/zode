#!/usr/bin/env python3
"""Tricky-bug (疑难杂症) agentic benchmark.

The hardest kind of harness test: each file hides a *subtle* bug that reads as
correct but fails at runtime — the classic Python footguns (mutable default
arg, closure late-binding, dict-mutation-during-iteration, shallow-copy
aliasing, generator exhaustion, `is` vs `==`, shared class attributes, float
truncation). The agent can't fix these by skimming; it must run the test,
reproduce the failure, diagnose the real cause, and fix it.

Head-to-head, same hidden grader for both tracks (reuses agentic.py's runner):
  zode   : `zode -p "<task>" --yolo` debugs agentically
  claude : Claude's direct fix

Usage:  python3 benchmarks/hardbugs.py [--track zode|claude|both] [-jN]
"""
import argparse
import concurrent.futures
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from agentic import run_zode_task, run_claude_task  # noqa: E402  (reuse runner)

TASKS = [
    {
        "id": "mutable_default_arg",
        "dim": "subtle-state",
        "files": {
            "accum.py": "def collect(item, bucket=[]):\n    bucket.append(item)\n    return bucket\n",
            "test_accum.py": "from accum import collect\n\ndef test_fresh():\n    assert collect(1) == [1]\n    assert collect(2) == [2]\n",
        },
        "prompt": "collect() has a subtle bug: two separate calls with no bucket argument share the same list. Run test_accum.py to see it, diagnose the cause, and fix collect() so each call without a bucket starts empty (while still appending to a bucket that IS passed).",
        "grader": (
            "from accum import collect\n"
            "assert collect(1) == [1]\n"
            "assert collect(2) == [2]\n"
            "b = []\n"
            "assert collect(3, b) == [3]\n"
            "assert collect(4, b) == [3, 4]\n"
            "assert collect(5) == [5]\n"
        ),
        "claude": {"accum.py": "def collect(item, bucket=None):\n    if bucket is None:\n        bucket = []\n    bucket.append(item)\n    return bucket\n"},
    },
    {
        "id": "closure_late_binding",
        "dim": "subtle-scope",
        "files": {
            "factory.py": "def make_adders():\n    return [lambda x: x + i for i in range(4)]\n",
            "test_factory.py": "from factory import make_adders\n\ndef test_adders():\n    fs = make_adders()\n    assert fs[0](10) == 10\n",
        },
        "prompt": "make_adders() should return four functions that add 0, 1, 2, 3 respectively, but they all add 3. Run the test, find why, and fix make_adders().",
        "grader": (
            "from factory import make_adders\n"
            "fs = make_adders()\n"
            "assert [f(10) for f in fs] == [10, 11, 12, 13]\n"
            "assert [f(0) for f in fs] == [0, 1, 2, 3]\n"
        ),
        "claude": {"factory.py": "def make_adders():\n    return [lambda x, i=i: x + i for i in range(4)]\n"},
    },
    {
        "id": "dict_mutation_iter",
        "dim": "runtime-error",
        "files": {
            "filt.py": "def drop_negatives(d):\n    for k in d:\n        if d[k] < 0:\n            del d[k]\n    return d\n",
            "test_filt.py": "from filt import drop_negatives\n\ndef test_drop():\n    assert drop_negatives({'a': 1}) == {'a': 1}\n",
        },
        "prompt": "drop_negatives should remove every key whose value is negative, but it raises RuntimeError on some inputs. Run the test, diagnose, and fix it.",
        "grader": (
            "from filt import drop_negatives\n"
            "assert drop_negatives({'a': 1, 'b': -2, 'c': -3, 'd': 4}) == {'a': 1, 'd': 4}\n"
            "assert drop_negatives({'x': -1, 'y': -2}) == {}\n"
            "assert drop_negatives({}) == {}\n"
        ),
        "claude": {"filt.py": "def drop_negatives(d):\n    for k in list(d):\n        if d[k] < 0:\n            del d[k]\n    return d\n"},
    },
    {
        "id": "shallow_grid_aliasing",
        "dim": "subtle-aliasing",
        "files": {
            "grid.py": "def make_grid(rows, cols):\n    return [[0] * cols] * rows\n",
            "test_grid.py": "from grid import make_grid\n\ndef test_shape():\n    g = make_grid(2, 3)\n    assert len(g) == 2 and len(g[0]) == 3\n",
        },
        "prompt": "make_grid builds a rows×cols grid of zeros, but setting one cell mysteriously changes a whole column — the rows are aliased to the same list. Fix make_grid so each row is independent.",
        "grader": (
            "from grid import make_grid\n"
            "g = make_grid(3, 3)\n"
            "g[0][0] = 1\n"
            "assert g[1][0] == 0 and g[2][0] == 0\n"
            "assert g[0][0] == 1\n"
            "assert make_grid(2, 2) == [[0, 0], [0, 0]]\n"
        ),
        "claude": {"grid.py": "def make_grid(rows, cols):\n    return [[0] * cols for _ in range(rows)]\n"},
    },
    {
        "id": "generator_consumed_twice",
        "dim": "subtle-iterator",
        "files": {
            "gstats.py": "def min_max(gen):\n    return (min(gen), max(gen))\n",
            "test_gstats.py": "from gstats import min_max\n\ndef test_list():\n    assert min_max([3, 1, 2]) == (1, 3)\n",
        },
        "prompt": "min_max works on a list but breaks on a one-shot generator: the second aggregation sees an already-exhausted iterator. Fix min_max so it works for any iterable, including generators.",
        "grader": (
            "from gstats import min_max\n"
            "assert min_max([3, 1, 2]) == (1, 3)\n"
            "assert min_max(iter([3, 1, 2])) == (1, 3)\n"
            "assert min_max(x for x in [5]) == (5, 5)\n"
            "assert min_max(x * x for x in range(1, 4)) == (1, 9)\n"
        ),
        "claude": {"gstats.py": "def min_max(gen):\n    items = list(gen)\n    return (min(items), max(items))\n"},
    },
    {
        "id": "is_vs_equals",
        "dim": "subtle-identity",
        "files": {
            "tally.py": "def count_equal(items, target):\n    return sum(1 for x in items if x is target)\n",
            "test_tally.py": "from tally import count_equal\n\ndef test_small():\n    assert count_equal([1, 2, 1], 1) == 2\n",
        },
        "prompt": "count_equal should count items equal to target, and the small-int test passes — but it fails for large integers and for most strings because it compares identity instead of equality. Fix it.",
        "grader": (
            "from tally import count_equal\n"
            "assert count_equal([1, 2, 1], 1) == 2\n"
            "assert count_equal([1000, 1000, 2], 1000) == 2\n"
            "assert count_equal(['ab', 'a' + 'b', 'c'], 'ab') == 2\n"
            "assert count_equal([], 1) == 0\n"
        ),
        "claude": {"tally.py": "def count_equal(items, target):\n    return sum(1 for x in items if x == target)\n"},
    },
    {
        "id": "shared_class_attr",
        "dim": "subtle-state",
        "files": {
            "stackmod.py": "class Stack:\n    items = []\n\n    def push(self, x):\n        self.items.append(x)\n\n    def pop(self):\n        return self.items.pop()\n",
            "test_stackmod.py": "from stackmod import Stack\n\ndef test_one():\n    s = Stack()\n    s.push(1)\n    assert s.pop() == 1\n",
        },
        "prompt": "Two separate Stack() instances unexpectedly share the same contents (pushing to one shows up in the other). Fix the class so each instance owns its own items list.",
        "grader": (
            "from stackmod import Stack\n"
            "a = Stack(); b = Stack()\n"
            "a.push(1); a.push(2)\n"
            "assert b.items == []\n"
            "assert a.pop() == 2\n"
            "b.push(9)\n"
            "assert a.items == [1] and b.items == [9]\n"
        ),
        "claude": {"stackmod.py": "class Stack:\n    def __init__(self):\n        self.items = []\n\n    def push(self, x):\n        self.items.append(x)\n\n    def pop(self):\n        return self.items.pop()\n"},
    },
    {
        "id": "float_truncation",
        "dim": "subtle-float",
        "files": {
            "money.py": "def cents(dollars):\n    return int(dollars * 100)\n",
            "test_money.py": "from money import cents\n\ndef test_round_values():\n    assert cents(2.00) == 200\n",
        },
        "prompt": "cents(dollars) converts a dollar amount to integer cents, but it's off by one for many values — e.g. cents(1.15) gives 114, not 115 — because floating-point multiplication lands just under the integer and int() truncates. Fix it to round correctly.",
        "grader": (
            "from money import cents\n"
            "assert cents(1.15) == 115\n"
            "assert cents(0.07) == 7\n"
            "assert cents(2.00) == 200\n"
            "assert cents(0.10) == 10\n"
            "assert cents(4.20) == 420\n"
        ),
        "claude": {"money.py": "def cents(dollars):\n    return round(dollars * 100)\n"},
    },
    {
        "id": "off_by_one_window",
        "dim": "off-by-one",
        "files": {
            "window.py": "def max_pair_sum(a):\n    best = a[0] + a[1]\n    for i in range(len(a)):\n        best = max(best, a[i] + a[i + 1])\n    return best\n",
            "test_window.py": "from window import max_pair_sum\n\ndef test_basic():\n    assert max_pair_sum([1, 2, 3, 4]) == 7\n",
        },
        "prompt": "max_pair_sum should return the largest sum of two adjacent elements, but it raises IndexError. Run the test, find the off-by-one, and fix it.",
        "grader": (
            "from window import max_pair_sum\n"
            "assert max_pair_sum([1, 2, 3, 4]) == 7\n"
            "assert max_pair_sum([5, 1]) == 6\n"
            "assert max_pair_sum([-1, -2, -3]) == -3\n"
            "assert max_pair_sum([10, -5, 10, -5, 10]) == 5\n"
        ),
        "claude": {"window.py": "def max_pair_sum(a):\n    best = a[0] + a[1]\n    for i in range(len(a) - 1):\n        best = max(best, a[i] + a[i + 1])\n    return best\n"},
    },
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--track", choices=["zode", "claude", "both"], default="both")
    ap.add_argument("-j", "--jobs", type=int, default=4)
    args = ap.parse_args()
    print(f"{len(TASKS)} tricky-bug tasks (diagnose subtle bugs via the harness)\n")

    out = {}
    if args.track in ("claude", "both"):
        cres = [run_claude_task(t) for t in TASKS]
        for r in cres:
            print(f"  [{'PASS' if r['passed'] else 'FAIL'}] claude  {r['id']}"
                  + (f"  ({r['detail']})" if not r["passed"] else ""))
        out["claude"] = {"total": [sum(r["passed"] for r in cres), len(cres)], "results": cres}
        print()
    if args.track in ("zode", "both"):
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as ex:
            zres = list(ex.map(run_zode_task, TASKS))
        for r in zres:
            print(f"  [{'PASS' if r['passed'] else 'FAIL'}] zode    {r['id']:<22} {r['secs']:>5}s"
                  f"  tools: {','.join(r['tools']) or '(none)'}"
                  + (f"  ({r['detail']})" if not r["passed"] else ""))
        tools = sorted({t for r in zres for t in r["tools"]})
        out["zode"] = {"total": [sum(r["passed"] for r in zres), len(zres)],
                       "tools_seen": tools, "results": zres}

    (Path(__file__).resolve().parent / "hardbugs_results.json").write_text(json.dumps(out, indent=2))
    print()
    for track, s in out.items():
        p, n = s["total"]
        print(f"{track}: {p}/{n} ({100 * p // n}%)")


if __name__ == "__main__":
    main()
