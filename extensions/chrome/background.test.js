#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const backgroundPath = path.join(__dirname, "background.js");
const backgroundSource = fs.readFileSync(backgroundPath, "utf8");

function makeChrome(storage = {}) {
  const runtimeMessages = [];
  const calls = {
    created: [],
    updated: [],
    attached: [],
    detached: [],
    commands: [],
    queries: [],
    offscreenCreated: [],
    iconPaths: [],
    runtimeSent: [],
    sidePanelBehaviors: [],
    sidePanelOpens: [],
  };
  const listeners = {
    tabRemoved: [],
    debuggerDetach: [],
    debuggerEvent: [],
    webNavCommitted: [],
    downloadCreated: [],
    downloadChanged: [],
    command: [],
  };
  const tabsById = new Map();
  let nextTabId = 100;

  const api = {
    runtimeMessages,
    offscreenContexts: [],
    runtimeSendError: null,
    storageGetHandler: null,
    storageSetHandler: null,
    commandHandler: null,
    sidePanelBehaviorError: null,
    sidePanelBehaviorSyncError: null,
    storageData: storage,
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
    fireDebuggerEvent(source, method, params) {
      listeners.debuggerEvent.forEach((listener) => listener(source, method, params));
    },
    fireNavCommitted(details) {
      listeners.webNavCommitted.forEach((listener) => listener(details));
    },
    fireDownloadCreated(item) {
      listeners.downloadCreated.forEach((listener) => listener(item));
    },
    fireDownloadChanged(delta) {
      listeners.downloadChanged.forEach((listener) => listener(delta));
    },
    fireCommand(command, tab) {
      listeners.command.forEach((listener) => listener(command, tab));
    },
    lastFocusedWindowId: 1,
    storage: {
      local: {
        get: async (keys) => {
          if (api.storageGetHandler) {
            await api.storageGetHandler(keys);
          }
          return storage;
        },
        set: async (values) => {
          if (api.storageSetHandler) {
            await api.storageSetHandler(values);
          }
          Object.assign(storage, values);
        },
        remove: async (key) => {
          delete storage[key];
        },
      },
    },
    runtime: {
      onMessage: { addListener: (handler) => runtimeMessages.push(handler) },
      getContexts: async () => api.offscreenContexts,
      sendMessage: async (message) => {
        calls.runtimeSent.push(JSON.parse(JSON.stringify(message)));
        if (api.runtimeSendError) {
          throw api.runtimeSendError;
        }
      },
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
        if (api.commandHandler) {
          return await api.commandHandler(target, method, params);
        }
        return {};
      },
      onDetach: { addListener: (listener) => listeners.debuggerDetach.push(listener) },
      onEvent: { addListener: (listener) => listeners.debuggerEvent.push(listener) },
    },
    offscreen: {
      createDocument: async (opts) => {
        calls.offscreenCreated.push(JSON.parse(JSON.stringify(opts)));
      },
    },
    action: {
      setIcon: async (opts) => {
        calls.iconPaths.push(JSON.parse(JSON.stringify(opts.path)));
      },
    },
    sidePanel: {
      setPanelBehavior: (behavior) => {
        calls.sidePanelBehaviors.push(JSON.parse(JSON.stringify(behavior)));
        if (api.sidePanelBehaviorSyncError) {
          throw api.sidePanelBehaviorSyncError;
        }
        if (api.sidePanelBehaviorError) {
          return Promise.reject(api.sidePanelBehaviorError);
        }
        return Promise.resolve();
      },
      open: (options) => {
        calls.sidePanelOpens.push(JSON.parse(JSON.stringify(options)));
        return Promise.resolve();
      },
    },
    commands: {
      onCommand: { addListener: (listener) => listeners.command.push(listener) },
    },
    windows: {
      getLastFocused: () => Promise.resolve({ id: api.lastFocusedWindowId }),
    },
    webNavigation: {
      onCommitted: { addListener: (listener) => listeners.webNavCommitted.push(listener) },
    },
    downloads: {
      onCreated: { addListener: (listener) => listeners.downloadCreated.push(listener) },
      onChanged: { addListener: (listener) => listeners.downloadChanged.push(listener) },
    },
  };
  return api;
}

async function flushImmediates(count = 5) {
  for (let i = 0; i < count; i += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function makeRpcHarness(storage = { zodePort: 17657, zodeToken: "token" }) {
  const chrome = makeChrome(storage);
  const frames = [];
  const sockets = [];
  const timers = new Map();
  const clearedTimers = [];
  let nextTimerId = 1;
  let nextSocketAutoAuth = true;
  let serverAuthToken = storage.zodeToken;

  const fakeSetTimeout = (callback, delay) => {
    const id = nextTimerId++;
    timers.set(id, { callback, delay });
    return id;
  };
  const fakeClearTimeout = (id) => {
    clearedTimers.push(id);
    timers.delete(id);
  };

  class RpcWebSocket {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSED = 3;

    constructor(url) {
      this.url = url;
      this.readyState = RpcWebSocket.CONNECTING;
      this.sent = [];
      this.sendError = null;
      this.onSend = null;
      this.autoAuth = nextSocketAutoAuth;
      nextSocketAutoAuth = true;
      sockets.push(this);
      RpcWebSocket.last = this;
      setImmediate(() => {
        if (this.readyState !== RpcWebSocket.CONNECTING) {
          return;
        }
        this.readyState = RpcWebSocket.OPEN;
        this.onopen?.();
      });
    }

    send(text) {
      if (this.sendError) {
        throw this.sendError;
      }
      const frame = JSON.parse(text);
      frames.push(frame);
      this.sent.push(frame);
      if (this.onSend) {
        this.onSend(frame);
      }
      if (frame.type === "auth" && this.autoAuth) {
        const response =
          frame.token === serverAuthToken
            ? { type: "ok", taskProtocol: 1 }
            : { type: "rejected", reason: "invalid token" };
        setImmediate(() => {
          this.onmessage?.({ data: JSON.stringify(response) });
        });
      }
    }

    close() {
      this.readyState = RpcWebSocket.CLOSED;
    }
  }

  const sandbox = {
    console: { ...console, debug: () => {} },
    setImmediate,
    setTimeout: fakeSetTimeout,
    clearTimeout: fakeClearTimeout,
    setInterval: () => 1,
    clearInterval: () => {},
    WebSocket: RpcWebSocket,
    chrome,
  };
  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });

  return {
    chrome,
    describeElementSource: () => vm.runInNewContext("DESCRIBE_ELEMENT_FN", sandbox),
    frames,
    sockets,
    timers,
    clearedTimers,
    async connect() {
      const handler = chrome.runtimeMessages[0];
      handler({ type: "zode-reconnect" }, {}, () => {});
      await flushImmediates();
    },
    setNextSocketAutoAuth(enabled) {
      nextSocketAutoAuth = enabled;
    },
    setServerAuthToken(token) {
      serverAuthToken = token;
    },
    runtimeCall(message) {
      const handler = chrome.runtimeMessages[0];
      const responses = [];
      let resolveFirst;
      const promise = new Promise((resolve) => {
        resolveFirst = resolve;
      });
      let asyncResponse;
      try {
        asyncResponse = handler(message, {}, (value) => {
          responses.push(plain(value));
          resolveFirst(value);
        });
        assert.equal(asyncResponse, true, `${message.type} must keep sendResponse alive`);
      } catch (error) {
        resolveFirst(Promise.reject(error));
      }
      return { promise, responses, asyncResponse };
    },
    runtime(message) {
      return this.runtimeCall(message).promise;
    },
    taskRequest(method, params) {
      const message = { type: "zode-task-request", method };
      if (arguments.length >= 2) {
        message.params = params;
      }
      return this.runtime(message);
    },
    async deliver(frame, socket = RpcWebSocket.last) {
      socket.onmessage?.({ data: JSON.stringify(frame) });
      await flushImmediates();
    },
    naturalClose(socket = RpcWebSocket.last) {
      const handler = socket.onclose;
      socket.readyState = RpcWebSocket.CLOSED;
      handler?.();
      return handler;
    },
    runTimer(id) {
      const timer = timers.get(id);
      assert.notEqual(timer, undefined, `missing timer ${id}`);
      timers.delete(id);
      timer.callback();
    },
    async rpc(frame) {
      RpcWebSocket.last.onmessage?.({ data: JSON.stringify(frame) });
      await flushImmediates(10);
      const reply = frames.filter((f) => f.id === frame.id).at(-1);
      assert.notEqual(reply, undefined, `no reply frame for rpc id ${frame.id}`);
      return reply;
    },
  };
}

