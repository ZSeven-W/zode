#!/usr/bin/env python3
"""Harder, head-to-head agentic harness benchmark for Zode.

These tasks stress Zode's *harness*, not just the model: each seeds a scratch
repo with buggy/stub files (often plus a visible test the agent can run), hands
over a natural-language instruction, and the agent must read, edit, run, and
fix real files to succeed. Tasks are deliberately harder than one-shot
completion — subtle bugs that only show under testing, multi-method classes,
algorithms with error cases, and a refactor-and-extend.

Head-to-head: two tracks scored by the SAME hidden grader (never shown to the
model, often stronger than the visible test, so passing the visible test isn't
enough):
  - zode   : `zode -p "<task>" --yolo` edits the files agentically
  - claude : Claude's direct fix (final file contents authored by Claude)

Usage:  python3 benchmarks/agentic.py [--track zode|claude|both] [-jN]
Env:    ZODE_BIN, ZODE_CONFIG_DIR
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

ROOT = Path(__file__).resolve().parent.parent
_zbin = os.environ.get("ZODE_BIN", str(ROOT / "target" / "release" / "zode"))
ZODE_BIN = str((ROOT / _zbin).resolve() if not os.path.isabs(_zbin) else Path(_zbin))
ZODE_TIMEOUT = 300
GRADE_TIMEOUT = 20

TASKS = [
    {
        "id": "fix_binary_search",
        "dim": "debug",
        "files": {
            "search.py": (
                "def bsearch(a, x):\n"
                "    lo, hi = 0, len(a)\n"
                "    while lo < hi:\n"
                "        mid = (lo + hi) // 2\n"
                "        if a[mid] == x:\n"
                "            return mid\n"
                "        elif a[mid] < x:\n"
                "            lo = mid        # bug: should be mid + 1\n"
                "        else:\n"
                "            hi = mid\n"
                "    return -1\n"
            ),
            "test_search.py": (
                "from search import bsearch\n\n"
                "def test_basic():\n"
                "    assert bsearch([1, 3, 5, 7, 9], 7) == 3\n"
                "    assert bsearch([1, 3, 5, 7, 9], 4) == -1\n"
            ),
        },
        "prompt": "search.py has a binary-search bug — on some inputs bsearch loops forever or returns the wrong answer. Run test_search.py, find the bug, and fix bsearch in search.py so it returns the index of x (or -1 if absent). Do not modify the test.",
        "grader": (
            "from search import bsearch\n"
            "assert bsearch([], 5) == -1\n"
            "assert bsearch([1], 1) == 0\n"
            "assert bsearch([1], 2) == -1\n"
            "assert bsearch([1, 3, 5, 7, 9], 7) == 3\n"
            "assert bsearch([1, 3, 5, 7, 9], 4) == -1\n"
            "import random\n"
            "a = list(range(0, 2000, 2))\n"
            "for x in [0, 1998, 999, 1000, -1, 1999]:\n"
            "    i = bsearch(a, x)\n"
            "    assert (i == -1 and x not in a) or a[i] == x\n"
        ),
        "claude": {
            "search.py": (
                "def bsearch(a, x):\n"
                "    lo, hi = 0, len(a)\n"
                "    while lo < hi:\n"
                "        mid = (lo + hi) // 2\n"
                "        if a[mid] == x:\n"
                "            return mid\n"
                "        elif a[mid] < x:\n"
                "            lo = mid + 1\n"
                "        else:\n"
                "            hi = mid\n"
                "    return -1\n"
            ),
        },
    },
    {
        "id": "implement_lru",
        "dim": "implement-class",
        "files": {
            "cache.py": (
                "class LRUCache:\n"
                "    def __init__(self, capacity):\n"
                "        self.capacity = capacity\n"
                "    # TODO: implement get(key) and put(key, value) with LRU eviction\n"
            ),
            "test_cache.py": (
                "from cache import LRUCache\n\n"
                "def test_basic():\n"
                "    c = LRUCache(2)\n"
                "    c.put(1, 1); c.put(2, 2)\n"
                "    assert c.get(1) == 1\n"
                "    c.put(3, 3)\n"
                "    assert c.get(2) == -1\n"
            ),
        },
        "prompt": "Implement LRUCache in cache.py: get(key) returns the value or -1 if absent; put(key, value) inserts/updates; when over capacity it evicts the least-recently-used entry. Both get and put count as a use. Make test_cache.py pass.",
        "grader": (
            "from cache import LRUCache\n"
            "c = LRUCache(2)\n"
            "c.put(1, 1); c.put(2, 2)\n"
            "assert c.get(1) == 1\n"
            "c.put(3, 3)\n"            # evicts 2
            "assert c.get(2) == -1\n"
            "c.put(4, 4)\n"            # evicts 1 (3 was used last via put; get(1) made 1 MRU then put(3) evicted 2, get... )
            "assert c.get(1) == -1\n"
            "assert c.get(3) == 3\n"
            "assert c.get(4) == 4\n"
            "c2 = LRUCache(1)\n"
            "c2.put(5, 5); c2.put(6, 6)\n"
            "assert c2.get(5) == -1 and c2.get(6) == 6\n"
        ),
        "claude": {
            "cache.py": (
                "from collections import OrderedDict\n\n"
                "class LRUCache:\n"
                "    def __init__(self, capacity):\n"
                "        self.capacity = capacity\n"
                "        self.d = OrderedDict()\n\n"
                "    def get(self, key):\n"
                "        if key not in self.d:\n"
                "            return -1\n"
                "        self.d.move_to_end(key)\n"
                "        return self.d[key]\n\n"
                "    def put(self, key, value):\n"
                "        if key in self.d:\n"
                "            self.d.move_to_end(key)\n"
                "        self.d[key] = value\n"
                "        if len(self.d) > self.capacity:\n"
                "            self.d.popitem(last=False)\n"
            ),
        },
    },
    {
        "id": "mini_eval",
        "dim": "implement-algo",
        "files": {
            "calc.py": (
                "def evaluate(expr):\n"
                "    \"\"\"Evaluate an integer arithmetic expression.\"\"\"\n"
                "    # TODO: implement\n"
                "    raise NotImplementedError\n"
            ),
            "test_calc.py": (
                "from calc import evaluate\n\n"
                "def test_basic():\n"
                "    assert evaluate('1 + 2 * 3') == 7\n"
                "    assert evaluate('(1 + 2) * 3') == 9\n"
            ),
        },
        "prompt": "Implement evaluate(expr) in calc.py: evaluate an arithmetic expression of integers with + - * / and parentheses, honoring precedence, with '/' as integer division truncating toward zero. Ignore whitespace. Do NOT use Python's eval(). Make test_calc.py pass.",
        "grader": (
            "from calc import evaluate\n"
            "assert evaluate('1 + 2 * 3') == 7\n"
            "assert evaluate('(1 + 2) * 3') == 9\n"
            "assert evaluate('10 / 3') == 3\n"
            "assert evaluate('7 - 2 - 3') == 2\n"
            "assert evaluate('2 * (3 + (4 - 1))') == 12\n"
            "assert evaluate('(8 / (2 + 2))') == 2\n"
            "assert evaluate('100') == 100\n"
            "import calc, inspect\n"
            "assert 'eval(' not in inspect.getsource(calc.evaluate)\n"
        ),
        "claude": {
            "calc.py": (
                "def evaluate(expr):\n"
                "    s = expr.replace(' ', '')\n"
                "    pos = 0\n"
                "    def peek():\n"
                "        return s[pos] if pos < len(s) else ''\n"
                "    def parse_expr():\n"
                "        nonlocal pos\n"
                "        val = parse_term()\n"
                "        while peek() in ('+', '-'):\n"
                "            op = s[pos]; pos += 1\n"
                "            r = parse_term()\n"
                "            val = val + r if op == '+' else val - r\n"
                "        return val\n"
                "    def parse_term():\n"
                "        nonlocal pos\n"
                "        val = parse_factor()\n"
                "        while peek() in ('*', '/'):\n"
                "            op = s[pos]; pos += 1\n"
                "            r = parse_factor()\n"
                "            val = val * r if op == '*' else int(val / r)\n"
                "        return val\n"
                "    def parse_factor():\n"
                "        nonlocal pos\n"
                "        if peek() == '(':\n"
                "            pos += 1\n"
                "            val = parse_expr()\n"
                "            pos += 1\n"
                "            return val\n"
                "        start = pos\n"
                "        if peek() in ('+', '-'):\n"
                "            pos += 1\n"
                "        while pos < len(s) and s[pos].isdigit():\n"
                "            pos += 1\n"
                "        return int(s[start:pos])\n"
                "    return parse_expr()\n"
            ),
        },
    },
    {
        "id": "fix_three_bugs",
        "dim": "multi-bug",
        "files": {
            "textproc.py": (
                "def word_count(text):\n"
                "    return len(text.split(' '))          # bug: miscounts on multiple/leading spaces\n\n"
                "def initials(name):\n"
                "    return ''.join(p[0] for p in name.split())   # bug: not uppercased\n\n"
                "def truncate(s, n):\n"
                "    return s[:n] + '...'                  # bug: always appends, even when short\n"
            ),
            "test_textproc.py": (
                "from textproc import word_count, initials, truncate\n\n"
                "def test_all():\n"
                "    assert word_count('a b c') == 3\n"
                "    assert initials('ada lovelace') == 'AL'\n"
                "    assert truncate('hello', 10) == 'hello'\n"
            ),
        },
        "prompt": "textproc.py has three separate bugs and test_textproc.py fails. Fix all three functions so the tests pass: word_count must count words robustly (multiple/leading/trailing spaces), initials must be uppercase, and truncate must only append '...' when it actually shortens the string.",
        "grader": (
            "from textproc import word_count, initials, truncate\n"
            "assert word_count('a b c') == 3\n"
            "assert word_count('  a   b  ') == 2\n"
            "assert word_count('') == 0\n"
            "assert initials('ada lovelace') == 'AL'\n"
            "assert initials('grace') == 'G'\n"
            "assert truncate('hello', 10) == 'hello'\n"
            "assert truncate('hello world', 5) == 'hello...'\n"
            "assert truncate('hello', 5) == 'hello'\n"
        ),
        "claude": {
            "textproc.py": (
                "def word_count(text):\n"
                "    return len(text.split())\n\n"
                "def initials(name):\n"
                "    return ''.join(p[0] for p in name.split()).upper()\n\n"
                "def truncate(s, n):\n"
                "    return s if len(s) <= n else s[:n] + '...'\n"
            ),
        },
    },
    {
        "id": "topo_sort",
        "dim": "implement-algo",
        "files": {
            "graph.py": (
                "def topo_sort(graph):\n"
                "    \"\"\"graph: {node: [dependencies]}. Return an order with each\n"
                "    dependency before the nodes that depend on it. Raise ValueError\n"
                "    on a cycle.\"\"\"\n"
                "    # TODO: implement\n"
                "    raise NotImplementedError\n"
            ),
            "test_graph.py": (
                "from graph import topo_sort\n\n"
                "def test_order():\n"
                "    order = topo_sort({'a': [], 'b': ['a'], 'c': ['b']})\n"
                "    assert order.index('a') < order.index('b') < order.index('c')\n"
            ),
        },
        "prompt": "Implement topo_sort(graph) in graph.py where graph maps each node to a list of its dependencies. Return a list of all nodes where every dependency comes before the nodes that depend on it. Raise ValueError if the graph has a cycle. Make test_graph.py pass.",
        "grader": (
            "from graph import topo_sort\n"
            "o = topo_sort({'a': [], 'b': ['a'], 'c': ['b', 'a'], 'd': ['c']})\n"
            "for node, deps in {'a': [], 'b': ['a'], 'c': ['b', 'a'], 'd': ['c']}.items():\n"
            "    for dep in deps:\n"
            "        assert o.index(dep) < o.index(node)\n"
            "assert set(o) == {'a', 'b', 'c', 'd'}\n"
            "raised = False\n"
            "try:\n"
            "    topo_sort({'x': ['y'], 'y': ['x']})\n"
            "except ValueError:\n"
            "    raised = True\n"
            "assert raised, 'cycle must raise ValueError'\n"
        ),
        "claude": {
            "graph.py": (
                "def topo_sort(graph):\n"
                "    from collections import deque\n"
                "    indeg = {n: 0 for n in graph}\n"
                "    adj = {n: [] for n in graph}\n"
                "    for node, deps in graph.items():\n"
                "        for dep in deps:\n"
                "            adj[dep].append(node)\n"
                "            indeg[node] += 1\n"
                "    q = deque(sorted(n for n in graph if indeg[n] == 0))\n"
                "    order = []\n"
                "    while q:\n"
                "        n = q.popleft()\n"
                "        order.append(n)\n"
                "        for m in adj[n]:\n"
                "            indeg[m] -= 1\n"
                "            if indeg[m] == 0:\n"
                "                q.append(m)\n"
                "    if len(order) != len(graph):\n"
                "        raise ValueError('cycle detected')\n"
                "    return order\n"
            ),
        },
    },
    {
        "id": "refactor_extend",
        "dim": "refactor",
        "files": {
            "counter.py": (
                "class Counter:\n"
                "    def __init__(self):\n"
                "        self.pairs = []          # list of [item, count]\n\n"
                "    def add(self, item):\n"
                "        for p in self.pairs:\n"
                "            if p[0] == item:\n"
                "                p[1] += 1\n"
                "                return\n"
                "        self.pairs.append([item, 1])\n\n"
                "    def count(self, item):\n"
                "        for p in self.pairs:\n"
                "            if p[0] == item:\n"
                "                return p[1]\n"
                "        return 0\n"
            ),
            "test_counter.py": (
                "from counter import Counter\n\n"
                "def test_basic():\n"
                "    c = Counter()\n"
                "    for w in 'a b a c a b'.split():\n"
                "        c.add(w)\n"
                "    assert c.count('a') == 3 and c.count('z') == 0\n"
            ),
        },
        "prompt": "counter.py uses a list of pairs, so add() is O(n). Refactor Counter to store counts in a dict internally (keep the public API add(item) and count(item) identical), and ADD a method most_common(k) that returns the k most frequent items as (item, count) tuples, most frequent first, ties broken by first-insertion order. Keep test_counter.py passing.",
        "grader": (
            "from counter import Counter\n"
            "c = Counter()\n"
            "for w in 'a b a c a b'.split():\n"
            "    c.add(w)\n"
            "assert c.count('a') == 3 and c.count('b') == 2 and c.count('z') == 0\n"
            "assert c.most_common(2) == [('a', 3), ('b', 2)]\n"
            "assert c.most_common(1) == [('a', 3)]\n"
            "import counter, inspect\n"
            "assert 'dict' in inspect.getsource(counter.Counter).lower() or '{}' in inspect.getsource(counter.Counter)\n"
        ),
        "claude": {
            "counter.py": (
                "class Counter:\n"
                "    def __init__(self):\n"
                "        self.counts = {}\n"
                "        self.order = []\n\n"
                "    def add(self, item):\n"
                "        if item not in self.counts:\n"
                "            self.counts[item] = 0\n"
                "            self.order.append(item)\n"
                "        self.counts[item] += 1\n\n"
                "    def count(self, item):\n"
                "        return self.counts.get(item, 0)\n\n"
                "    def most_common(self, k):\n"
                "        ranked = sorted(self.order, key=lambda it: -self.counts[it])\n"
                "        return [(it, self.counts[it]) for it in ranked[:k]]\n"
            ),
        },
    },
]

TOOL_RE = re.compile(r"^· (\w+)", re.MULTILINE)


def grade(work: Path, grader: str) -> tuple[bool, str]:
    g = subprocess.run([sys.executable, "-c", grader], capture_output=True,
                       text=True, timeout=GRADE_TIMEOUT, cwd=work)
    if g.returncode == 0:
        return True, ""
    return False, (g.stderr.strip().splitlines() or ["fail"])[-1]


def run_zode_task(task: dict) -> dict:
    t0 = time.time()
    with tempfile.TemporaryDirectory(prefix="zode-agentic-") as work:
        wp = Path(work)
        for name, content in task["files"].items():
            (wp / name).write_text(content)
        try:
            r = subprocess.run([ZODE_BIN, "-p", task["prompt"], "--yolo"],
                               capture_output=True, text=True, timeout=ZODE_TIMEOUT, cwd=work)
        except subprocess.TimeoutExpired:
            return {"id": task["id"], "dim": task["dim"], "passed": False,
                    "detail": "zode timeout", "tools": [], "secs": ZODE_TIMEOUT}
        except FileNotFoundError:
            sys.exit(f"zode binary not found at {ZODE_BIN}")
        tools = sorted(set(TOOL_RE.findall(r.stderr)))
        passed, detail = grade(wp, task["grader"])
    return {"id": task["id"], "dim": task["dim"], "passed": passed, "detail": detail,
            "tools": tools, "secs": round(time.time() - t0, 1)}


def run_claude_task(task: dict) -> dict:
    # Claude's direct fix: write the authored final file state, then grade.
    with tempfile.TemporaryDirectory(prefix="claude-agentic-") as work:
        wp = Path(work)
        for name, content in task["files"].items():
            (wp / name).write_text(content)
        for name, content in task["claude"].items():
            (wp / name).write_text(content)
        passed, detail = grade(wp, task["grader"])
    return {"id": task["id"], "dim": task["dim"], "passed": passed, "detail": detail}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--track", choices=["zode", "claude", "both"], default="both")
    ap.add_argument("-j", "--jobs", type=int, default=4)
    args = ap.parse_args()
    print(f"{len(TASKS)} harder agentic tasks (read/edit/run/fix via the harness)\n")

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
            print(f"  [{'PASS' if r['passed'] else 'FAIL'}] zode    {r['id']:<18} {r['secs']:>5}s"
                  f"  tools: {','.join(r['tools']) or '(none)'}"
                  + (f"  ({r['detail']})" if not r["passed"] else ""))
        tools = sorted({t for r in zres for t in r["tools"]})
        out["zode"] = {"total": [sum(r["passed"] for r in zres), len(zres)],
                       "tools_seen": tools, "results": zres}

    (Path(__file__).resolve().parent / "agentic_results.json").write_text(json.dumps(out, indent=2))
    print()
    for track, s in out.items():
        p, n = s["total"]
        extra = f"  tools: {', '.join(s.get('tools_seen', []))}" if "tools_seen" in s else ""
        print(f"{track}: {p}/{n} ({100 * p // n}%){extra}")


if __name__ == "__main__":
    main()
