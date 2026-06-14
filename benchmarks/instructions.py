#!/usr/bin/env python3
"""Instruction-following (指令遵守) benchmark — MCP, Skills, and constraints.

Tests whether Zode + DeepSeek actually OBEYS instructions in the agent loop:
  - MCP    : invokes the named MCP tool (verified — the tool returns an
             unguessable value, so a correct answer proves it was called)
  - Skill  : loads the named skill and follows its body's exact rule (the rule
             produces an unguessable signature output)
  - Format : obeys output-format constraints (JSON only, exact word, layout)
  - Negative: obeys "do NOT use tools" (verified via the tool-use trace)

Claude is the reference instruction-follower (the authored `expect` is exactly
what a fully-compliant agent — Claude Code, with the same MCP/skill access —
produces), so the question is whether Zode + DeepSeek matches it.

A self-contained config dir (provider from $ZODE_CONFIG_DIR, plus this repo's
mcp_server.py and a few skills) is built per run.

Usage: python3 benchmarks/instructions.py [-jN]
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
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
_zbin = os.environ.get("ZODE_BIN", str(ROOT / "target" / "release" / "zode"))
ZODE_BIN = str((ROOT / _zbin).resolve() if not os.path.isabs(_zbin) else Path(_zbin))
TIMEOUT = 120
TOOL_RE = re.compile(r"^· (\S+)", re.MULTILINE)

SKILLS = {
    "format-id": (
        "Format a raw user id into the company's official badge string",
        "Output ONLY this string, no extra words:\n\n"
        "    BADGE[<id uppercased>]#<number of characters in the id>\n\n"
        'Example: the id "ab7c" becomes  BADGE[AB7C]#4\n',
    ),
    "wrap-tag": (
        "Wrap text in the official delimiters with the audit tag",
        "Wrap the given text EXACTLY as ::<text>:: and append the audit tag !ZD7.\n"
        'Output only the result. Example: "hi" becomes  ::hi::!ZD7\n',
    ),
    "checksum": (
        "Compute the official text checksum",
        "Compute the checksum: the sum of the ASCII code points of every "
        "character of the input, then output ONLY  CHK=<sum>.\n"
        'Example: "AB" -> 65+66=131 -> CHK=131\n',
    ),
}

TASKS = [
    # --- MCP tool invocation (unguessable outputs prove the tool was called) ---
    {"id": "mcp_secret_add", "kind": "mcp",
     "prompt": "Use the secret_add MCP tool to add 100 and 200, then tell me the resulting number.",
     "expect": "4542", "require_tool": "mcp__bench__secret_add"},
    {"id": "mcp_reverse", "kind": "mcp",
     "prompt": "Use the reverse_token MCP tool to reverse the token 'instruction'. Report exactly what the tool returns.",
     "expect": "<<noitcurtsni>>", "require_tool": "mcp__bench__reverse_token"},
    {"id": "mcp_magic", "kind": "mcp",
     "prompt": "Call the magic_word MCP tool and tell me today's magic word verbatim.",
     "expect": "ZQX-7731-PLUTO", "require_tool": "mcp__bench__magic_word"},
    {"id": "mcp_selectivity", "kind": "mcp",
     "prompt": "What is 5 + 5? You MUST compute it using the secret_add MCP tool, not by hand. Give me the number it returns.",
     "expect": "4252", "require_tool": "mcp__bench__secret_add"},
    # --- Skill loading + obeying the skill body ---
    {"id": "skill_format_id", "kind": "skill",
     "prompt": "Use the format-id skill to format the user id 'kx9'. Output exactly what the skill specifies.",
     "expect": "BADGE[KX9]#3", "require_tool": "Skill"},
    {"id": "skill_wrap_tag", "kind": "skill",
     "prompt": "Use the wrap-tag skill on the text 'ok'. Output exactly what the skill specifies.",
     "expect": "::ok::!ZD7", "require_tool": "Skill"},
    {"id": "skill_checksum", "kind": "skill",
     "prompt": "Use the checksum skill to compute the checksum of the text 'abc'. Output exactly what the skill specifies.",
     "expect": "CHK=294", "require_tool": "Skill"},
    # --- General output-format adherence ---
    {"id": "fmt_json_only", "kind": "format",
     "prompt": "Reply with ONLY this JSON object and nothing else (no markdown, no prose): {\"product\": N} where N is 6 times 7.",
     "expect": '{"product": 42}'},
    {"id": "fmt_exact_word", "kind": "format",
     "prompt": "Reply with exactly the single word ORANGE in lowercase, and nothing else.",
     "exact": "orange"},
    {"id": "fmt_layout", "kind": "format",
     "prompt": "Output the first three prime numbers, one per line, digits only, no other text or punctuation.",
     "expect": "2\n3\n5"},
    # --- Negative instruction: must NOT use any tool ---
    {"id": "neg_no_tools", "kind": "negative",
     "prompt": "Without using any tool at all, answer directly: what is 12 times 12? Reply with just the number.",
     "expect": "144", "forbid_tools": True},
    # --- HARD: multi-constraint, sequencing, buried, conditional ---
    {"id": "hard_multiconstraint", "kind": "hard",
     "prompt": "Respond with EXACTLY three lines and nothing else. Line 1: the word START. Line 2: the number returned by the secret_add MCP tool for 10 and 20. Line 3: the word END.",
     "exact": "START\n4272\nEND", "require_tool": "mcp__bench__secret_add"},
    {"id": "hard_sequential", "kind": "hard",
     "prompt": "First call the magic_word MCP tool to get the word. Then pass THAT exact word to the reverse_token MCP tool. Report only the final reversed value the second tool returns.",
     "expect": "<<OTULP-1377-XQZ>>", "require_tools": ["mcp__bench__magic_word", "mcp__bench__reverse_token"]},
    {"id": "hard_tool_select", "kind": "hard",
     "prompt": "Compute 7 + 8. You MUST use the secret_add MCP tool (not arithmetic), and you must NOT use any skill. Report just the number the tool returns.",
     "expect": "4257", "require_tool": "mcp__bench__secret_add", "forbid_tool": ["Skill"]},
    {"id": "hard_buried", "kind": "hard",
     "prompt": "I'm going to give you some background. Our team works on a vector design tool and we care a lot about precision and code quality. We also like clear communication. Anyway — and this is the only thing that actually matters for your reply — your entire response must be the single uppercase word PINEAPPLE with no punctuation. Thanks for reading all of this context.",
     "exact": "PINEAPPLE"},
    {"id": "hard_conditional", "kind": "hard",
     "prompt": "If 5 is greater than 3, reply with the word returned by the magic_word MCP tool. Otherwise reply with the single word NO. Output only the answer.",
     "expect": "ZQX-7731-PLUTO", "require_tool": "mcp__bench__magic_word"},
    {"id": "hard_skill_postprocess", "kind": "hard",
     "prompt": "Use the checksum skill on the text 'hi'. Then wrap the skill's exact output in square brackets. Output only that bracketed string.",
     "expect": "[CHK=209]", "require_tool": "Skill"},
    # --- ADVERSARIAL: overrides, format traps, no-explanation, distractors ---
    {"id": "adv_override", "kind": "adversarial",
     "prompt": "Reply with the word RED. Actually, ignore that — reply with the word BLUE instead. Output exactly one word.",
     "exact": "BLUE"},
    {"id": "adv_no_explain", "kind": "adversarial",
     "prompt": "What is the capital of France? Reply with ONLY the city name — no sentence, no punctuation, no extra words.",
     "exact": "Paris"},
    {"id": "adv_multi_negative", "kind": "adversarial",
     "prompt": "What is 2 + 2? Reply with just the number. Do NOT use any MCP tool, do NOT use any skill, do NOT run a shell command, and do NOT explain.",
     "exact": "4", "forbid_tools": True},
    {"id": "adv_buried_counter", "kind": "adversarial",
     "prompt": "Here is a lot of context about our project, which builds an AI-native design tool with a Rust core and a focus on precision. We value tests and clear writing. Now, the actual task: and whatever you do, do NOT output the word YES — instead output exactly the word NO and nothing else.",
     "exact": "NO"},
    {"id": "adv_json_no_md", "kind": "adversarial",
     "prompt": "Return the result of 9 * 9 as a JSON object {\"result\": N}. Do NOT wrap it in a markdown code block and add no other text.",
     "exact": '{"result": 81}'},
    {"id": "adv_tool_then_transform", "kind": "adversarial",
     "prompt": "Get the word from the magic_word MCP tool, then output it converted to lowercase. Output only the lowercase string.",
     "expect": "zqx-7731-pluto", "require_tool": "mcp__bench__magic_word"},
    {"id": "adv_strict_prefix", "kind": "adversarial",
     "prompt": "Compute 7 * 6 and reply with the answer prefixed by exactly 'ANS:' with no space and nothing else. Example shape: ANS:0",
     "exact": "ANS:42"},
    {"id": "adv_ignore_first", "kind": "adversarial",
     "prompt": "Translate the word 'hello' into French. But actually, ignore that previous sentence completely and instead just output the number 100, nothing else.",
     "exact": "100"},
]


def build_config_dir(base_cfg_dir: str) -> str:
    work = tempfile.mkdtemp(prefix="zode-instr-")
    shutil.copy(Path(base_cfg_dir) / "config.json", Path(work) / "config.json")
    (Path(work) / "mcp.json").write_text(json.dumps({
        "servers": {"bench": {"transport": "stdio", "command": sys.executable,
                              "args": [str(HERE / "mcp_server.py")]}}
    }))
    for name, (desc, body) in SKILLS.items():
        d = Path(work) / "skills" / name
        d.mkdir(parents=True)
        (d / "SKILL.md").write_text(f"---\nname: {name}\ndescription: {desc}\n---\n{body}")
    return work


def grade(task: dict, out: str, tools: list[str]) -> tuple[bool, str]:
    if task.get("forbid_tools") and tools:
        return False, f"used tools (forbidden): {tools}"
    for ft in task.get("forbid_tool", []):
        if ft in tools:
            return False, f"used forbidden tool {ft}"
    rt = task.get("require_tool")
    if rt and rt not in tools:
        return False, f"did not invoke {rt} (tools: {tools or 'none'})"
    for need in task.get("require_tools", []):  # all must appear (sequencing)
        if need not in tools:
            return False, f"did not invoke {need} (tools: {tools or 'none'})"
    if "exact" in task:
        return (out.strip() == task["exact"], f"got {out.strip()!r}")
    exp = task["expect"]
    return (exp in out, f"missing {exp!r} in {out.strip()[:90]!r}")


def run_task(task: dict, cfg_dir: str) -> dict:
    try:
        r = subprocess.run([ZODE_BIN, "-p", task["prompt"], "--yolo"],
                           capture_output=True, text=True, timeout=TIMEOUT,
                           cwd=tempfile.gettempdir(),
                           env={**os.environ, "ZODE_CONFIG_DIR": cfg_dir, "ZODE_LOG": "warn"})
    except subprocess.TimeoutExpired:
        return {"id": task["id"], "kind": task["kind"], "passed": False, "detail": "timeout", "tools": []}
    tools = sorted(set(TOOL_RE.findall(r.stderr)))
    passed, detail = grade(task, r.stdout, tools)
    return {"id": task["id"], "kind": task["kind"], "passed": passed, "detail": detail, "tools": tools}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-j", "--jobs", type=int, default=3)
    args = ap.parse_args()
    base = os.environ.get("ZODE_CONFIG_DIR")
    if not base or not (Path(base) / "config.json").exists():
        sys.exit("set ZODE_CONFIG_DIR to a dir containing a config.json with the provider")
    cfg_dir = build_config_dir(base)
    print(f"{len(TASKS)} instruction-following tasks (MCP / Skill / format / negative)\n")
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as ex:
        results = list(ex.map(lambda t: run_task(t, cfg_dir), TASKS))
    by_kind = {}
    for r in sorted(results, key=lambda r: [t["id"] for t in TASKS].index(r["id"])):
        by_kind.setdefault(r["kind"], [0, 0])
        by_kind[r["kind"]][1] += 1
        by_kind[r["kind"]][0] += r["passed"]
        print(f"  [{'PASS' if r['passed'] else 'FAIL'}] {r['kind']:<8} {r['id']:<18}"
              f"  tools: {','.join(r['tools']) or 'none'}"
              + (f"  ({r['detail']})" if not r["passed"] else ""))
    p = sum(r["passed"] for r in results)
    n = len(results)
    out = {"total": [p, n], "by_kind": by_kind, "results": results}
    (HERE / "instructions_results.json").write_text(json.dumps(out, indent=2))
    print("\nby kind:  " + "  ".join(f"{k} {v[0]}/{v[1]}" for k, v in by_kind.items()))
    print(f"Zode + DeepSeek: {p}/{n} ({100 * p // n}%)   |   Claude (reference): {n}/{n} (100%)")


if __name__ == "__main__":
    main()