async function testTaskRequestResponseUsesDedicatedWireId() {
  const h = makeRpcHarness();
  await h.connect();

  const responsePromise = h.taskRequest("snapshot/read", {});
  await flushImmediates();
  const request = h.frames.find((frame) => frame.channel === "tasks");
  assert.deepEqual(request, {
    channel: "tasks",
    kind: "request",
    id: "ext-1",
    method: "snapshot/read",
    params: {},
  });

  await h.deliver({
    channel: "tasks",
    kind: "response",
    id: request.id,
    result: { tasks: [] },
  });
  assert.deepEqual(plain(await responsePromise), { ok: true, result: { tasks: [] } });
  assert.equal(h.timers.size, 0);
}

async function testAuthenticatedAndReplacementSocketsBroadcastTaskLifecycle() {
  const h = makeRpcHarness();
  await h.connect();

  let lifecycle = h.chrome.calls.runtimeSent.filter((message) =>
    ["zode-task-connected", "zode-task-disconnected"].includes(message.type),
  );
  assert.equal(lifecycle.length, 1);
  assert.equal(lifecycle[0].type, "zode-task-connected");
  assert.equal(lifecycle[0].protocolVersion, 1);
  assert.equal(typeof lifecycle[0].connectionId, "string");
  assert.notEqual(lifecycle[0].connectionId, "");
  const firstConnectionId = lifecycle[0].connectionId;

  assert.equal((await h.runtime({ type: "zode-reconnect" })).ok, true);
  lifecycle = h.chrome.calls.runtimeSent.filter((message) =>
    ["zode-task-connected", "zode-task-disconnected"].includes(message.type),
  );
  assert.deepEqual(
    lifecycle.map((message) => message.type),
    ["zode-task-connected", "zode-task-disconnected", "zode-task-connected"],
  );
  assert.notEqual(lifecycle[2].connectionId, firstConnectionId);

  h.naturalClose();
  await flushImmediates();
  lifecycle = h.chrome.calls.runtimeSent.filter((message) =>
    ["zode-task-connected", "zode-task-disconnected"].includes(message.type),
  );
  assert.equal(lifecycle.at(-1).type, "zode-task-disconnected");
}

async function testUnsupportedTaskProtocolKeepsLegacyRpcAvailable() {
  const h = makeRpcHarness();
  h.setNextSocketAutoAuth(false);
  const reconnect = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates();
  await h.deliver({ type: "ok" });
  assert.equal((await reconnect.promise).ok, true);

  const status = await h.runtime({ type: "zode-status" });
  assert.equal(status.status.connected, true);
  assert.equal(status.status.taskClientSupported, false);
  assert.equal(status.status.taskConnectionId, null);
  assert.deepEqual(h.chrome.calls.runtimeSent.at(-1), {
    type: "zode-task-unsupported",
    message: "Current zode version does not support the task client",
  });

  const task = h.runtimeCall({
    type: "zode-task-request",
    method: "snapshot/read",
    params: {},
  });
  await flushImmediates();
  assert.equal(task.responses.length, 1, "unsupported task request did not fail immediately");
  assert.deepEqual(task.responses[0], {
    ok: false,
    error: "Current zode version does not support the task client",
    code: "unsupported_task_client",
  });
  assert.equal(h.timers.size, 0);
  assert.equal(h.frames.some((frame) => frame.channel === "tasks"), false);

  const rpc = await h.rpc({ id: 880, kind: "ext", method: "tabs.list", params: {} });
  assert.equal(rpc.error, undefined, "unsupported task protocol broke legacy browser RPC");
}

async function testTaskRequestRegistersPendingBeforeSynchronousResponse() {
  const h = makeRpcHarness();
  await h.connect();
  const socket = h.sockets.at(-1);
  socket.onSend = (frame) => {
    if (frame.channel === "tasks") {
      socket.onmessage?.({
        data: JSON.stringify({
          channel: "tasks",
          kind: "response",
          id: frame.id,
          result: { immediate: true },
        }),
      });
    }
  };

  assert.deepEqual(plain(await h.taskRequest("snapshot/read")), {
    ok: true,
    result: { immediate: true },
  });
  assert.equal(h.timers.size, 0);
  assert.deepEqual(socket.sent.at(-1).params, {});
}

async function testTaskErrorUnknownAndLateIdsSettleOnlyTheirPendingRequest() {
  const h = makeRpcHarness();
  await h.connect();
  const responsePromise = h.taskRequest("turn/start", { input: "inspect" });
  await flushImmediates();
  const request = h.frames.find((frame) => frame.channel === "tasks");
  let settled = false;
  responsePromise.finally(() => {
    settled = true;
  });

  const beforeUnknown = h.frames.length;
  await h.deliver({
    channel: "tasks",
    kind: "response",
    id: "ext-unknown",
    result: { ignored: true },
  });
  assert.equal(settled, false);
  assert.equal(h.frames.length, beforeUnknown, "unknown task response reached handleRpc");

  await h.deliver({
    channel: "tasks",
    kind: "error",
    id: request.id,
    code: "task_busy",
    message: "task is busy",
  });
  assert.deepEqual(plain(await responsePromise), {
    ok: false,
    error: "task is busy",
    code: "task_busy",
  });
  assert.equal(h.timers.size, 0);

  await h.deliver({
    channel: "tasks",
    kind: "response",
    id: request.id,
    result: { tooLate: true },
  });
  assert.equal(h.timers.size, 0);
}

