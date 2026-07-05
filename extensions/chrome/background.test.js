#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const backgroundPath = path.join(__dirname, "background.js");
const backgroundSource = fs.readFileSync(backgroundPath, "utf8");

function makeChrome(storage = {}) {
  const runtimeMessages = [];
  return {
    runtimeMessages,
    storage: {
      local: {
        get: async () => storage,
        set: async (values) => {
          Object.assign(storage, values);
        },
        remove: async (key) => {
          delete storage[key];
        },
      },
    },
    runtime: {
      onMessage: { addListener: (handler) => runtimeMessages.push(handler) },
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

async function flushImmediates(count = 5) {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

async function testBackgroundStartupDoesNotTouchWebSocket() {
  const consoleErrors = [];
  let websocketAttempts = 0;
  class CountingWebSocket {
    static CONNECTING = 0;
    static OPEN = 1;

    constructor() {
      websocketAttempts += 1;
    }
  }
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
    WebSocket: CountingWebSocket,
    chrome: makeChrome({ zodeToken: "token" }),
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  await flushImmediates();

  assert.equal(websocketAttempts, 0);
  assert.deepEqual(consoleErrors, []);
}

async function testStatusMessageReturnsConnectionState() {
  const chrome = makeChrome();
  const sandbox = {
    console,
    setImmediate,
    setTimeout: () => 1,
    clearTimeout: () => {},
    setInterval: () => 1,
    clearInterval: () => {},
    WebSocket: class {},
    chrome,
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  const handler = chrome.runtimeMessages[0];
  let response = null;
  const asyncResponse = handler({ type: "zode-status" }, {}, (value) => {
    response = value;
  });

  assert.equal(asyncResponse, true);
  await flushImmediates();
  assert.equal(response.ok, true);
  assert.equal(response.status.connected, false);
}

async function testStatusLoadsStoredPortAndReconnectCapability() {
  const chrome = makeChrome({ zodePort: 17657, zodeToken: "token" });
  const sandbox = {
    console,
    setImmediate,
    setTimeout: () => 1,
    clearTimeout: () => {},
    setInterval: () => 1,
    clearInterval: () => {},
    WebSocket: class {},
    chrome,
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  const handler = chrome.runtimeMessages[0];
  let response = null;
  const asyncResponse = handler({ type: "zode-status" }, {}, (value) => {
    response = value;
  });

  assert.equal(asyncResponse, true);
  await flushImmediates();
  assert.equal(response.ok, true);
  assert.equal(response.status.connected, false);
  assert.equal(response.status.port, 17657);
  assert.equal(response.status.canReconnect, true);
}

async function testReconnectMessageUsesStoredTokenAndPort() {
  const chrome = makeChrome({ zodePort: 17657, zodeToken: "token" });
  const sockets = [];
  const sent = [];

  class FakeWebSocket {
    static CONNECTING = 0;
    static OPEN = 1;

    constructor(url) {
      this.url = url;
      this.readyState = FakeWebSocket.CONNECTING;
      sockets.push(this);
      setImmediate(() => {
        this.readyState = FakeWebSocket.OPEN;
        this.onopen();
      });
    }

    send(text) {
      sent.push(JSON.parse(text));
      setImmediate(() => {
        this.onmessage({ data: JSON.stringify({ type: "ok" }) });
      });
    }

    close() {
      this.readyState = 3;
    }
  }

  const sandbox = {
    console,
    setImmediate,
    setTimeout: () => 1,
    clearTimeout: () => {},
    setInterval: () => 1,
    clearInterval: () => {},
    WebSocket: FakeWebSocket,
    chrome,
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  const handler = chrome.runtimeMessages[0];
  let response = null;
  const asyncResponse = handler({ type: "zode-reconnect" }, {}, (value) => {
    response = value;
  });

  assert.equal(asyncResponse, true);
  await flushImmediates();
  assert.equal(sockets[0].url, "ws://127.0.0.1:17657");
  assert.deepEqual(sent, [{ type: "auth", token: "token" }]);
  assert.equal(response.ok, true);
  assert.equal(response.status.connected, true);
  assert.equal(response.status.port, 17657);
}

(async () => {
  await testBackgroundStartupDoesNotTouchWebSocket();
  await testStatusMessageReturnsConnectionState();
  await testStatusLoadsStoredPortAndReconnectCapability();
  await testReconnectMessageUsesStoredTokenAndPort();
  console.log("background tests passed");
})();
