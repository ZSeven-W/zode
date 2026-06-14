#!/usr/bin/env python3
"""Minimal stdio MCP server for the instruction-following benchmark.

Speaks newline-delimited JSON-RPC 2.0 over stdin/stdout (MCP stdio transport).
Its tools return values the model CANNOT guess (a hidden offset / a random-
looking token), so a correct final answer proves the agent actually invoked the
tool rather than computing it itself.

Tools:
  secret_add(a, b)      -> a + b + 4242     (hidden offset)
  reverse_token(text)   -> text reversed, wrapped as <<...>>
  magic_word()          -> a fixed unguessable token
"""
import json
import sys

TOOLS = [
    {
        "name": "secret_add",
        "description": "Add two integers using the team's secret accounting rule. Always returns the official total — do not compute it yourself.",
        "inputSchema": {
            "type": "object",
            "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
            "required": ["a", "b"],
        },
    },
    {
        "name": "reverse_token",
        "description": "Return the official reversed form of a token. Do not reverse it yourself; the server applies the canonical wrapping.",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    },
    {
        "name": "magic_word",
        "description": "Return today's magic word (only the server knows it).",
        "inputSchema": {"type": "object", "properties": {}},
    },
]


def call(name, args):
    if name == "secret_add":
        return str(int(args["a"]) + int(args["b"]) + 4242)
    if name == "reverse_token":
        return "<<" + args["text"][::-1] + ">>"
    if name == "magic_word":
        return "ZQX-7731-PLUTO"
    raise ValueError(f"unknown tool {name}")


def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except Exception:
            continue
        mid = msg.get("id")
        method = msg.get("method")
        if method == "initialize":
            proto = (msg.get("params") or {}).get("protocolVersion", "2024-11-05")
            send({"jsonrpc": "2.0", "id": mid, "result": {
                "protocolVersion": proto,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "bench", "version": "1.0.0"},
            }})
        elif method in ("notifications/initialized", "initialized"):
            pass  # notification, no response
        elif method == "tools/list":
            send({"jsonrpc": "2.0", "id": mid, "result": {"tools": TOOLS}})
        elif method == "tools/call":
            params = msg.get("params") or {}
            try:
                text = call(params.get("name"), params.get("arguments") or {})
                send({"jsonrpc": "2.0", "id": mid, "result": {
                    "content": [{"type": "text", "text": text}], "isError": False,
                }})
            except Exception as e:
                send({"jsonrpc": "2.0", "id": mid, "result": {
                    "content": [{"type": "text", "text": str(e)}], "isError": True,
                }})
        elif mid is not None:
            send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}})


if __name__ == "__main__":
    main()
