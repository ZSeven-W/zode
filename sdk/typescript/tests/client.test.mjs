import test from "node:test";
import assert from "node:assert/strict";
import { ProtocolMethod, ZodeClient } from "../dist/index.js";

test("defaults to zode binary", () => {
  const client = new ZodeClient();
  assert.equal(client.binary, "zode");
});

test("allows binary override", () => {
  const client = new ZodeClient({ binary: "/tmp/zode" });
  assert.equal(client.binary, "/tmp/zode");
});

test("protocol method enum exposes wire names", () => {
  assert.equal(ProtocolMethod.Initialize, "initialize");
  assert.equal(ProtocolMethod.CommandExec, "command/exec");
  assert.equal(ProtocolMethod.McpServerStatusList, "mcpServerStatus/list");
});
