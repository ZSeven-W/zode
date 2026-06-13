#!/usr/bin/env python3
"""Does the harness close the one-shot code-gen gap?

The two tasks DeepSeek occasionally misses one-shot (parse_ini, csv_parse_row)
are re-run *agentically*: implement in a file, then self-verify by running it
in the shell and fix until correct. Same hidden grader as the one-shot suite.
If agentic passes reliably where one-shot was ~67%, the harness is the fix.

Usage: python3 benchmarks/selfverify.py [-jN] [--runs N]
"""
import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from agentic import run_zode_task  # noqa: E402

TASKS = [
    {
        "id": "parse_ini",
        "dim": "parsing",
        "files": {"iniparse.py": (
            "def parse_ini(text):\n"
            "    \"\"\"Parse a simple INI string into a dict of dicts.\n"
            "    '[section]' starts a section; 'key = value' adds to the current\n"
            "    section (strip whitespace around key and value); blank lines and\n"
            "    lines starting with ';' or '#' are ignored; keys before any section\n"
            "    go under section ''.\"\"\"\n"
            "    # TODO: implement\n"
            "    raise NotImplementedError\n"
        )},
        "prompt": "Implement parse_ini(text) in iniparse.py per its docstring. Then VERIFY it by running python in the shell against tricky cases — empty string should give {}, a comment-only input should give {}, keys before any [section] go under '', ';' and '#' lines are comments, and whitespace around keys/values is stripped — and fix it until all your checks pass.",
        "grader": (
            "from iniparse import parse_ini\n"
            "ini = 'g=0\\n[a]\\nx = 1\\n; comment\\ny=2\\n\\n[b]\\nz =  three  '\n"
            "assert parse_ini(ini) == {'': {'g': '0'}, 'a': {'x': '1', 'y': '2'}, 'b': {'z': 'three'}}\n"
            "assert parse_ini('') == {}\n"
            "assert parse_ini('# only comment') == {}\n"
        ),
        "claude": {"iniparse.py": (
            "def parse_ini(text):\n"
            "    res, section = {}, ''\n"
            "    for raw in text.split('\\n'):\n"
            "        line = raw.strip()\n"
            "        if not line or line[0] in ';#':\n"
            "            continue\n"
            "        if line.startswith('[') and line.endswith(']'):\n"
            "            section = line[1:-1].strip()\n"
            "            res.setdefault(section, {})\n"
            "        elif '=' in line:\n"
            "            k, v = line.split('=', 1)\n"
            "            res.setdefault(section, {})[k.strip()] = v.strip()\n"
            "    return res\n"
        )},
    },
    {
        "id": "csv_parse_row",
        "dim": "parsing",
        "files": {"csvrow.py": (
            "def csv_parse_row(line):\n"
            "    \"\"\"Parse ONE CSV line into a list of field strings. Double-quoted\n"
            "    fields may contain commas and escaped quotes (\\\"\\\" inside a quoted\n"
            "    field means a literal \\\"). Surrounding quotes are removed. Do not\n"
            "    use the csv module.\"\"\"\n"
            "    # TODO: implement\n"
            "    raise NotImplementedError\n"
        )},
        "prompt": "Implement csv_parse_row(line) in csvrow.py per its docstring. Then VERIFY by running python in the shell on tricky cases — a quoted field containing a comma, a quoted field containing escaped \"\" quotes, an empty string, and consecutive commas (empty fields) — and fix until your checks pass. Don't use the csv module.",
        "grader": (
            "from csvrow import csv_parse_row\n"
            "assert csv_parse_row('a,b,c') == ['a', 'b', 'c']\n"
            "assert csv_parse_row('a,\"b,c\",d') == ['a', 'b,c', 'd']\n"
            "assert csv_parse_row('\"he said \"\"hi\"\"\",x') == ['he said \"hi\"', 'x']\n"
            "assert csv_parse_row('') == ['']\n"
            "assert csv_parse_row('a,,c') == ['a', '', 'c']\n"
        ),
        "claude": {"csvrow.py": (
            "def csv_parse_row(line):\n"
            "    fields, cur, i, n = [], [], 0, len(line)\n"
            "    while i < n:\n"
            "        if line[i] == '\"':\n"
            "            i += 1\n"
            "            while i < n:\n"
            "                if line[i] == '\"':\n"
            "                    if i + 1 < n and line[i + 1] == '\"':\n"
            "                        cur.append('\"'); i += 2\n"
            "                    else:\n"
            "                        i += 1; break\n"
            "                else:\n"
            "                    cur.append(line[i]); i += 1\n"
            "        elif line[i] == ',':\n"
            "            fields.append(''.join(cur)); cur = []; i += 1\n"
            "        else:\n"
            "            cur.append(line[i]); i += 1\n"
            "    fields.append(''.join(cur))\n"
            "    return fields\n"
        )},
    },
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-j", "--jobs", type=int, default=2)
    ap.add_argument("--runs", type=int, default=3)
    args = ap.parse_args()
    counts = {t["id"]: 0 for t in TASKS}
    for run in range(args.runs):
        for t in TASKS:
            r = run_zode_task(t)
            counts[t["id"]] += int(r["passed"])
            print(f"  run{run + 1} [{'PASS' if r['passed'] else 'FAIL'}] {t['id']:<16} {r['secs']}s tools:{','.join(r['tools'])}")
    print("\nagentic self-verify pass rate (one-shot was ~67% on these):")
    for tid, c in counts.items():
        print(f"  {tid}: {c}/{args.runs}")


if __name__ == "__main__":
    main()