async function testInvalidTaskMethodFailsBeforeTimerOrFrame() {
  const h = makeRpcHarness();
  await h.connect();
  const frameCount = h.frames.length;

  for (const method of ["", "   ", null]) {
    const call = h.runtimeCall({ type: "zode-task-request", method });
    await flushImmediates();
    assert.equal(call.responses.length, 1, `invalid method ${String(method)} did not settle`);
    assert.equal(call.responses[0].ok, false);
    assert.match(call.responses[0].error, /method/i);
  }

  assert.equal(h.frames.length, frameCount);
  assert.equal(h.timers.size, 0);
}

async function testTaskPendingCapRejectsBeforeTimerOrFrame() {
  const h = makeRpcHarness();
  await h.connect();
  const pending = Array.from({ length: 500 }, (_, index) =>
    h.runtimeCall({
      type: "zode-task-request",
      method: "snapshot/read",
      params: { index },
    }),
  );
  assert.equal(h.frames.filter((frame) => frame.channel === "tasks").length, 500);
  assert.equal(h.timers.size, 500);

  const overflow = h.runtimeCall({
    type: "zode-task-request",
    method: "snapshot/read",
    params: { overflow: true },
  });
  await flushImmediates();
  assert.equal(overflow.responses.length, 1, "overflow request did not settle immediately");
  assert.equal(overflow.responses[0].ok, false);
  assert.match(overflow.responses[0].error, /pending|limit|capacity/i);
  assert.equal(h.frames.filter((frame) => frame.channel === "tasks").length, 500);
  assert.equal(h.timers.size, 500);

  h.naturalClose();
  await Promise.all(pending.map((call) => call.promise));
  assert.equal(h.timers.size, 0);
}

async function testTaskEventBroadcastsBeforeRpcAndRuntimeRejectionIsBestEffort() {
  const h = makeRpcHarness();
  await h.connect();
  const before = h.frames.length;
  const eventBroadcastsBefore = h.chrome.calls.runtimeSent.filter(
    (message) => message.type === "zode-task-event",
  ).length;
  h.chrome.runtimeSendError = new Error("no side panel listener");

  await h.deliver({
    channel: "tasks",
    kind: "event",
    event: "message/delta",
    params: { taskId: "s1", delta: "hi" },
  });

  assert.deepEqual(h.chrome.calls.runtimeSent.at(-1), {
    type: "zode-task-event",
    event: "message/delta",
    params: { taskId: "s1", delta: "hi" },
  });
  assert.equal(
    h.chrome.calls.runtimeSent.filter((message) => message.type === "zode-task-event").length,
    eventBroadcastsBefore + 1,
    "one task event frame must produce exactly one runtime broadcast",
  );
  assert.equal(h.frames.length, before, "task event fell through to handleRpc");

  const reply = await h.rpc({ id: 801, kind: "ext", method: "tabs.list", params: {} });
  assert.equal(reply.error, undefined, "runtime broadcast rejection broke old RPC handling");
}

async function testNaturalCloseRejectsAllTaskPendingAndBroadcastsOnce() {
  const h = makeRpcHarness();
  await h.connect();
  const first = h.taskRequest("snapshot/read", {});
  const second = h.taskRequest("task/select", { taskId: "s1" });
  await flushImmediates();
  const timerIds = [...h.timers.keys()];
  assert.equal(timerIds.length, 2);

  const staleCloseHandler = h.naturalClose();
  assert.match((await first).error, /connection closed/i);
  assert.match((await second).error, /connection closed/i);
  assert.equal(h.timers.size, 0);
  assert.deepEqual(h.clearedTimers.slice(-2).sort(), timerIds.sort());
  assert.equal(
    h.chrome.calls.runtimeSent.filter((message) => message.type === "zode-task-disconnected")
      .length,
    1,
  );

  staleCloseHandler?.();
  await flushImmediates();
  assert.equal(
    h.chrome.calls.runtimeSent.filter((message) => message.type === "zode-task-disconnected")
      .length,
    1,
    "same socket broadcast disconnect twice",
  );
}

async function testReplacementRejectsPendingAndStaleCloseCannotClearNewSocket() {
  const h = makeRpcHarness();
  await h.connect();
  const oldSocket = h.sockets.at(-1);
  const staleCloseHandler = oldSocket.onclose;
  const pending = h.taskRequest("snapshot/read", {});
  await flushImmediates();
  const oldTimerId = [...h.timers.keys()][0];

  const reconnect = h.runtime({ type: "zode-reconnect" });
  await flushImmediates(10);
  assert.equal((await reconnect).ok, true);
  assert.match((await pending).error, /replaced/i);
  assert.ok(h.clearedTimers.includes(oldTimerId));
  assert.equal(h.timers.size, 0);
  assert.equal(h.sockets.length, 2);

  staleCloseHandler?.();
  await flushImmediates();
  assert.equal(
    h.chrome.calls.runtimeSent.filter((message) => message.type === "zode-task-disconnected")
      .length,
    1,
  );

  const current = h.taskRequest("snapshot/read", {});
  await flushImmediates();
  const newSocket = h.sockets.at(-1);
  const request = newSocket.sent.find((frame) => frame.channel === "tasks");
  assert.notEqual(request, undefined, "stale close cleared replacement socket");
  await h.deliver(
    { channel: "tasks", kind: "response", id: request.id, result: { current: true } },
    newSocket,
  );
  assert.deepEqual(plain(await current), { ok: true, result: { current: true } });
}

async function testStaleRpcCompletionNeverWritesReplacementSocket() {
  const h = makeRpcHarness();
  await h.connect();
  await h.rpc({
    id: 900,
    kind: "cdp",
    method: "Runtime.evaluate",
    params: { expression: "0" },
  });

  const oldSocket = h.sockets.at(-1);
  const commandGate = deferred();
  h.chrome.commandHandler = async (_target, method) => {
    if (method === "Runtime.evaluate") {
      return await commandGate.promise;
    }
    return {};
  };
  oldSocket.onmessage?.({
    data: JSON.stringify({
      id: 901,
      kind: "cdp",
      method: "Runtime.evaluate",
      params: { expression: "1" },
    }),
  });
  await flushImmediates();

  assert.equal((await h.runtime({ type: "zode-reconnect" })).ok, true);
  const newSocket = h.sockets.at(-1);
  commandGate.resolve({ value: "old" });
  await flushImmediates(10);
  assert.equal(
    newSocket.sent.some((frame) => frame.id === 901),
    false,
    "stale RPC response leaked into the replacement socket",
  );

  h.chrome.commandHandler = null;
  const current = await h.rpc({
    id: 902,
    kind: "cdp",
    method: "Runtime.evaluate",
    params: { expression: "2" },
  });
  assert.equal(current.error, undefined);
}

