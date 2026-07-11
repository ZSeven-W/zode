import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ZodeClient } from "../dist/index.js";

const binary = process.env.ZODE_BIN;

test("stdio basic run against ZODE_BIN", { skip: !binary }, async () => {
  if (!binary) {
    console.error("ZODE_BIN unset; skipping TypeScript SDK stdio e2e");
    return;
  }
  const configDir = await mkdtemp(join(tmpdir(), `zode-ts-e2e-${process.pid}-`));
  await writeFile(join(configDir, "config.json"), JSON.stringify({ provider: { type: "anthropic" }, sandbox: { enabled: false } }));
  const client = new ZodeClient({ binary, env: { ...process.env, ZODE_CONFIG_DIR: configDir } });
  try {
    const initialize = await client.initialize("typescript-sdk-e2e", "0.1.0", { approvalPolicy: "auto" });
    assert.equal(initialize.approvalPolicy, "auto");
    const started = await client.request("thread/start", { cwd: process.cwd() });
    const seen = [];
    client.onNotification((notification) => seen.push(notification.method));
    await client.request("turn/start", { threadId: started.thread.id, input: "echo hi" });
    await new Promise((resolve, reject) => {
      const deadline = setTimeout(() => reject(new Error(`missing turn notifications: ${seen.join(",")}`)), 5_000);
      const poll = () => {
        if (seen.includes("turn/started") && seen.includes("turn/failed")) {
          clearTimeout(deadline);
          resolve();
        } else setTimeout(poll, 10);
      };
      poll();
    });
    const command = await client.request("command/exec", { command: ["sh", "-c", "printf hi"] });
    assert.equal(command.stdout, "hi");
    assert.equal(command.exitCode, 0);
  } finally {
    client.close();
  }
});
