#!/usr/bin/env python3
"""Complex, multi-file agentic benchmark — the hardest harness tier.

Each task is a small project (often several files) requiring substantial
implementation or cross-module debugging: a template engine, a chainable query
engine, a multi-file expression language with variables + functions, a build-
order resolver with cycle detection, and a cross-file precedence bug the agent
must trace from the REPL into the engine. The agent must navigate (Grep/Glob/
ListDir), read multiple files, implement/edit, and run tests to verify.

Hidden graders (much stronger than any visible test). Head-to-head:
  zode   : `zode -p "<task>" --yolo` works the project agentically
  claude : Claude's direct implementation

Usage:  python3 benchmarks/complex.py [--track zode|claude|both] [-jN]
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
        "id": "template_engine",
        "dim": "implement-system",
        "files": {
            "template.py": (
                "def render(template, context):\n"
                "    \"\"\"Render a template string. Supports:\n"
                "      {{ var }}            substitution, incl. dotted access {{ a.b }}\n"
                "      {% for x in items %}...{% endfor %}   loop (x available inside)\n"
                "      {% if cond %}...{% endif %}           include body if context value is truthy\n"
                "    A missing variable renders as the empty string.\"\"\"\n"
                "    # TODO: implement\n"
                "    raise NotImplementedError\n"
            ),
            "test_template.py": (
                "from template import render\n\n"
                "def test_basic():\n"
                "    assert render('Hi {{ name }}', {'name': 'Sam'}) == 'Hi Sam'\n"
            ),
        },
        "prompt": "Implement render(template, context) in template.py exactly per its docstring: {{ var }} substitution including dotted access ({{ user.name }}), {% for item in items %}...{% endfor %} loops with the loop variable bound inside, {% if cond %}...{% endif %} conditionals on a truthy context value, and missing variables render as ''. Run test_template.py.",
        "grader": (
            "from template import render\n"
            "assert render('Hi {{ name }}', {'name': 'Sam'}) == 'Hi Sam'\n"
            "assert render('{{ user.name }}', {'user': {'name': 'Ada'}}) == 'Ada'\n"
            "assert render('{{ missing }}', {}) == ''\n"
            "assert render('{% if ok %}yes{% endif %}', {'ok': True}) == 'yes'\n"
            "assert render('{% if ok %}yes{% endif %}', {'ok': False}) == ''\n"
            "assert render('{% for x in xs %}[{{ x }}]{% endfor %}', {'xs': [1, 2, 3]}) == '[1][2][3]'\n"
            "out = render('{% for u in users %}{{ u.name }};{% endfor %}', {'users': [{'name': 'a'}, {'name': 'b'}]})\n"
            "assert out == 'a;b;', out\n"
        ),
        "claude": {
            "template.py": (
                "import re\n\n"
                "def _lookup(name, ctx):\n"
                "    cur = ctx\n"
                "    for part in name.split('.'):\n"
                "        if isinstance(cur, dict) and part in cur:\n"
                "            cur = cur[part]\n"
                "        else:\n"
                "            return None\n"
                "    return cur\n\n"
                "def _subst(text, ctx):\n"
                "    def repl(m):\n"
                "        v = _lookup(m.group(1).strip(), ctx)\n"
                "        return '' if v is None else str(v)\n"
                "    return re.sub(r'\\{\\{\\s*(.*?)\\s*\\}\\}', repl, text)\n\n"
                "_BLOCK = re.compile(\n"
                "    r'\\{%\\s*(?:for\\s+(\\w+)\\s+in\\s+(\\w+)|if\\s+(\\w+))\\s*%\\}(.*?)\\{%\\s*end(?:for|if)\\s*%\\}',\n"
                "    re.DOTALL,\n"
                ")\n\n"
                "def render(template, context):\n"
                "    def repl(m):\n"
                "        forvar, foriter, ifcond, body = m.group(1), m.group(2), m.group(3), m.group(4)\n"
                "        if forvar:\n"
                "            items = _lookup(foriter, context) or []\n"
                "            parts = []\n"
                "            for it in items:\n"
                "                local = dict(context)\n"
                "                local[forvar] = it\n"
                "                parts.append(_subst(body, local))\n"
                "            return ''.join(parts)\n"
                "        return _subst(body, context) if _lookup(ifcond, context) else ''\n"
                "    text = _BLOCK.sub(repl, template)\n"
                "    return _subst(text, context)\n"
            ),
        },
    },
    {
        "id": "query_engine",
        "dim": "implement-system",
        "files": {
            "db.py": (
                "class Table:\n"
                "    def __init__(self, rows):\n"
                "        # rows is a list of dict\n"
                "        ...\n"
                "    # TODO: where(pred), order_by(key, reverse=False), select(*cols),\n"
                "    #       limit(n) — each returns a NEW Table; rows() materializes.\n"
            ),
            "test_db.py": (
                "from db import Table\n\n"
                "def test_basic():\n"
                "    t = Table([{'a': 1}, {'a': 2}])\n"
                "    assert t.where(lambda r: r['a'] > 1).rows() == [{'a': 2}]\n"
            ),
        },
        "prompt": "Implement the Table class in db.py over a list of dict rows. Methods: where(pred) keeps rows where pred(row) is truthy; order_by(key, reverse=False) sorts by row[key]; select(*cols) projects each row to only those keys; limit(n) takes the first n. Each returns a NEW Table (the original is never mutated) and they chain. rows() returns the current list of dicts. Make test_db.py pass.",
        "grader": (
            "from db import Table\n"
            "rows = [{'name': 'a', 'age': 30}, {'name': 'b', 'age': 25}, {'name': 'c', 'age': 35}]\n"
            "t = Table(rows)\n"
            "out = t.where(lambda r: r['age'] >= 30).order_by('age').select('name').rows()\n"
            "assert out == [{'name': 'a'}, {'name': 'c'}], out\n"
            "assert t.rows() == rows\n"  # original unchanged
            "assert Table(rows).order_by('age', reverse=True).limit(1).select('name').rows() == [{'name': 'c'}]\n"
            "assert Table([]).where(lambda r: True).rows() == []\n"
        ),
        "claude": {
            "db.py": (
                "class Table:\n"
                "    def __init__(self, rows):\n"
                "        self._rows = [dict(r) for r in rows]\n\n"
                "    def where(self, pred):\n"
                "        return Table([r for r in self._rows if pred(r)])\n\n"
                "    def order_by(self, key, reverse=False):\n"
                "        return Table(sorted(self._rows, key=lambda r: r[key], reverse=reverse))\n\n"
                "    def select(self, *cols):\n"
                "        return Table([{c: r[c] for c in cols if c in r} for r in self._rows])\n\n"
                "    def limit(self, n):\n"
                "        return Table(self._rows[:n])\n\n"
                "    def rows(self):\n"
                "        return [dict(r) for r in self._rows]\n"
            ),
        },
    },
    {
        "id": "expr_language",
        "dim": "multi-file",
        "files": {
            "lexer.py": (
                "import re\n"
                "_TOKEN = re.compile(r'\\s*(?:(\\d+)|([A-Za-z_]\\w*)|(.))')\n\n"
                "def tokenize(s):\n"
                "    out = []\n"
                "    for num, name, sym in _TOKEN.findall(s):\n"
                "        if num:\n"
                "            out.append(('NUM', int(num)))\n"
                "        elif name:\n"
                "            out.append(('NAME', name))\n"
                "        elif sym and not sym.isspace():\n"
                "            out.append(('OP', sym))\n"
                "    return out\n"
            ),
            "evaluator.py": (
                "from lexer import tokenize\n\n"
                "def run(program, env=None):\n"
                "    \"\"\"Evaluate ';'-separated statements. A statement is `name = expr`\n"
                "    (assignment) or `expr`. Expressions: + - * / and parentheses over\n"
                "    integers, variables (from env or earlier assignments), and the\n"
                "    built-in functions min, max, abs with comma-separated args. Return\n"
                "    the value of the LAST expression statement. Use tokenize().\"\"\"\n"
                "    # TODO: implement\n"
                "    raise NotImplementedError\n"
            ),
            "test_evaluator.py": (
                "from evaluator import run\n\n"
                "def test_basic():\n"
                "    assert run('1 + 2 * 3') == 7\n"
            ),
        },
        "prompt": "Implement run(program, env=None) in evaluator.py per its docstring. The tokenizer is already implemented in lexer.py — read it and use tokenize(). Support assignments (`x = 3`), variables, + - * / with correct precedence and parentheses, integer division truncating toward zero, and the functions min/max/abs with comma-separated arguments. Return the value of the last expression statement. Make test_evaluator.py pass.",
        "grader": (
            "from evaluator import run\n"
            "assert run('1 + 2 * 3') == 7\n"
            "assert run('x = 3; x * 2') == 6\n"
            "assert run('x = 3; y = x + 1; max(x, y)') == 4\n"
            "assert run('abs(0 - 5)') == 5\n"
            "assert run('min(3, 1, 2) + max(4, 5)') == 6\n"
            "assert run('(a + b) * 2', {'a': 1, 'b': 2}) == 6\n"
            "assert run('10 / 3') == 3\n"
        ),
        "claude": {
            "evaluator.py": (
                "from lexer import tokenize\n\n"
                "_FUNCS = {'min': min, 'max': max, 'abs': lambda *a: abs(a[0])}\n\n"
                "def run(program, env=None):\n"
                "    env = dict(env or {})\n"
                "    last = None\n"
                "    for stmt in program.split(';'):\n"
                "        stmt = stmt.strip()\n"
                "        if not stmt:\n"
                "            continue\n"
                "        toks = tokenize(stmt)\n"
                "        if len(toks) >= 2 and toks[0][0] == 'NAME' and toks[1] == ('OP', '='):\n"
                "            env[toks[0][1]] = _eval(toks[2:], env)\n"
                "            last = None\n"
                "        else:\n"
                "            last = _eval(toks, env)\n"
                "    return last\n\n"
                "def _eval(toks, env):\n"
                "    pos = 0\n"
                "    def peek():\n"
                "        return toks[pos] if pos < len(toks) else (None, None)\n"
                "    def expr():\n"
                "        nonlocal pos\n"
                "        val = term()\n"
                "        while peek() in (('OP', '+'), ('OP', '-')):\n"
                "            op = toks[pos][1]; pos += 1\n"
                "            r = term()\n"
                "            val = val + r if op == '+' else val - r\n"
                "        return val\n"
                "    def term():\n"
                "        nonlocal pos\n"
                "        val = factor()\n"
                "        while peek() in (('OP', '*'), ('OP', '/')):\n"
                "            op = toks[pos][1]; pos += 1\n"
                "            r = factor()\n"
                "            val = val * r if op == '*' else int(val / r)\n"
                "        return val\n"
                "    def factor():\n"
                "        nonlocal pos\n"
                "        kind, v = peek()\n"
                "        if (kind, v) == ('OP', '('):\n"
                "            pos += 1\n"
                "            val = expr()\n"
                "            pos += 1\n"
                "            return val\n"
                "        if (kind, v) == ('OP', '-'):\n"
                "            pos += 1\n"
                "            return -factor()\n"
                "        if kind == 'NUM':\n"
                "            pos += 1\n"
                "            return v\n"
                "        if kind == 'NAME':\n"
                "            pos += 1\n"
                "            if peek() == ('OP', '('):\n"
                "                pos += 1\n"
                "                args = []\n"
                "                if peek() != ('OP', ')'):\n"
                "                    args.append(expr())\n"
                "                    while peek() == ('OP', ','):\n"
                "                        pos += 1\n"
                "                        args.append(expr())\n"
                "                pos += 1\n"
                "                return _FUNCS[v](*args)\n"
                "            return env[v]\n"
                "        raise ValueError(f'unexpected token {kind} {v}')\n"
                "    return expr()\n"
            ),
        },
    },
    {
        "id": "build_resolver",
        "dim": "implement-algo",
        "files": {
            "build.py": (
                "def resolve(graph):\n"
                "    \"\"\"graph maps target -> list of dependency targets. Return a build\n"
                "    order where every dependency comes before the targets that need it,\n"
                "    de-duplicated. Raise ValueError if there is a dependency cycle. A\n"
                "    dependency not present as a key is a leaf.\"\"\"\n"
                "    # TODO: implement\n"
                "    raise NotImplementedError\n"
            ),
            "test_build.py": (
                "from build import resolve\n\n"
                "def test_basic():\n"
                "    o = resolve({'a': [], 'b': ['a']})\n"
                "    assert o.index('a') < o.index('b')\n"
            ),
        },
        "prompt": "Implement resolve(graph) in build.py per its docstring: a topological build order (dependencies first), de-duplicated, raising ValueError on a cycle, treating a dependency that isn't a key as a leaf. Make test_build.py pass.",
        "grader": (
            "from build import resolve\n"
            "g = {'app': ['lib', 'util'], 'lib': ['util'], 'util': [], 'test': ['app', 'lib']}\n"
            "o = resolve(g)\n"
            "for t, deps in g.items():\n"
            "    for d in deps:\n"
            "        assert o.index(d) < o.index(t)\n"
            "assert len(o) == len(set(o)) == 4\n"
            "o2 = resolve({'x': ['y'], 'y': []})\n"
            "assert o2.index('y') < o2.index('x')\n"
            "raised = False\n"
            "try:\n"
            "    resolve({'a': ['b'], 'b': ['a']})\n"
            "except ValueError:\n"
            "    raised = True\n"
            "assert raised\n"
        ),
        "claude": {
            "build.py": (
                "def resolve(graph):\n"
                "    order, visited, visiting = [], set(), set()\n\n"
                "    def dfs(node):\n"
                "        if node in visited:\n"
                "            return\n"
                "        if node in visiting:\n"
                "            raise ValueError(f'cycle at {node}')\n"
                "        visiting.add(node)\n"
                "        for dep in graph.get(node, []):\n"
                "            dfs(dep)\n"
                "        visiting.discard(node)\n"
                "        visited.add(node)\n"
                "        order.append(node)\n\n"
                "    for node in graph:\n"
                "        dfs(node)\n"
                "    return order\n"
            ),
        },
    },
    {
        "id": "cross_file_precedence_bug",
        "dim": "multi-file-debug",
        "files": {
            "tokenizer.py": (
                "def tokens(s):\n"
                "    out, i = [], 0\n"
                "    while i < len(s):\n"
                "        c = s[i]\n"
                "        if c.isspace():\n"
                "            i += 1\n"
                "        elif c.isdigit():\n"
                "            j = i\n"
                "            while j < len(s) and s[j].isdigit():\n"
                "                j += 1\n"
                "            out.append(int(s[i:j])); i = j\n"
                "        else:\n"
                "            out.append(c); i += 1\n"
                "    return out\n"
            ),
            "calc_engine.py": (
                "from tokenizer import tokens\n\n"
                "def evaluate(expr):\n"
                "    ts = tokens(expr)\n"
                "    # evaluates strictly left-to-right (bug: ignores * and / precedence)\n"
                "    val = ts[0]\n"
                "    i = 1\n"
                "    while i < len(ts):\n"
                "        op, rhs = ts[i], ts[i + 1]\n"
                "        i += 2\n"
                "        if op == '+':\n"
                "            val += rhs\n"
                "        elif op == '-':\n"
                "            val -= rhs\n"
                "        elif op == '*':\n"
                "            val *= rhs\n"
                "        elif op == '/':\n"
                "            val = int(val / rhs)\n"
                "    return val\n"
            ),
            "repl.py": (
                "from calc_engine import evaluate\n\n"
                "def run(line):\n"
                "    return evaluate(line)\n"
            ),
            "test_repl.py": (
                "from repl import run\n\n"
                "def test_precedence():\n"
                "    assert run('1 + 2 * 3') == 7\n"
            ),
        },
        "prompt": "test_repl.py fails: the REPL returns wrong answers for expressions that mix + - with * /. repl.py calls calc_engine.py (which uses tokenizer.py). Trace the problem across the files, find the root cause, and fix it so operator precedence is correct (* and / bind tighter than + and -). Do not change the tests or the tokenizer.",
        "grader": (
            "from repl import run\n"
            "assert run('1 + 2 * 3') == 7\n"
            "assert run('2 + 3 * 4') == 14\n"
            "assert run('10 - 2 * 3') == 4\n"
            "assert run('2 * 3 + 4') == 10\n"
            "assert run('20 / 4 + 1') == 6\n"
            "assert run('100') == 100\n"
        ),
        "claude": {
            "calc_engine.py": (
                "from tokenizer import tokens\n\n"
                "def evaluate(expr):\n"
                "    ts = tokens(expr)\n"
                "    # pass 1: resolve * and / (higher precedence)\n"
                "    acc = [ts[0]]\n"
                "    i = 1\n"
                "    while i < len(ts):\n"
                "        op, rhs = ts[i], ts[i + 1]\n"
                "        i += 2\n"
                "        if op == '*':\n"
                "            acc[-1] = acc[-1] * rhs\n"
                "        elif op == '/':\n"
                "            acc[-1] = int(acc[-1] / rhs)\n"
                "        else:\n"
                "            acc.append(op)\n"
                "            acc.append(rhs)\n"
                "    # pass 2: resolve + and - left to right\n"
                "    val = acc[0]\n"
                "    j = 1\n"
                "    while j < len(acc):\n"
                "        op, rhs = acc[j], acc[j + 1]\n"
                "        j += 2\n"
                "        val = val + rhs if op == '+' else val - rhs\n"
                "    return val\n"
            ),
        },
    },
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--track", choices=["zode", "claude", "both"], default="both")
    ap.add_argument("-j", "--jobs", type=int, default=3)
    args = ap.parse_args()
    print(f"{len(TASKS)} complex multi-file tasks\n")

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
            print(f"  [{'PASS' if r['passed'] else 'FAIL'}] zode    {r['id']:<28} {r['secs']:>5}s"
                  f"  tools: {','.join(r['tools']) or '(none)'}"
                  + (f"  ({r['detail']})" if not r["passed"] else ""))
        tools = sorted({t for r in zres for t in r["tools"]})
        out["zode"] = {"total": [sum(r["passed"] for r in zres), len(zres)],
                       "tools_seen": tools, "results": zres}

    (Path(__file__).resolve().parent / "complex_results.json").write_text(json.dumps(out, indent=2))
    print()
    for track, s in out.items():
        p, n = s["total"]
        print(f"{track}: {p}/{n} ({100 * p // n}%)")


if __name__ == "__main__":
    main()