async function testInFlightGuardCoversPreSocketAsyncWork() {
  for (const delayedOperation of ["getPort", "getToken", "setPort"]) {
    const h = makeRpcHarness();
    const gate = deferred();
    let matchingCalls = 0;
    if (delayedOperation === "getPort" || delayedOperation === "getToken") {
      const delayedKey = delayedOperation === "getPort" ? "zodePort" : "zodeToken";
      h.chrome.storageGetHandler = async (keys) => {
        if (keys === delayedKey && matchingCalls++ === 0) {
          await gate.promise;
        }
      };
    } else {
      h.chrome.storageSetHandler = async (values) => {
        if (values.zodePort != null && matchingCalls++ === 0) {
          await gate.promise;
        }
      };
    }

    const first = h.runtimeCall({ type: "zode-reconnect" });
    await flushImmediates();
    assert.equal(h.sockets.length, 0, `${delayedOperation} did not pause before socket creation`);

    const overlapping = h.runtimeCall({ type: "zode-reconnect" });
    await flushImmediates(10);
    assert.equal(overlapping.responses.length, 1, `${delayedOperation} overlap did not settle`);
    assert.equal(overlapping.responses[0].ok, false);
    assert.match(overlapping.responses[0].error, /already|busy|in progress/i);
    assert.equal(first.responses.length, 0);
    assert.equal(h.sockets.length, 0, "overlap bypassed the pre-socket connection guard");

    gate.resolve();
    await flushImmediates(15);
    assert.equal((await first.promise).ok, true);
    assert.equal(first.responses.length, 1);
    assert.equal(overlapping.responses.length, 1);
    assert.equal(h.sockets.length, 1);
  }
}

async function testOverlappingUnauthenticatedConnectReturnsBusy() {
  const h = makeRpcHarness();
  h.setNextSocketAutoAuth(false);
  const first = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates();
  const firstSocket = h.sockets.at(-1);

  const overlapping = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates(10);
  assert.equal(overlapping.responses.length, 1);
  assert.equal(overlapping.responses[0].ok, false);
  assert.match(overlapping.responses[0].error, /already|busy|in progress/i);
  assert.equal(first.responses.length, 0);
  assert.equal(h.sockets.length, 1);

  await h.deliver({ type: "ok" }, firstSocket);
  assert.equal((await first.promise).ok, true);
  assert.equal(first.responses.length, 1);
  assert.equal(overlapping.responses.length, 1);
  const status = await h.runtime({ type: "zode-status" });
  assert.equal(status.status.connected, true);
  assert.equal(firstSocket.readyState, 1);
}

async function testPairingBlocksOldTokenReconnectUntilNewTokenIsPersisted() {
  const h = makeRpcHarness({ zodePort: 17657, zodeToken: "old-token" });
  const pairing = h.runtimeCall({ type: "zode-pair", port: 17657, code: "123456" });
  await flushImmediates();
  const pairSocket = h.sockets.at(-1);
  const tokenWriteGate = deferred();
  h.chrome.storageSetHandler = async (values) => {
    if (values.zodeToken === "new-token") {
      await tokenWriteGate.promise;
    }
  };

  pairSocket.onmessage?.({
    data: JSON.stringify({ type: "paired", token: "new-token" }),
  });
  await flushImmediates();
  h.setServerAuthToken("new-token");
  const overlapping = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates(10);
  assert.equal(overlapping.responses.length, 1, "overlapping reconnect did not settle");
  assert.equal(overlapping.responses[0].ok, false);
  assert.match(overlapping.responses[0].error, /already|busy|in progress/i);
  assert.equal(pairing.responses.length, 0);
  assert.equal(h.sockets.length, 1);
  assert.equal(h.frames.some((frame) => frame.type === "auth"), false);
  assert.equal(h.chrome.storageData.zodeToken, "old-token");

  tokenWriteGate.resolve();
  await flushImmediates(15);
  assert.equal((await pairing.promise).ok, true);
  assert.equal(pairing.responses.length, 1);
  assert.equal(h.chrome.storageData.zodeToken, "new-token");

  h.setServerAuthToken("new-token");
  const reconnect = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates(15);
  assert.equal((await reconnect.promise).ok, true);
  assert.equal(reconnect.responses.length, 1);
  assert.deepEqual(
    h.frames.filter((frame) => frame.type === "auth"),
    [{ type: "auth", token: "new-token" }],
  );
  assert.equal(h.chrome.storageData.zodeToken, "new-token");
}

async function testPairTokenWriteSurvivesCloseAndReconnectWaitsForIt() {
  const h = makeRpcHarness({ zodePort: 17657, zodeToken: "old-token" });
  const pairing = h.runtimeCall({ type: "zode-pair", port: 17657, code: "123456" });
  await flushImmediates();
  const pairSocket = h.sockets.at(-1);
  const tokenWriteGate = deferred();
  h.chrome.storageSetHandler = async (values) => {
    if (values.zodeToken === "new-token") {
      await tokenWriteGate.promise;
    }
  };
  h.setServerAuthToken("new-token");

  pairSocket.onmessage?.({
    data: JSON.stringify({ type: "paired", token: "new-token" }),
  });
  await flushImmediates();
  h.naturalClose(pairSocket);
  await flushImmediates(10);
  assert.equal(pairing.responses.length, 1);
  assert.equal(pairing.responses[0].ok, false);
  assert.match(pairing.responses[0].error, /connection closed/i);

  pairSocket.onmessage?.({
    data: JSON.stringify({ type: "paired", token: "stale-token" }),
  });
  await flushImmediates();
  const reconnect = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates(10);
  assert.equal(reconnect.responses.length, 0);
  assert.equal(h.sockets.length, 1, "reconnect created a socket before token mutations settled");
  assert.deepEqual(h.frames.filter((frame) => frame.type === "auth"), []);
  assert.equal(h.chrome.storageData.zodeToken, "old-token");

  tokenWriteGate.resolve();
  await flushImmediates(20);
  assert.equal((await reconnect.promise).ok, true);
  assert.equal(reconnect.responses.length, 1);
  assert.deepEqual(
    h.frames.filter((frame) => frame.type === "auth"),
    [{ type: "auth", token: "new-token" }],
  );
  assert.equal(h.chrome.storageData.zodeToken, "new-token");
}

