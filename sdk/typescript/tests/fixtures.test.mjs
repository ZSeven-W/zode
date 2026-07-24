import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const fixtures = {
  "initialize.request": { id: "init", jsonrpc: "2.0", method: "initialize", params: { approvalPolicy: "readOnly", clientInfo: { name: "fixture", version: "0.0.0" } } },
  "initialize.response": { id: "init", jsonrpc: "2.0", result: { approvalPolicy: "readOnly", capabilities: ["threads", "turns", "fs", "command", "models", "config", "skills", "hooks", "mcp", "plugins"], platformFamily: "unix", platformOs: "macos", serverInfo: { name: "zode", version: "0.1.0-beta.7" }, zodeHome: "/tmp/zode" } },
  "thread-start.request": { id: "thread", jsonrpc: "2.0", method: "thread/start", params: { cwd: "/tmp/project", model: "default" } },
  "fs-read-file.request": { id: "read", jsonrpc: "2.0", method: "fs/readFile", params: { path: "/tmp/project/hello.txt" } },
  "command-exec.request": { id: "cmd", jsonrpc: "2.0", method: "command/exec", params: { command: ["sh", "-c", "printf hi"] } },
};

for (const [name, expected] of Object.entries(fixtures)) {
  test(`fixture ${name} has semantic parity`, async () => {
    const parsed = JSON.parse(await readFile(new URL(`../../fixtures/jsonrpc/${name}.json`, import.meta.url)));
    assert.deepEqual(parsed, expected);
  });
}
