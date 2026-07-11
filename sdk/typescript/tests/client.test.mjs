import test from "node:test";
import assert from "node:assert/strict";
import { chmod, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:http";
import { WebSocketServer } from "ws";
import {
  ProtocolMethod,
  ZodeClient,
  classifyIncomingFrame,
} from "../dist/index.js";

let scriptCounter = 0;

async function scriptedChild(body) {
  const directory = await mkdtemp(join(tmpdir(), `zode-ts-${process.pid}-${scriptCounter++}-`));
  const file = join(directory, "zode");
  await writeFile(file, `#!/bin/sh\n${body}\n`);
  await chmod(file, 0o755);
  return file;
}

test("defaults to zode binary", () => {
  const client = new ZodeClient();
  assert.equal(client.binary, "zode");
});

test("allows binary override", () => {
  const client = new ZodeClient({ binary: "/tmp/zode" });
  assert.equal(client.binary, "/tmp/zode");
});

test("protocol method enum matches the shared fixture", async () => {
  const schema = JSON.parse(await readFile(new URL("../../fixtures/jsonrpc/protocol.schema.json", import.meta.url)));
  assert.deepEqual(Object.values(ProtocolMethod), schema.methods);
  assert.equal(Object.values(ProtocolMethod).length, 27);
});

test("classifies every incoming JSON-RPC frame kind", () => {
  assert.equal(classifyIncomingFrame({ jsonrpc: "2.0", id: 1, result: {} }).kind, "response");
  assert.equal(classifyIncomingFrame({ jsonrpc: "2.0", id: 1, error: { code: -1, message: "x" } }).kind, "error");
  assert.equal(classifyIncomingFrame({ jsonrpc: "2.0", method: "turn/started", params: {} }).kind, "notification");
  assert.equal(classifyIncomingFrame({ jsonrpc: "2.0", id: "a", method: "approval/request", params: {} }).kind, "serverRequest");
  assert.throws(() => classifyIncomingFrame({ id: 1, result: {} }), /JSON-RPC/);
});

test("dispatches notifications while resolving out-of-order responses", async () => {
  const binary = await scriptedChild(`
read first
read second
printf '%s\\n' '{"jsonrpc":"2.0","method":"turn/started","params":{"turnId":"t"}}'
printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"value":"second"}}'
printf '%s\\n' '{"jsonrpc":"2.0","id":1,"result":{"value":"first"}}'
`);
  const client = new ZodeClient({ binary });
  const notifications = [];
  client.onNotification((notification) => notifications.push(notification));
  const first = client.request("one", {});
  const second = client.request("two", {});
  assert.deepEqual(await second, { value: "second" });
  assert.deepEqual(await first, { value: "first" });
  assert.deepEqual(notifications.map((value) => value.method), ["turn/started"]);
  client.close();
});

test("handles approval allow, rejected-handler deny, and missing-handler deny", async () => {
  const binary = await scriptedChild(`
read request
printf '%s\\n' '{"jsonrpc":"2.0","id":"approval-1","method":"approval/request","params":{"approvalId":"a1","kind":"command","summary":"run"}}'
read approval
case "$approval" in *'"decision":"allow"'*) printf '%s\\n' '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}';; *) exit 2;; esac
read request
printf '%s\\n' '{"jsonrpc":"2.0","id":"approval-2","method":"approval/request","params":{"approvalId":"a2","kind":"command","summary":"run"}}'
read approval
case "$approval" in *'"decision":"deny"'*) printf '%s\\n' '{"jsonrpc":"2.0","id":2,"result":{"ok":true}}';; *) exit 2;; esac
read request
printf '%s\\n' '{"jsonrpc":"2.0","id":"approval-3","method":"approval/request","params":{"approvalId":"a3","kind":"command","summary":"run"}}'
read approval
case "$approval" in *'"decision":"deny"'*) printf '%s\\n' '{"jsonrpc":"2.0","id":3,"result":{"ok":true}}';; *) exit 2;; esac
`);
  const client = new ZodeClient({ binary });
  for (const [handler, expected] of [
    [async () => "allow", "allow"],
    [async () => { throw new Error("no"); }, "deny"],
    [undefined, "deny"],
  ]) {
    const unregister = handler ? client.onApprovalRequest(handler) : undefined;
    assert.deepEqual(await client.request("test", {}), { ok: true });
    unregister?.();
    assert.ok(expected === "allow" || expected === "deny");
  }
  client.close();
});

test("initialize includes an explicitly selected approval policy", async () => {
  const binary = await scriptedChild(`
read request
case "$request" in *'"jsonrpc":"2.0"'*'"approvalPolicy":"auto"'*) printf '%s\\n' '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}';; *) exit 2;; esac
`);
  const client = new ZodeClient({ binary });
  assert.deepEqual(await client.initialize("test", "1", { approvalPolicy: "auto" }), { ok: true });
  client.close();
});

test("connectWebSocket authenticates and uses the same dispatcher", async (t) => {
  const httpServer = createServer();
  const server = new WebSocketServer({ server: httpServer });
  server.on("error", () => {});
  try {
    await new Promise((resolve, reject) => {
      httpServer.once("listening", resolve);
      httpServer.once("error", reject);
      httpServer.listen(0, "127.0.0.1");
    });
  } catch (error) {
    if (error?.code === "EPERM") {
      console.error("loopback listeners forbidden; skipping TypeScript SDK WebSocket transport test");
      t.skip("loopback listeners forbidden by sandbox");
      return;
    }
    throw error;
  }
  const address = httpServer.address();
  assert.equal(typeof address, "object");
  server.on("connection", (socket, request) => {
    assert.equal(request.headers.authorization, "Bearer secret");
    socket.once("message", (data) => {
      const requestFrame = JSON.parse(data.toString());
      assert.equal(requestFrame.jsonrpc, "2.0");
      socket.send(JSON.stringify({ jsonrpc: "2.0", method: "turn/completed", params: {} }));
      socket.send(JSON.stringify({ jsonrpc: "2.0", id: requestFrame.id, result: { ok: true } }));
    });
  });
  const client = await ZodeClient.connectWebSocket({ url: `ws://127.0.0.1:${address.port}`, token: "secret" });
  const notification = new Promise((resolve) => client.onNotification(resolve));
  assert.deepEqual(await client.request("test", {}), { ok: true });
  assert.equal((await notification).method, "turn/completed");
  client.close();
  await new Promise((resolve) => server.close(resolve));
  await new Promise((resolve) => httpServer.close(resolve));
});
