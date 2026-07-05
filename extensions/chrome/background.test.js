#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const backgroundPath = path.join(__dirname, "background.js");
const backgroundSource = fs.readFileSync(backgroundPath, "utf8");

function makeChrome(storage = {}) {
  return {
    storage: {
      local: {
        get: async () => storage,
        set: async () => {},
        remove: async () => {},
      },
    },
    alarms: {
      create: () => {},
      onAlarm: { addListener: () => {} },
    },
    runtime: {
      onInstalled: { addListener: () => {} },
      onStartup: { addListener: () => {} },
      onMessage: { addListener: () => {} },
    },
    tabs: {
      query: async () => [],
      create: async () => ({ id: 1, windowId: 1 }),
      remove: async () => {},
      update: async () => {},
      onRemoved: { addListener: () => {} },
    },
    tabGroups: {
      query: async () => [],
      update: async () => {},
      onRemoved: { addListener: () => {} },
    },
    debugger: {
      attach: async () => {},
      detach: async () => {},
      sendCommand: async () => ({}),
      onDetach: { addListener: () => {} },
      onEvent: { addListener: () => {} },
    },
  };
}

class FailingWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;

  constructor() {
    this.readyState = FailingWebSocket.CONNECTING;
    setImmediate(() => {
      this.onerror?.(new Error("connect refused"));
      this.onclose?.();
    });
  }

  close() {}
}

async function flushImmediates(count = 5) {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

async function testAutomaticReconnectFailureDoesNotLogError() {
  const consoleErrors = [];
  const sandbox = {
    console: {
      ...console,
      error: (...args) => consoleErrors.push(args.map(String).join(" ")),
      debug: () => {},
    },
    setImmediate,
    setTimeout: () => 1,
    clearTimeout: () => {},
    setInterval: () => 1,
    clearInterval: () => {},
    WebSocket: FailingWebSocket,
    chrome: makeChrome({ zodeToken: "token" }),
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  await flushImmediates();

  assert.deepEqual(consoleErrors, []);
}

(async () => {
  await testAutomaticReconnectFailureDoesNotLogError();
  console.log("background tests passed");
})();