async function testSilentAuthenticationTimesOutAndReleasesGuard() {
  const h = makeRpcHarness();
  h.setNextSocketAutoAuth(false);
  const silent = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates();
  const silentSocket = h.sockets.at(-1);
  const staleMessageHandler = silentSocket.onmessage;
  const authTimers = [...h.timers.entries()].filter(([, timer]) => timer.delay === 15_000);
  assert.equal(authTimers.length, 1, "authentication attempt has no bounded timeout");

  h.runTimer(authTimers[0][0]);
  await flushImmediates(10);
  assert.equal(silent.responses.length, 1);
  assert.equal(silent.responses[0].ok, false);
  assert.match(silent.responses[0].error, /authentication.*timed out/i);
  assert.equal(silentSocket.readyState, 3);

  staleMessageHandler?.({ data: JSON.stringify({ type: "ok" }) });
  await flushImmediates();
  assert.equal(silent.responses.length, 1, "late auth settled the timed-out response twice");

  h.setServerAuthToken("token");
  const recovered = h.runtimeCall({ type: "zode-reconnect" });
  await flushImmediates(15);
  assert.equal((await recovered.promise).ok, true);
  assert.equal(recovered.responses.length, 1);
  assert.equal(h.sockets.length, 2);
}

async function testTaskSendFailureClearsPendingAndTimer() {
  const h = makeRpcHarness();
  await h.connect();
  const socket = h.sockets.at(-1);
  socket.sendError = new Error("send failed");
  const clearedBefore = h.clearedTimers.length;

  const response = await h.taskRequest("snapshot/read", {});
  assert.deepEqual(plain(response), { ok: false, error: "send failed" });
  assert.equal(h.timers.size, 0);
  assert.equal(h.clearedTimers.length, clearedBefore + 1);
}

async function testTaskTimeoutClearsPendingAndIgnoresLateResponse() {
  const h = makeRpcHarness();
  await h.connect();
  const responsePromise = h.taskRequest("snapshot/read", {});
  await flushImmediates();
  const request = h.frames.find((frame) => frame.channel === "tasks");
  const [[timerId, timer]] = [...h.timers.entries()];
  assert.equal(timer.delay, 30_000);

  h.runTimer(timerId);
  assert.match((await responsePromise).error, /snapshot\/read.*timed out/i);
  assert.equal(h.timers.size, 0);
  await h.deliver({
    channel: "tasks",
    kind: "response",
    id: request.id,
    result: { late: true },
  });
  assert.equal(h.timers.size, 0);
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

async function evaluateSidePanelStartup(chrome) {
  let websocketAttempts = 0;
  const consoleErrors = [];
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
    chrome,
  };

  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  await flushImmediates();
  return { websocketAttempts, consoleErrors };
}

async function testOpenSidePanelCommandOpensPanelForFocusedWindow() {
  const chrome = makeChrome();
  await evaluateSidePanelStartup(chrome);

  // Command fired on a tab: open the panel in that tab's window.
  chrome.fireCommand("open-side-panel", { id: 5, windowId: 7 });
  await flushImmediates();
  assert.deepEqual(chrome.calls.sidePanelOpens, [{ windowId: 7 }]);

  // No tab context: fall back to the last focused window.
  chrome.lastFocusedWindowId = 3;
  chrome.fireCommand("open-side-panel", undefined);
  await flushImmediates();
  assert.deepEqual(chrome.calls.sidePanelOpens, [{ windowId: 7 }, { windowId: 3 }]);

  // Unknown commands are ignored.
  chrome.fireCommand("something-else", { id: 5, windowId: 7 });
  await flushImmediates();
  assert.equal(chrome.calls.sidePanelOpens.length, 2);
}

async function testStartupEnablesActionOpenedSidePanelExactlyOnce() {
  const chrome = makeChrome();
  const startup = await evaluateSidePanelStartup(chrome);

  assert.deepEqual(chrome.calls.sidePanelBehaviors, [{ openPanelOnActionClick: true }]);
  assert.equal(startup.websocketAttempts, 0);
  assert.deepEqual(startup.consoleErrors, []);
}

