#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const popupPath = path.join(__dirname, "popup.js");
const popupSource = fs.readFileSync(popupPath, "utf8");
const popupHtmlPath = path.join(__dirname, "popup.html");
const popupHtmlSource = fs.readFileSync(popupHtmlPath, "utf8");

function makeElement(id) {
  return {
    id,
    value: "",
    textContent: "",
    hidden: false,
    disabled: false,
    listeners: {},
    classList: {
      values: new Set(),
      add(value) {
        this.values.add(value);
      },
      remove(value) {
        this.values.delete(value);
      },
      contains(value) {
        return this.values.has(value);
      },
    },
    addEventListener(event, handler) {
      this.listeners[event] = handler;
    },
  };
}

function makeSandbox({ search = "", response }) {
  const elements = {};
  for (const id of [
    "body",
    "connected-view",
    "connect-view",
    "status-pill",
    "status-text",
    "status-detail",
    "version",
    "port",
    "code",
    "go",
    "form-status",
  ]) {
    elements[id] = makeElement(id);
  }
  elements.body.classList = elements.body.classList;

  const messages = [];
  const sandbox = {
    URLSearchParams,
    queueMicrotask: (fn) => fn(),
    document: {
      body: elements.body,
      getElementById: (id) => elements[id],
    },
    window: {
      location: { search },
      close: () => {},
    },
    chrome: {
      runtime: {
        sendMessage: async (message) => {
          messages.push(message);
          return response(message);
        },
        getManifest: () => ({ version: "0.1.0" }),
      },
      tabs: {
        getCurrent: async () => null,
        remove: async () => {},
      },
    },
  };

  return { sandbox, elements, messages };
}

async function flush() {
  for (let i = 0; i < 10; i += 1) {
    await Promise.resolve();
  }
}

async function testConnectedStatusShowsConnectedView() {
  const { sandbox, elements, messages } = makeSandbox({
    response: async (message) => {
      if (message.type === "zode-status") {
        return { ok: true, status: { connected: true, port: 17657 } };
      }
      throw new Error(`unexpected message ${message.type}`);
    },
  });

  vm.runInNewContext(popupSource, sandbox, { filename: popupPath });
  await flush();

  assert.deepEqual(messages.map((message) => message.type), ["zode-status"]);
  assert.equal(elements["connected-view"].hidden, false);
  assert.equal(elements["connect-view"].hidden, true);
  assert.equal(elements["status-text"].textContent, "Connected");
  assert.equal(elements["status-detail"].textContent, "Port 17657");
  assert.equal(elements.version.textContent, "Version v0.1.0");
}

async function testDisconnectedStatusShowsConnectView() {
  const { sandbox, elements } = makeSandbox({
    response: async (message) => {
      if (message.type === "zode-status") {
        return { ok: true, status: { connected: false, port: null } };
      }
      throw new Error(`unexpected message ${message.type}`);
    },
  });

  vm.runInNewContext(popupSource, sandbox, { filename: popupPath });
  await flush();

  assert.equal(elements["connected-view"].hidden, true);
  assert.equal(elements["connect-view"].hidden, false);
  assert.equal(elements["form-status"].textContent, "Not connected");
}

async function testAutoConnectFromQueryPairsWithoutStatusRace() {
  const { sandbox, elements, messages } = makeSandbox({
    search: "?port=17657&code=127179&connect=1",
    response: async (message) => {
      if (message.type === "zode-pair") {
        return { ok: true, status: { connected: true, port: message.port } };
      }
      throw new Error(`unexpected message ${message.type}`);
    },
  });

  vm.runInNewContext(popupSource, sandbox, { filename: popupPath });
  await flush();

  assert.deepEqual(messages.map((message) => message.type), ["zode-pair"]);
  assert.equal(messages[0].port, 17657);
  assert.equal(messages[0].code, "127179");
  assert.equal(elements["connected-view"].hidden, false);
  assert.equal(elements["connect-view"].hidden, true);
  assert.equal(elements["status-detail"].textContent, "Port 17657");
}

async function testDisconnectedStatusSilentlyReconnectsWhenTokenAvailable() {
  const { sandbox, elements, messages } = makeSandbox({
    response: async (message) => {
      if (message.type === "zode-status") {
        return {
          ok: true,
          status: { connected: false, port: 17657, canReconnect: true },
        };
      }
      if (message.type === "zode-reconnect") {
        return { ok: true, status: { connected: true, port: 17657 } };
      }
      throw new Error(`unexpected message ${message.type}`);
    },
  });

  vm.runInNewContext(popupSource, sandbox, { filename: popupPath });
  await flush();

  assert.deepEqual(messages.map((message) => message.type), ["zode-status", "zode-reconnect"]);
  assert.equal(elements["connected-view"].hidden, false);
  assert.equal(elements["connect-view"].hidden, true);
  assert.equal(elements["status-detail"].textContent, "Port 17657");
}

function testHiddenPanelsAreCssHidden() {
  assert.match(popupHtmlSource, /\[hidden\]\s*\{[^}]*display:\s*none\s*!important/i);
}

(async () => {
  testHiddenPanelsAreCssHidden();
  await testConnectedStatusShowsConnectedView();
  await testDisconnectedStatusShowsConnectView();
  await testAutoConnectFromQueryPairsWithoutStatusRace();
  await testDisconnectedStatusSilentlyReconnectsWhenTokenAvailable();
  console.log("popup tests passed");
})();
