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
  for (const id of ["body", "port", "code", "go", "form-status"]) {
    elements[id] = makeElement(id);
  }
  elements.body.classList = elements.body.classList;

  const messages = [];
  const closeEvents = [];
  const sandbox = {
    URLSearchParams,
    queueMicrotask: (fn) => fn(),
    document: {
      body: elements.body,
      getElementById: (id) => {
        assert.ok(Object.hasOwn(elements, id), `unexpected pairing-page element: ${id}`);
        return elements[id];
      },
    },
    window: {
      location: { search },
      close: () => closeEvents.push("window.close"),
    },
    chrome: {
      runtime: {
        sendMessage: async (message) => {
          messages.push(message);
          return response(message);
        },
      },
      tabs: {
        getCurrent: async () => null,
        remove: async () => {},
      },
    },
  };

  return { sandbox, elements, messages, closeEvents };
}

async function flush() {
  for (let i = 0; i < 10; i += 1) {
    await Promise.resolve();
  }
}

async function testAlreadyConnectedStatusClosesPairingPage() {
  const { sandbox, messages, closeEvents } = makeSandbox({
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
  assert.deepEqual(closeEvents, ["window.close"]);
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

  assert.equal(elements["form-status"].textContent, "Not connected");
}

async function testAutoConnectFromQueryPairsWithoutStatusRace() {
  const { sandbox, messages, closeEvents } = makeSandbox({
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
  assert.deepEqual(closeEvents, ["window.close"]);
}

async function testDisconnectedStatusSilentlyReconnectsWhenTokenAvailable() {
  const { sandbox, messages, closeEvents } = makeSandbox({
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
  assert.equal(messages[1].port, 17657);
  assert.deepEqual(closeEvents, ["window.close"]);
}

function testMarkupAndScriptRemainPairingOnly() {
  assert.match(popupHtmlSource, /id="port"/i);
  assert.match(popupHtmlSource, /id="code"/i);
  assert.match(popupHtmlSource, /id="go"/i);
  assert.match(popupHtmlSource, /id="form-status"/i);
  assert.doesNotMatch(
    popupHtmlSource,
    /connected-view|status-pill|status-detail|Bridge settings|id="version"/i,
  );
  assert.doesNotMatch(popupSource, /renderConnected|statusText|statusDetail|versionText/);
  assert.doesNotMatch(popupSource, /zode-task-call|task\/|model\/|permission\/|attachment\//);
}

function testBrandMarkUsesExtensionIcon() {
  assert.match(popupHtmlSource, /<img[^>]+class="mark"[^>]+src="icons\/zode-48\.png"/i);
}

(async () => {
  testMarkupAndScriptRemainPairingOnly();
  testBrandMarkUsesExtensionIcon();
  await testAlreadyConnectedStatusClosesPairingPage();
  await testDisconnectedStatusShowsConnectView();
  await testAutoConnectFromQueryPairsWithoutStatusRace();
  await testDisconnectedStatusSilentlyReconnectsWhenTokenAvailable();
  console.log("popup tests passed");
})();