async function testSidePanelStartupIsBestEffortWhenApiIsUnavailableOrFails() {
  const cases = [
    {
      name: "missing API",
      configure(chrome) {
        delete chrome.sidePanel;
      },
      expectedCalls: 0,
    },
    {
      name: "rejected promise",
      configure(chrome) {
        chrome.sidePanelBehaviorError = new Error("side panel rejected");
      },
      expectedCalls: 1,
    },
    {
      name: "synchronous throw",
      configure(chrome) {
        chrome.sidePanelBehaviorSyncError = new Error("side panel threw");
      },
      expectedCalls: 1,
    },
  ];

  for (const testCase of cases) {
    const chrome = makeChrome();
    testCase.configure(chrome);
    const startup = await evaluateSidePanelStartup(chrome);

    assert.equal(
      chrome.calls.sidePanelBehaviors.length,
      testCase.expectedCalls,
      `${testCase.name} called setPanelBehavior unexpectedly`,
    );
    assert.equal(startup.websocketAttempts, 0, `${testCase.name} opened a WebSocket`);
    assert.deepEqual(startup.consoleErrors, [], `${testCase.name} broke startup`);
    assert.equal(chrome.runtimeMessages.length, 1, `${testCase.name} skipped runtime listener setup`);
    assert.equal(chrome.calls.offscreenCreated.length, 1, `${testCase.name} skipped theme startup`);
  }
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
        this.onmessage({ data: JSON.stringify({ type: "ok", taskProtocol: 1 }) });
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

async function testReconnectAutoStartsNativeHostWhenZodeIsOffline() {
  const chrome = makeChrome({ zodePort: 17657, zodeToken: "token" });
  const sockets = [];
  const nativeCalls = [];
  const nativeMessages = [];
  const nativeMessageListeners = [];
  const nativeDisconnectListeners = [];
  chrome.runtime.connectNative = (name) => {
    nativeCalls.push(name);
    return {
      onMessage: { addListener: (listener) => nativeMessageListeners.push(listener) },
      onDisconnect: { addListener: (listener) => nativeDisconnectListeners.push(listener) },
      postMessage(message) {
        nativeMessages.push(plain(message));
        setImmediate(() => nativeMessageListeners[0]({ ok: true, port: 17658 }));
      },
      disconnect() {
        nativeDisconnectListeners.forEach((listener) => listener());
      },
    };
  };

  class OfflineThenOnlineWebSocket {
    static CONNECTING = 0;
    static OPEN = 1;

    constructor(url) {
      this.url = url;
      this.readyState = OfflineThenOnlineWebSocket.CONNECTING;
      sockets.push(this);
      const attempt = sockets.length;
      setImmediate(() => {
        if (attempt === 1) {
          this.readyState = 3;
          this.onerror?.();
          return;
        }
        this.readyState = OfflineThenOnlineWebSocket.OPEN;
        this.onopen?.();
      });
    }

    send(text) {
      const message = JSON.parse(text);
      if (message.type === "auth") {
        setImmediate(() => {
          this.onmessage?.({ data: JSON.stringify({ type: "ok", taskProtocol: 1 }) });
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
    WebSocket: OfflineThenOnlineWebSocket,
    chrome,
  };
  vm.runInNewContext(backgroundSource, sandbox, { filename: backgroundPath });
  const handler = chrome.runtimeMessages[0];
  let response = null;
  assert.equal(
    handler({ type: "zode-reconnect" }, {}, (value) => {
      response = value;
    }),
    true,
  );
  await flushImmediates(12);

  assert.equal(sockets.length, 2);
  assert.equal(sockets[0].url, "ws://127.0.0.1:17657");
  assert.equal(sockets[1].url, "ws://127.0.0.1:17658");
  assert.deepEqual(nativeCalls, ["ai.zode.browser_bridge"]);
  assert.deepEqual(nativeMessages, [{ type: "start", protocolVersion: 1 }]);
  assert.equal(response.ok, true);
  assert.equal(response.status.connected, true);
  assert.equal(response.status.port, 17658);
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
  // about:blank, not the NTP: the debugger cannot attach to chrome:// targets
  assert.equal(h.chrome.calls.created[0].url, "about:blank");
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

async function testSidePanelTurnTargetsTheActivePage() {
  const h = makeRpcHarness();
  h.chrome.addTab({ id: 5, url: "https://human.example/", active: true });
  await h.connect();

  const turn = h.taskRequest("turn/start", { taskId: "s1", input: "analyze this page" });
  await flushImmediates();

  const result = await h.rpc({
    id: 12,
    kind: "cdp",
    method: "Runtime.evaluate",
    params: { expression: "document.title" },
  });
  assert.equal(result.error, undefined);
  assert.equal(h.chrome.calls.created.length, 0, "side-panel turn opened a new tab");
  assert.equal(h.chrome.calls.attached.at(-1), 5);
  assert.equal(h.chrome.tabsById.get(5).active, true);

  const request = h.frames.find(
    (frame) => frame.channel === "tasks" && frame.method === "turn/start",
  );
  await h.deliver({
    channel: "tasks",
    kind: "response",
    id: request.id,
    result: { turnId: "turn-1" },
  });
  assert.deepEqual(plain(await turn), { ok: true, result: { turnId: "turn-1" } });
}

async function testTabRpcsManageControlledTab() {
  const h = makeRpcHarness();
  await h.connect();

  const created = await h.rpc({
    id: 20,
    kind: "ext",
    method: "tabs.new",
    params: { url: "https://a.example/" },
  });
  assert.equal(created.error, undefined);
  const newTabId = Number(created.result.id);

  // tabs.new took control: the next cdp action reuses it
  await h.rpc({ id: 21, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 1);
  assert.equal(h.chrome.calls.attached.at(-1), newTabId);

  // tabs.select repoints control at an arbitrary existing tab
  h.chrome.addTab({ id: 7, url: "https://b.example/" });
  await h.rpc({ id: 22, kind: "ext", method: "tabs.select", params: { id: "7" } });
  assert.equal(h.chrome.calls.attached.at(-1), 7);

  // tabs.current reports the controlled tab, not the active one
  h.chrome.addTab({ id: 8, url: "https://active.example/", active: true });
  const current = await h.rpc({ id: 23, kind: "ext", method: "tabs.current" });
  assert.equal(current.result, "https://b.example/");

  // closing the controlled tab clears control; next action acquires afresh
  h.chrome.fireTabRemoved(7);
  await h.rpc({ id: 24, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 2);
}

async function testHumanNavigationHandsOffControlledTab() {
  const h = makeRpcHarness();
  await h.connect();
  await h.rpc({
    id: 30,
    kind: "cdp",
    method: "Page.navigate",
    params: { url: "https://zode.example/" },
  });
  const zodeTabId = h.chrome.calls.attached[0];

  // zode's own navigate is rewritten away from the CDP default ("typed"),
  // so it can never look like a human address-bar navigation
  const nav = h.chrome.calls.commands.find((c) => c.method === "Page.navigate");
  assert.equal(nav.params.transitionType, "link");
  assert.equal(nav.params.url, "https://zode.example/");

  // link/redirect commits do not hand off
  h.chrome.fireNavCommitted({
    tabId: zodeTabId,
    frameId: 0,
    transitionType: "link",
    transitionQualifiers: [],
  });
  await h.rpc({ id: 31, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 1);

  // an address-bar commit hands off: detach now, fresh tab on the next action
  h.chrome.fireNavCommitted({
    tabId: zodeTabId,
    frameId: 0,
    transitionType: "typed",
    transitionQualifiers: ["from_address_bar"],
  });
  await flushImmediates();
  assert.equal(h.chrome.calls.detached.at(-1), zodeTabId);
  await h.rpc({ id: 32, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 2);
}

async function testDebuggerDetachReasonControlsHandoff() {
  const h = makeRpcHarness();
  await h.connect();
  await h.rpc({ id: 40, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  const zodeTabId = h.chrome.calls.attached[0];

  // target_closed-style detach keeps control: the next action re-attaches the same tab
  h.chrome.fireDebuggerDetach({ tabId: zodeTabId }, "target_closed");
  await h.rpc({ id: 41, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 1);
  assert.deepEqual(h.chrome.calls.attached, [zodeTabId, zodeTabId]);

  // the user clicking "Cancel" on the debugger bar releases the tab entirely
  h.chrome.fireDebuggerDetach({ tabId: zodeTabId }, "canceled_by_user");
  await h.rpc({ id: 42, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  assert.equal(h.chrome.calls.created.length, 2);
}

async function testScreenshotActivatesControlledTabAndRestores() {
  const h = makeRpcHarness();
  h.chrome.addTab({ id: 5, url: "https://human.example/", active: true });
  await h.connect();
  await h.rpc({ id: 50, kind: "cdp", method: "Runtime.evaluate", params: { expression: "1" } });
  const zodeTabId = h.chrome.calls.attached[0];
  assert.equal(h.chrome.tabsById.get(5).active, true);

  await h.rpc({ id: 51, kind: "cdp", method: "Page.captureScreenshot", params: { format: "jpeg" } });
  const activations = h.chrome.calls.updated.filter(([, props]) => props && props.active);
  assert.deepEqual(
    activations.map(([id]) => id),
    [zodeTabId, 5],
  );
  assert.equal(h.chrome.tabsById.get(5).active, true);
  assert.equal(
    h.chrome.calls.commands.filter((c) => c.method === "Page.captureScreenshot").length,
    1,
  );

  // no activate/restore churn when the controlled tab is already active
  h.chrome.setActive(zodeTabId);
  await h.rpc({ id: 52, kind: "cdp", method: "Page.captureScreenshot", params: {} });
  assert.equal(h.chrome.calls.updated.filter(([, props]) => props && props.active).length, 2);
}

async function testStartupCreatesThemeWatcher() {
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
  await flushImmediates();
  assert.equal(chrome.calls.offscreenCreated.length, 1);
  assert.equal(chrome.calls.offscreenCreated[0].url, "offscreen.html");
  assert.deepEqual(chrome.calls.offscreenCreated[0].reasons, ["MATCH_MEDIA"]);
  assert.deepEqual(chrome.calls.runtimeSent, []);
}

async function testStartupPingsExistingThemeWatcher() {
  const chrome = makeChrome();
  chrome.offscreenContexts.push({ contextType: "OFFSCREEN_DOCUMENT" });
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
  await flushImmediates();
  assert.equal(chrome.calls.offscreenCreated.length, 0);
  assert.deepEqual(chrome.calls.runtimeSent, [{ type: "zode-theme-ping" }]);
}

async function testThemeMessageSwitchesIcon() {
  const h = makeRpcHarness();
  const handler = h.chrome.runtimeMessages[0];
  assert.equal(handler({ type: "zode-theme", dark: false }, {}, () => {}), false);
  await flushImmediates();
  assert.deepEqual(h.chrome.calls.iconPaths.at(-1), {
    16: "icons/zode-light-16.png",
    32: "icons/zode-light-32.png",
    48: "icons/zode-light-48.png",
    128: "icons/zode-light-128.png",
  });
  handler({ type: "zode-theme", dark: true }, {}, () => {});
  await flushImmediates();
  assert.deepEqual(h.chrome.calls.iconPaths.at(-1), {
    16: "icons/zode-16.png",
    32: "icons/zode-32.png",
    48: "icons/zode-48.png",
    128: "icons/zode-128.png",
  });
}

async function testDownloadsAreConnectionLocalAndNewestFirst() {
  const h = makeRpcHarness();
  h.chrome.fireDownloadCreated({
    id: 1,
    url: "https://before.example/file",
    state: "complete",
    filename: "/secret/history",
    bytesReceived: 1,
    totalBytes: 1,
  });
  await h.connect();
  h.chrome.fireDownloadCreated({
    id: 2,
    url: "https://after.example/a",
    state: "in_progress",
    filename: "",
    bytesReceived: 2,
    totalBytes: 10,
    tabId: 999,
  });
  h.chrome.fireDownloadCreated({
    id: 3,
    url: "https://after.example/b",
    state: "in_progress",
    filename: "",
    bytesReceived: 0,
    totalBytes: 4,
  });
  h.chrome.fireDownloadChanged({
    id: 2,
    state: { current: "complete" },
    filename: { current: "/downloads/a" },
    bytesReceived: { current: 10 },
  });
  const reply = await h.rpc({ id: 60, kind: "ext", method: "downloads.list", params: { limit: 1000 } });
  assert.equal(reply.error, undefined);
  assert.deepEqual(
    reply.result.map((entry) => entry.url),
    ["https://after.example/b", "https://after.example/a"],
  );
  assert.equal(reply.result[1].status, "complete");
  assert.equal(reply.result[1].path, "/downloads/a");
  assert.equal(reply.result[0].attribution, "unknown");
  assert.equal(reply.result.some((entry) => entry.url.includes("before")), false);
}

async function testDownloadCancellationAndLimitValidation() {
  const h = makeRpcHarness();
  await h.connect();
  h.chrome.fireDownloadCreated({ id: 4, url: "https://x/4", state: "in_progress" });
  h.chrome.fireDownloadChanged({
    id: 4,
    state: { current: "interrupted" },
    error: { current: "USER_CANCELED" },
  });
  const one = await h.rpc({ id: 61, kind: "ext", method: "downloads.list", params: { limit: 1 } });
  assert.equal(one.result[0].status, "canceled");
  assert.equal(one.result[0].error, "USER_CANCELED");
  const invalid = await h.rpc({ id: 62, kind: "ext", method: "downloads.list", params: { limit: 0 } });
  assert.match(invalid.error, /limit/);
}

async function testElementPickInspectsActivePageAndBroadcastsTheElement() {
  const h = makeRpcHarness();
  h.chrome.addTab({ id: 7, url: "https://human.example/pricing", active: true });
  await h.connect();
  h.chrome.commandHandler = async (_target, method) => {
    if (method === "DOM.resolveNode") {
      return { object: { objectId: "obj-1" } };
    }
    if (method === "Runtime.callFunctionOn") {
      return { result: { value: { selector: "#buy", tag: "button", text: "Buy now" } } };
    }
    return {};
  };

  const started = await h.runtime({ type: "zode-pick-element" });
  assert.deepEqual(plain(started), {
    ok: true,
    target: { tabId: "7", url: "https://human.example/pricing", title: "" },
  });
  assert.equal(h.chrome.calls.created.length, 0, "picking must not open a new tab");
  assert.equal(h.chrome.calls.attached.at(-1), 7);
  const inspect = h.chrome.calls.commands.find((call) => call.method === "Overlay.setInspectMode");
  assert.equal(inspect.params.mode, "searchForNode");

  h.chrome.fireDebuggerEvent({ tabId: 7 }, "Overlay.inspectNodeRequested", { backendNodeId: 42 });
  await flushImmediates(10);

  const picked = h.chrome.calls.runtimeSent.find((message) => message.type === "zode-element-picked");
  assert.deepEqual(plain(picked.element), { selector: "#buy", tag: "button", text: "Buy now" });
  assert.equal(
    h.chrome.calls.commands.filter((call) => call.method === "Overlay.setInspectMode").at(-1).params
      .mode,
    "none",
    "inspect mode is turned off once the element is picked",
  );
  assert.ok(
    h.chrome.calls.commands.some((call) => call.method === "Runtime.releaseObject"),
    "the resolved node handle is released",
  );

  // A second inspect event without an active pick is ignored.
  const before = h.chrome.calls.runtimeSent.length;
  h.chrome.fireDebuggerEvent({ tabId: 7 }, "Overlay.inspectNodeRequested", { backendNodeId: 43 });
  await flushImmediates(10);
  assert.equal(h.chrome.calls.runtimeSent.length, before, "a stale inspect event is ignored");
}

async function testElementPickRejectsUndebuggablePagesAndCancelsOnNavigation() {
  const h = makeRpcHarness();
  h.chrome.addTab({ id: 8, url: "chrome://settings", active: true });
  await h.connect();

  const rejected = await h.runtime({ type: "zode-pick-element" });
  assert.equal(rejected.ok, false);
  assert.match(rejected.error, /cannot be inspected/);
  assert.equal(h.chrome.calls.attached.length, 0, "an unsupported page never attaches");

  h.chrome.addTab({ id: 9, url: "https://human.example/", active: true });
  assert.equal((await h.runtime({ type: "zode-pick-element" })).ok, true);
  h.chrome.fireNavCommitted({ tabId: 9, frameId: 0, transitionType: "link" });
  await flushImmediates();
  const canceled = h.chrome.calls.runtimeSent.filter(
    (message) => message.type === "zode-element-pick-canceled",
  );
  assert.equal(canceled.length, 1);
  assert.match(canceled[0].reason, /navigated/);

  // The pick is gone, so a late inspect event cannot resurrect it.
  const before = h.chrome.calls.runtimeSent.length;
  h.chrome.fireDebuggerEvent({ tabId: 9 }, "Overlay.inspectNodeRequested", { backendNodeId: 1 });
  await flushImmediates(10);
  assert.equal(h.chrome.calls.runtimeSent.length, before);
}

function fakeElementTree() {
  const index = new Map();
  const doc = {
    title: "Pricing",
    querySelectorAll: (selector) => index.get(selector) || [],
  };
  const view = { location: { href: "https://x/pricing" }, CSS: { escape: (value) => value } };
  view.top = view;
  doc.defaultView = view;

  const element = (tag, attrs = {}, extra = {}) => {
    const node = {
      nodeType: 1,
      tagName: tag.toUpperCase(),
      ownerDocument: doc,
      parentElement: null,
      children: [],
      className: attrs.class || "",
      attributes: Object.entries(attrs).map(([name, value]) => ({ name, value })),
      getAttribute: (name) => (Object.hasOwn(attrs, name) ? attrs[name] : null),
      getBoundingClientRect: () => ({ left: 12.4, top: 40.6, width: 120, height: 36 }),
      outerHTML: `<${tag}>${extra.text || ""}</${tag}>`,
      ...extra,
    };
    return node;
  };
  const append = (parent, child) => {
    parent.children.push(child);
    child.parentElement = parent;
    return child;
  };
  return { doc, view, index, element, append };
}

function testPickedElementSummaryIsSelectorAccurateAndPasswordSafe() {
  const h = makeRpcHarness();
  const describe = new Function(`return (${h.describeElementSource()})`)();
  const dom = fakeElementTree();

  const withId = dom.element(
    "button",
    { id: "buy", class: "primary cta" },
    { text: " Buy  now ", innerText: " Buy  now " },
  );
  dom.index.set("#buy", [withId]);
  const described = describe.call(withId);
  assert.equal(described.selector, "#buy");
  assert.equal(described.label, "button#buy.primary.cta");
  assert.equal(described.tag, "button");
  assert.equal(described.text, "Buy now");
  assert.equal(described.url, "https://x/pricing");
  assert.equal(described.title, "Pricing");
  assert.equal(described.inFrame, false);
  assert.deepEqual(described.rect, { x: 12, y: 41, width: 120, height: 36 });
  assert.deepEqual(described.attributes, [
    { name: "id", value: "buy" },
    { name: "class", value: "primary cta" },
  ]);

  // No usable id: the path walks up until querySelectorAll proves it unique.
  const root = dom.element("div", { id: "root" });
  const twin = dom.append(root, dom.element("span"));
  const target = dom.append(root, dom.element("span", {}, { innerText: "second" }));
  dom.index.set("#root", [root]);
  dom.index.set("span:nth-of-type(2)", [twin, target]);
  dom.index.set("#root > span:nth-of-type(2)", [target]);
  const nested = describe.call(target);
  assert.equal(nested.selector, "#root > span:nth-of-type(2)");
  assert.equal(nested.label, "span");

  // A text node resolves to its element, and a password value is never read.
  const password = dom.element("input", { type: "password" }, { type: "password", value: "hunter2" });
  password.tagName = "INPUT";
  assert.equal(describe.call(password).value, "");
  const search = dom.element("input", { type: "search" }, { type: "search", value: "zode" });
  search.tagName = "INPUT";
  assert.equal(describe.call(search).value, "zode");
  assert.equal(describe.call({ nodeType: 3, parentElement: withId }).selector, "#buy");
  assert.equal(describe.call(null), null);
}

(async () => {
  await testStartupCreatesThemeWatcher();
  await testStartupPingsExistingThemeWatcher();
  await testThemeMessageSwitchesIcon();
  await testBackgroundStartupDoesNotTouchWebSocket();
  await testStartupEnablesActionOpenedSidePanelExactlyOnce();
  await testOpenSidePanelCommandOpensPanelForFocusedWindow();
  await testSidePanelStartupIsBestEffortWhenApiIsUnavailableOrFails();
  await testStatusMessageReturnsConnectionState();
  await testStatusLoadsStoredPortAndReconnectCapability();
  await testReconnectMessageUsesStoredTokenAndPort();
  await testReconnectAutoStartsNativeHostWhenZodeIsOffline();
  await testAuthenticatedAndReplacementSocketsBroadcastTaskLifecycle();
  await testUnsupportedTaskProtocolKeepsLegacyRpcAvailable();
  await testTaskRequestResponseUsesDedicatedWireId();
  await testTaskRequestRegistersPendingBeforeSynchronousResponse();
  await testStaleRpcCompletionNeverWritesReplacementSocket();
  await testInFlightGuardCoversPreSocketAsyncWork();
  await testOverlappingUnauthenticatedConnectReturnsBusy();
  await testPairingBlocksOldTokenReconnectUntilNewTokenIsPersisted();
  await testPairTokenWriteSurvivesCloseAndReconnectWaitsForIt();
  await testSilentAuthenticationTimesOutAndReleasesGuard();
  await testTaskErrorUnknownAndLateIdsSettleOnlyTheirPendingRequest();
  await testInvalidTaskMethodFailsBeforeTimerOrFrame();
  await testTaskPendingCapRejectsBeforeTimerOrFrame();
  await testTaskEventBroadcastsBeforeRpcAndRuntimeRejectionIsBestEffort();
  await testNaturalCloseRejectsAllTaskPendingAndBroadcastsOnce();
  await testReplacementRejectsPendingAndStaleCloseCannotClearNewSocket();
  await testTaskSendFailureClearsPendingAndTimer();
  await testTaskTimeoutClearsPendingAndIgnoresLateResponse();
  await testCdpActionCreatesAndReusesControlledTab();
  await testSidePanelTurnTargetsTheActivePage();
  await testTabRpcsManageControlledTab();
  await testHumanNavigationHandsOffControlledTab();
  await testDebuggerDetachReasonControlsHandoff();
  await testScreenshotActivatesControlledTabAndRestores();
  await testDownloadsAreConnectionLocalAndNewestFirst();
  await testDownloadCancellationAndLimitValidation();
  await testElementPickInspectsActivePageAndBroadcastsTheElement();
  await testElementPickRejectsUndebuggablePagesAndCancelsOnNavigation();
  testPickedElementSummaryIsSelectorAccurateAndPasswordSafe();
  console.log("background tests passed");
})();
