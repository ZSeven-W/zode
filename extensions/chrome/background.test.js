#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const backgroundPath = path.join(__dirname, "background.js");
const backgroundSource = fs.readFileSync(backgroundPath, "utf8");

function makeChrome(storage = {}) {
  const runtimeMessages = [];
  const calls = { created: [], updated: [], attached: [], detached: [], commands: [], queries: [] };
  const listeners = { tabRemoved: [], debuggerDetach: [], webNavCommitted: [] };
  const tabsById = new Map();
  let nextTabId = 100;

  const api = {
    runtimeMessages,
    calls,
    tabsById,
    addTab({ id = nextTabId++, url = "", active = false, windowId = 1 } = {}) {
      const tab = { id, url, title: "", active, windowId };
      tabsById.set(id, tab);
      if (active) {
        api.setActive(id);
      }
      return tab;
    },
    setActive(id) {
      for (const tab of tabsById.values()) {
        tab.active = tab.id === id;
      }
    },
    fireTabRemoved(id) {
      tabsById.delete(id);
      listeners.tabRemoved.forEach((listener) => listener(id));
    },
    fireDebuggerDetach(source, reason) {
      listeners.debuggerDetach.forEach((listener) => listener(source, reason));
    },
    fireNavCommitted(details) {
      listeners.webNavCommitted.forEach((listener) => listener(details));
    },
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
      query: async (filter = {}) => {
        calls.queries.push(filter);
        let list = [...tabsById.values()];
        if (filter.active) {
          list = list.filter((tab) => tab.active);
        }
        if (filter.windowId != null) {
          list = list.filter((tab) => tab.windowId === filter.windowId);
        }
        return list;
      },
      get: async (id) => {
        const tab = tabsById.get(id);
        if (!tab) {
          throw new Error(`no tab ${id}`);
        }
        return tab;
      },
      create: async (opts = {}) => {
        calls.created.push(opts);
        return api.addTab({ url: opts.url || "", active: opts.active !== false });
      },
      remove: async (id) => {
        tabsById.delete(id);
      },
      update: async (id, props = {}) => {
        calls.updated.push([id, props]);
        if (props.active) {
          api.setActive(id);
        }
        return tabsById.get(id);
      },
      group: async () => 1,
      onRemoved: { addListener: (listener) => listeners.tabRemoved.push(listener) },
    },
    tabGroups: {
      query: async () => [],
      update: async () => {},
      onRemoved: { addListener: () => {} },
    },
    debugger: {
      attach: async (target) => {
        calls.attached.push(target.tabId);
      },
      detach: async (target) => {
        calls.detached.push(target.tabId);
      },
      sendCommand: async (target, method, params) => {
        calls.commands.push({ tabId: target.tabId, method, params });
        return {};
      },
      onDetach: { addListener: (listener) => listeners.debuggerDetach.push(listener) },
      onEvent: { addListener: () => {} },
    },
    webNavigation: {
      onCommitted: { addListener: (listener) => listeners.webNavCommitted.push(listener) },
    },
  };
  return api;
}

async function flushImmediates(count = 5) {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function makeRpcHarness(storage = { zodePort: 17657, zodeToken: "token" }) {
  const chrome = makeChrome(storage);
  const frames = [];

  class RpcWebSocket {
    static CONNECTING = 0;
    static OPEN = 1;

    constructor(url) {
      this.url = url;
      this.readyState = RpcWebSocket.CONNECTING;
      RpcWebSocket.last = this;
      setImmediate(() => {
        this.readyState = RpcWebSocket.OPEN;
        this.onopen();
      });
    }

    send(text) {
      const frame = JSON.parse(text);
      frames.push(frame);
      if (frame.type === "auth") {
        setImmediate(() => {
          this.onmessage({ data: JSON.stringify({ type: "ok" }) });
        });
      }
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
    WebSocket: RpcWebSocket,
    chrome,
  };
  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });

  return {
    chrome,
    frames,
    async connect() {
      const handler = chrome.runtimeMessages[0];
      handler({ type: "zode-reconnect" }, {}, () => {});
      await flushImmediates();
    },
    async rpc(frame) {
      RpcWebSocket.last.onmessage({ data: JSON.stringify(frame) });
      await flushImmediates(10);
      const reply = frames.filter((f) => f.id === frame.id).at(-1);
      assert.notEqual(reply, undefined, `no reply frame for rpc id ${frame.id}`);
      return reply;
    },
  };
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

async function testCdpActionCreatesAndReusesControlledTab() {
  const h = makeRpcHarness();
  h.chrome.addTab({ id: 5, url: "https://human.example/", active: true });
  await h.connect();

  const first = await h.rpc({
    id: 10,
    kind: "cdp",
    method: "Runtime.evaluate",
    params: { expression: "1" },
  });
  assert.equal(first.error, undefined);
  // acquired a fresh background tab, not the human's active tab
  assert.equal(h.chrome.calls.created.length, 1);
  assert.equal(h.chrome.calls.created[0].active, false);
  const zodeTabId = h.chrome.calls.attached[0];
  assert.notEqual(zodeTabId, 5);
  assert.equal(h.chrome.tabsById.get(5).active, true);

  const second = await h.rpc({
    id: 11,
    kind: "cdp",
    method: "Runtime.evaluate",
    params: { expression: "2" },
  });
  assert.equal(second.error, undefined);
  // reused: no second create, no re-attach
  assert.equal(h.chrome.calls.created.length, 1);
  assert.equal(h.chrome.calls.attached.length, 1);
  const evals = h.chrome.calls.commands.filter((c) => c.method === "Runtime.evaluate");
  assert.deepEqual(
    evals.map((c) => c.tabId),
    [zodeTabId, zodeTabId],
  );
}

(async () => {
  await testBackgroundStartupDoesNotTouchWebSocket();
  await testStatusMessageReturnsConnectionState();
  await testStatusLoadsStoredPortAndReconnectCapability();
  await testReconnectMessageUsesStoredTokenAndPort();
  await testCdpActionCreatesAndReusesControlledTab();
  console.log("background tests passed");
})();
