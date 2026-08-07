// zode browser bridge - MV3 service worker.
// Connects to the local zode BridgeServer, authenticates with a pairing code
// or stored token, and relays RPC frames to chrome.debugger / chrome.tabs.

const STORAGE_TOKEN = "zodeToken";
const STORAGE_PORT = "zodePort";
const KEEPALIVE_MS = 20_000;
const AUTH_ATTEMPT_TIMEOUT_MS = 15_000;
const TASK_REQUEST_TIMEOUT_MS = 30_000;
const TASK_PROTOCOL_VERSION = 1;
const TASK_PENDING_CAP = 500;
const NATIVE_HOST_NAME = "ai.zode.browser_bridge";
const NATIVE_START_TIMEOUT_MS = 30_000;
const BUF_CAP = 500;
const PENDING_REQUEST_CAP = 500;
const DOWNLOAD_LIMIT_CAP = 100;
const ELEMENT_PICK_TIMEOUT_MS = 120_000;
const ELEMENT_TEXT_CAP = 400;
const ELEMENT_HTML_CAP = 2000;
const ELEMENT_ATTR_VALUE_CAP = 200;
const ELEMENT_ATTR_CAP = 24;
const PICKABLE_SCHEMES = ["http:", "https:", "file:"];
const ZODE_TAB_GROUP_TITLE = "zode";
const ZODE_TAB_GROUP_COLOR = "blue";
const DARK_ICONS = {
  16: "icons/zode-16.png",
  32: "icons/zode-32.png",
  48: "icons/zode-48.png",
  128: "icons/zode-128.png",
};
const LIGHT_ICONS = {
  16: "icons/zode-light-16.png",
  32: "icons/zode-light-32.png",
  48: "icons/zode-light-48.png",
  128: "icons/zode-light-128.png",
};

let ws = null;
let keepalive = null;
let lastPort = null;
let attachedTabId = null;
let controlledTabId = null;
let zodeGroupId = null;
let consoleBuf = [];
let networkBuf = [];
let pendingRequests = new Map();
let offscreenCreation = null;
let downloadSessionActive = false;
let downloadBuf = [];
let nextTaskRequestId = 1;
let nextTaskConnectionId = 1;
const pendingTaskRequests = new Map();
let tokenMutationQueue = Promise.resolve();
let authAttempt = null;
let nativePort = null;
let nativeStatus = null;
let nativeStartPromise = null;
let elementPick = null;

async function getStored(keys) {
  return await chrome.storage.local.get(keys);
}

async function getToken() {
  const data = await getStored(STORAGE_TOKEN);
  return data[STORAGE_TOKEN] || null;
}

async function getPort() {
  const data = await getStored(STORAGE_PORT);
  const port = Number(data[STORAGE_PORT]);
  return Number.isInteger(port) && port > 0 && port <= 65535 ? port : null;
}

async function setToken(token) {
  await chrome.storage.local.set({ [STORAGE_TOKEN]: token });
}

async function clearToken() {
  await chrome.storage.local.remove(STORAGE_TOKEN);
}

async function setPort(port) {
  lastPort = port;
  await chrome.storage.local.set({ [STORAGE_PORT]: port });
}

function mutateTokenForSocket(socket, mutation) {
  if (ws !== socket) {
    return Promise.resolve(false);
  }
  const nextMutation = tokenMutationQueue.catch(() => {}).then(async () => {
    await mutation();
    return true;
  });
  tokenMutationQueue = nextMutation;
  return nextMutation;
}

function persistTokenForSocket(socket, token) {
  return mutateTokenForSocket(socket, () => setToken(token));
}

function clearTokenForSocket(socket) {
  return mutateTokenForSocket(socket, clearToken);
}

function statusSnapshot(port = lastPort, token = null) {
  const connected = ws != null && ws.readyState === WebSocket.OPEN;
  const taskClientSupported = connected && ws.taskProtocolVersion === TASK_PROTOCOL_VERSION;
  return {
    connected,
    port,
    attachedTabId,
    canReconnect: !connected && port != null && token != null,
    taskClientSupported,
    taskConnectionId: taskClientSupported ? ws.taskConnectionId : null,
  };
}

function createTaskConnectionId() {
  if (globalThis.crypto && typeof globalThis.crypto.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  const sequence = nextTaskConnectionId++;
  return `task-${Date.now().toString(36)}-${sequence}-${Math.random().toString(36).slice(2)}`;
}

function announceTaskTransport(socket) {
  if (socket.taskProtocolVersion === TASK_PROTOCOL_VERSION) {
    socket.taskConnectionId ||= createTaskConnectionId();
    broadcastRuntime({
      type: "zode-task-connected",
      connectionId: socket.taskConnectionId,
      protocolVersion: socket.taskProtocolVersion,
    });
    return;
  }
  socket.taskConnectionId = null;
  broadcastRuntime({
    type: "zode-task-unsupported",
    message: "Current zode version does not support the task client",
  });
}

function connect(port, code) {
  if (authAttempt) {
    return Promise.reject(new Error("zode bridge authentication already in progress"));
  }
  const attempt = { socket: null, expired: false, timeout: null };
  authAttempt = attempt;
  const timeout = new Promise((_, reject) => {
    attempt.timeout = setTimeout(() => {
      attempt.expired = true;
      reject(new Error("zode bridge authentication timed out"));
      if (attempt.socket && ws === attempt.socket) {
        closeSocket();
      }
    }, AUTH_ATTEMPT_TIMEOUT_MS);
  });
  return Promise.race([connectAttempt(port, code, attempt), timeout]).finally(() => {
    clearTimeout(attempt.timeout);
    if (authAttempt === attempt) {
      authAttempt = null;
    }
  });
}

function requireCurrentAuthAttempt(attempt) {
  if (authAttempt !== attempt || attempt.expired) {
    throw new Error("zode bridge authentication attempt is no longer active");
  }
}

async function connectAttempt(port, code, attempt) {
  let targetPort = port;
  if (!targetPort && !code) {
    targetPort = await getPort();
    requireCurrentAuthAttempt(attempt);
    if (!targetPort) {
      throw new Error("no stored zode bridge port");
    }
  }
  const numericPort = Number(targetPort);
  if (!Number.isInteger(numericPort) || numericPort <= 0 || numericPort > 65535) {
    throw new Error("enter the WS port shown by /browser pair");
  }
  await tokenMutationQueue.catch(() => {});
  requireCurrentAuthAttempt(attempt);
  const token = await getToken();
  requireCurrentAuthAttempt(attempt);
  if (!code && !token) {
    throw new Error("enter the pairing code shown by /browser pair");
  }
  await setPort(numericPort);
  requireCurrentAuthAttempt(attempt);
  closeSocket();

  return await new Promise((resolve, reject) => {
    let authenticated = false;
    let settled = false;
    let socket = null;

    const resolveAuth = () => {
      if (!settled) {
        authenticated = true;
        settled = true;
        socket.cancelConnect = null;
        // Connected — stop the background retry cadence until we drop.
        clearReconnectAlarm();
        announceTaskTransport(socket);
        resolve(statusSnapshot(numericPort, token));
      }
    };
    const rejectAuth = (error) => {
      if (!settled) {
        settled = true;
        if (socket) {
          socket.cancelConnect = null;
        }
        reject(error);
      }
    };

    socket = new WebSocket(`ws://127.0.0.1:${numericPort}`);
    attempt.socket = socket;
    socket.authToken = token;
    socket.taskProtocolVersion = null;
    socket.taskConnectionId = null;
    socket.cancelConnect = (error) => rejectAuth(error);
    ws = socket;
    socket.onopen = async () => {
      if (ws !== socket) {
        return;
      }
      try {
        downloadBuf = [];
        downloadSessionActive = true;
        if (code) {
          send({ type: "pair", code: String(code) });
        } else if (token) {
          send({ type: "auth", token });
        }
        keepalive = setInterval(() => {
          try {
            send({ type: "ping" });
          } catch (_) {
            // The close handler clears socket state.
          }
        }, KEEPALIVE_MS);
      } catch (error) {
        console.error("zode bridge open failed", error);
        rejectAuth(error);
        closeSocket();
      }
    };
    socket.onmessage = (event) => {
      if (ws !== socket) {
        return;
      }
      handleMessage(event.data, socket)
        .then((result) => {
          if (ws !== socket) {
            return;
          }
          if (result === "authenticated") {
            resolveAuth();
          }
        })
        .catch((error) => {
          if (ws !== socket) {
            return;
          }
          console.error("zode bridge message failed", error);
          rejectAuth(error);
        });
    };
    socket.onerror = () => {
      if (ws !== socket) {
        return;
      }
      rejectAuth(new Error("zode bridge websocket error"));
    };
    socket.onclose = () => {
      if (ws !== socket) {
        return;
      }
      clearInterval(keepalive);
      keepalive = null;
      ws = null;
      downloadSessionActive = false;
      disconnectTaskTransport("zode task connection closed");
      // Dropped (zode exited / restarted): arm the quiet retry cadence so
      // the stored token reconnects without the user re-pairing.
      scheduleReconnectAlarm();
      if (!authenticated) {
        rejectAuth(new Error("zode bridge connection closed"));
      }
    };
  });
}

async function connectionStatus() {
  const [storedPort, token] = await Promise.all([getPort(), getToken()]);
  const port = lastPort || storedPort;
  if (lastPort == null && storedPort != null) {
    lastPort = storedPort;
  }
  return statusSnapshot(port, token);
}

function applyThemeIcon(dark) {
  if (!chrome.action || !chrome.action.setIcon) {
    return;
  }
  Promise.resolve(chrome.action.setIcon({ path: dark ? DARK_ICONS : LIGHT_ICONS })).catch(
    (error) => console.debug("zode theme icon update failed", error),
  );
}

// chrome://extensions/shortcuts command: open the zode side panel in the
// window the shortcut was pressed in. Command handlers carry the user
// gesture that sidePanel.open() requires; call it directly off the
// listener's tab argument and only fall back to an async window lookup.
function enableSidePanelCommand() {
  if (
    !chrome.commands ||
    !chrome.sidePanel ||
    typeof chrome.sidePanel.open !== "function"
  ) {
    return;
  }
  chrome.commands.onCommand.addListener((command, tab) => {
    if (command !== "open-side-panel") {
      return;
    }
    const openPanel = (windowId) => {
      if (windowId == null) {
        return;
      }
      Promise.resolve(chrome.sidePanel.open({ windowId })).catch((error) =>
        console.debug("zode side panel open failed", error),
      );
    };
    if (tab && tab.windowId != null) {
      openPanel(tab.windowId);
      return;
    }
    Promise.resolve(chrome.windows.getLastFocused())
      .then((window) => openPanel(window && window.id))
      .catch((error) => console.debug("zode side panel open failed", error));
  });
}

function enableActionOpenedSidePanel() {
  if (!chrome.sidePanel || typeof chrome.sidePanel.setPanelBehavior !== "function") {
    return;
  }
  try {
    Promise.resolve(
      chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }),
    ).catch((error) => console.debug("zode side panel behavior unavailable", error));
  } catch (error) {
    console.debug("zode side panel behavior unavailable", error);
  }
}

// Chrome allows one offscreen document per extension; share the in-flight
// creation promise instead of sniffing duplicate-creation error text.
async function ensureThemeWatcher() {
  if (!chrome.offscreen || !chrome.runtime.getContexts) {
    return;
  }
  try {
    const contexts = await chrome.runtime.getContexts({
      contextTypes: ["OFFSCREEN_DOCUMENT"],
    });
    if (contexts.length > 0) {
      // The document survived a worker restart: ask it to re-post.
      await chrome.runtime.sendMessage({ type: "zode-theme-ping" }).catch(() => {});
      return;
    }
    if (!offscreenCreation) {
      offscreenCreation = chrome.offscreen
        .createDocument({
          url: "offscreen.html",
          reasons: ["MATCH_MEDIA"],
          justification: "detect light/dark appearance for the toolbar icon",
        })
        .finally(() => {
          offscreenCreation = null;
        });
    }
    await offscreenCreation;
  } catch (error) {
    console.debug("zode theme watcher unavailable", error);
  }
}

function reconnectStored(port) {
  return connect(port || null, "");
}

function nativeRuntimeError(fallback) {
  const message = chrome.runtime.lastError && chrome.runtime.lastError.message;
  return new Error(
    message ||
      fallback ||
      "zode native host is unavailable; run the updated zode CLI once to install it",
  );
}

function startNativeZode() {
  if (nativePort && nativeStatus) {
    return Promise.resolve(nativeStatus);
  }
  if (nativeStartPromise) {
    return nativeStartPromise;
  }
  nativeStartPromise = new Promise((resolve, reject) => {
    let port;
    let settled = false;
    let timeout;
    const finish = (error, status) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (error) {
        if (nativePort === port) nativePort = null;
        nativeStatus = null;
        try {
          port?.disconnect();
        } catch (_) {
          // Chrome may already have closed a failed native host.
        }
        reject(error);
      } else {
        nativeStatus = status;
        resolve(status);
      }
    };
    try {
      port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
      nativePort = port;
      port.onMessage.addListener((message) => {
        if (!message || message.ok !== true || !Number.isInteger(message.port)) {
          finish(new Error((message && message.error) || "zode native host failed to start"));
          return;
        }
        finish(null, { port: message.port });
      });
      port.onDisconnect.addListener(() => {
        const error = nativeRuntimeError("zode native host disconnected before startup");
        if (nativePort === port) nativePort = null;
        nativeStatus = null;
        if (!settled) finish(error);
      });
      timeout = setTimeout(() => {
        finish(new Error("zode native host startup timed out"));
        try {
          port.disconnect();
        } catch (_) {
          // Chrome may already have closed a failed native host.
        }
      }, NATIVE_START_TIMEOUT_MS);
      port.postMessage({ type: "start", protocolVersion: 1 });
    } catch (error) {
      finish(nativeRuntimeError(String(error && error.message ? error.message : error)));
    }
  });
  const pending = nativeStartPromise;
  pending.then(
    () => {
      if (nativeStartPromise === pending) nativeStartPromise = null;
    },
    () => {
      if (nativeStartPromise === pending) nativeStartPromise = null;
    },
  );
  return nativeStartPromise;
}

async function reconnectOrStart(port) {
  try {
    return await reconnectStored(port);
  } catch (connectionError) {
    let native;
    try {
      native = await startNativeZode();
    } catch (nativeError) {
      throw new Error(
        `${connectionError.message}. ${nativeError.message || nativeError}`,
      );
    }
    return await reconnectStored(native.port || port);
  }
}

// ── Automatic reconnect ────────────────────────────────────────────────
// Pairing has always been persistent (long-term token in chrome.storage +
// ~/.zode/browser-bridge.json); what was missing is a TRIGGER — reconnect
// logic only ran when the popup/side panel explicitly asked, so every zode
// restart looked like it needed re-pairing. Reconnect silently on browser
// startup / extension install, and while disconnected with a stored token
// retry on a periodic alarm (alarms survive MV3 worker suspension).
// Deliberately reconnect-only — never startNativeZode() here: auto-starting
// the native host on a timer would keep relaunching zode after the user
// closed it.
const RECONNECT_ALARM = "zode-auto-reconnect";
const RECONNECT_PERIOD_MINUTES = 1;

async function attemptStoredReconnect(reason) {
  if (ws != null) {
    return; // already connected or connecting
  }
  const [port, token] = await Promise.all([getPort(), getToken()]);
  if (port == null || token == null) {
    clearReconnectAlarm(); // never paired (or unpaired) — nothing to retry
    return;
  }
  try {
    await reconnectStored(port);
    console.debug("zode auto-reconnect ok", reason);
  } catch (error) {
    console.debug(
      "zode auto-reconnect failed",
      reason,
      String((error && error.message) || error),
    );
  }
}

function scheduleReconnectAlarm() {
  if (!chrome.alarms) {
    return;
  }
  Promise.resolve(
    chrome.alarms.create(RECONNECT_ALARM, {
      periodInMinutes: RECONNECT_PERIOD_MINUTES,
    }),
  ).catch(() => {});
}

function clearReconnectAlarm() {
  if (!chrome.alarms) {
    return;
  }
  Promise.resolve(chrome.alarms.clear(RECONNECT_ALARM)).catch(() => {});
}

if (chrome.alarms && chrome.alarms.onAlarm) {
  chrome.alarms.onAlarm.addListener((alarm) => {
    if (alarm.name === RECONNECT_ALARM) {
      attemptStoredReconnect("alarm").catch(() => {});
    }
  });
}
if (chrome.runtime && chrome.runtime.onStartup) {
  chrome.runtime.onStartup.addListener(() => {
    scheduleReconnectAlarm();
    attemptStoredReconnect("startup").catch(() => {});
  });
}
if (chrome.runtime && chrome.runtime.onInstalled) {
  chrome.runtime.onInstalled.addListener(() => {
    scheduleReconnectAlarm();
    attemptStoredReconnect("installed").catch(() => {});
  });
}

function closeSocket() {
  downloadSessionActive = false;
  if (ws != null) {
    const old = ws;
    ws = null;
    old.onclose = null;
    if (old.cancelConnect) {
      old.cancelConnect(new Error("zode bridge connection replaced"));
      old.cancelConnect = null;
    }
    disconnectTaskTransport("zode task connection replaced");
    try {
      old.close();
    } catch (_) {
      // Closing a half-open socket is best effort.
    }
  }
  clearInterval(keepalive);
  keepalive = null;
}

function sendOnSocket(socket, value) {
  if (!socket || ws !== socket || socket.readyState !== WebSocket.OPEN) {
    return false;
  }
  socket.send(JSON.stringify(value));
  return true;
}

function send(value) {
  const socket = ws;
  if (!sendOnSocket(socket, value)) {
    throw new Error("zode bridge is not connected");
  }
}

function broadcastRuntime(message) {
  try {
    Promise.resolve(chrome.runtime.sendMessage(message)).catch((error) => {
      console.debug("zode task runtime broadcast failed", error);
    });
  } catch (error) {
    console.debug("zode task runtime broadcast failed", error);
  }
}

function rejectTaskPending(reason) {
  const error = reason instanceof Error ? reason : new Error(String(reason));
  for (const [id, pending] of pendingTaskRequests) {
    pendingTaskRequests.delete(id);
    clearTimeout(pending.timeout);
    pending.reject(error);
  }
}

function disconnectTaskTransport(reason) {
  rejectTaskPending(reason);
  broadcastRuntime({ type: "zode-task-disconnected" });
}

function requestTask(method, params = {}) {
  if (typeof method !== "string" || method.trim().length === 0) {
    return Promise.reject(new Error("zode task method must be a non-empty string"));
  }
  if (
    ws != null &&
    ws.readyState === WebSocket.OPEN &&
    ws.taskProtocolVersion !== TASK_PROTOCOL_VERSION
  ) {
    const error = new Error("Current zode version does not support the task client");
    error.code = "unsupported_task_client";
    return Promise.reject(error);
  }
  if (pendingTaskRequests.size >= TASK_PENDING_CAP) {
    return Promise.reject(new Error(`zode task pending request limit is ${TASK_PENDING_CAP}`));
  }
  const id = `ext-${nextTaskRequestId++}`;
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      const pending = pendingTaskRequests.get(id);
      if (!pending) {
        return;
      }
      pendingTaskRequests.delete(id);
      pending.reject(new Error(`zode task request ${method} (${id}) timed out`));
    }, TASK_REQUEST_TIMEOUT_MS);
    pendingTaskRequests.set(id, { resolve, reject, timeout });
    try {
      send({ channel: "tasks", kind: "request", id, method, params });
    } catch (error) {
      pendingTaskRequests.delete(id);
      clearTimeout(timeout);
      reject(error);
    }
  });
}

// A turn submitted from the side panel is about the page beside the panel.
// Make that page the bridge target before the agent can invoke browser_read;
// unlike zode-created tabs, it remains in the user's existing tab group.
async function targetActivePageForSidePanelTurn() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab || tab.id == null) {
    return;
  }
  if (controlledTabId !== tab.id) {
    await detachAttached();
    controlledTabId = tab.id;
  }
}

function handleTaskFrame(msg) {
  if (msg.kind === "event") {
    broadcastRuntime({
      type: "zode-task-event",
      event: msg.event,
      params: msg.params,
    });
    return;
  }
  if (msg.kind !== "response" && msg.kind !== "error") {
    return;
  }
  const pending = pendingTaskRequests.get(msg.id);
  if (!pending) {
    return;
  }
  pendingTaskRequests.delete(msg.id);
  clearTimeout(pending.timeout);
  if (msg.kind === "response") {
    pending.resolve(msg.result);
  } else {
    const error = new Error(msg.message || msg.code || "zode task request failed");
    if (msg.code != null) {
      error.code = msg.code;
    }
    pending.reject(error);
  }
}

async function handleMessage(raw, sourceSocket) {
  if (ws !== sourceSocket) {
    return null;
  }
  const msg = JSON.parse(raw);
  if (msg.channel === "tasks") {
    handleTaskFrame(msg);
    return null;
  }
  if (msg.type === "paired") {
    sourceSocket.taskProtocolVersion = Number.isInteger(msg.taskProtocol)
      ? msg.taskProtocol
      : null;
    sourceSocket.authToken = msg.token;
    const current = await persistTokenForSocket(sourceSocket, msg.token);
    return current ? "authenticated" : null;
  }
  if (msg.type === "ok") {
    sourceSocket.taskProtocolVersion = Number.isInteger(msg.taskProtocol)
      ? msg.taskProtocol
      : null;
    const current = await persistTokenForSocket(sourceSocket, sourceSocket.authToken);
    return current ? "authenticated" : null;
  }
  if (msg.type === "ping") {
    return null;
  }
  if (msg.type === "rejected") {
    const current = await clearTokenForSocket(sourceSocket);
    if (!current) {
      return null;
    }
    throw new Error(msg.reason || "zode bridge rejected authentication");
  }
  await handleRpc(msg, sourceSocket);
  return null;
}

async function handleRpc(request, sourceSocket) {
  let response;
  try {
    const result = await dispatch(request);
    response = { id: request.id, result };
  } catch (error) {
    response = { id: request.id, error: String((error && error.message) || error) };
  }
  sendOnSocket(sourceSocket, response);
}

async function dispatch(request) {
  if (request.kind === "cdp") {
    await ensureControlledTab();
    let params = request.params || {};
    if (request.method === "Page.navigate") {
      // CDP defaults transitionType to "typed", which would look like a
      // human address-bar navigation to the handoff listener below.
      params = { transitionType: "link", ...params };
    }
    if (request.method === "Page.captureScreenshot") {
      return await captureControlledTab(params);
    }
    return await cdp(request.method, params);
  }
  if (request.kind !== "ext") {
    throw new Error(`unknown rpc kind ${request.kind}`);
  }

  const params = request.params || {};
  switch (request.method) {
    case "tabs.list": {
      const tabs = await chrome.tabs.query({});
      return tabs.map(toTabInfo);
    }
    case "tabs.new": {
      const tab = await chrome.tabs.create({ url: params.url || undefined });
      await groupZodeTab(tab);
      controlledTabId = tab.id;
      await attachTo(tab.id);
      return toTabInfo(tab);
    }
    case "tabs.close": {
      await chrome.tabs.remove(Number(params.id));
      if (attachedTabId === Number(params.id)) {
        attachedTabId = null;
      }
      if (controlledTabId === Number(params.id)) {
        controlledTabId = null;
      }
      return null;
    }
    case "tabs.select": {
      const tabId = Number(params.id);
      await chrome.tabs.update(tabId, { active: true });
      controlledTabId = tabId;
      await attachTo(tabId);
      return null;
    }
    case "tabs.current": {
      const tab = await ensureControlledTab();
      return tab.url || "";
    }
    case "logs.console":
      return tail(consoleBuf, Number(params.limit) || 100);
    case "logs.network":
      return tail(networkBuf, Number(params.limit) || 100);
    case "downloads.list": {
      const limit = Number(params.limit == null ? DOWNLOAD_LIMIT_CAP : params.limit);
      if (!Number.isInteger(limit) || limit < 1) {
        throw new Error("downloads.list limit must be a positive integer");
      }
      return downloadBuf.slice(-Math.min(limit, DOWNLOAD_LIMIT_CAP)).reverse();
    }
    case "debugger.detach":
      await detachAttached();
      return null;
    default:
      throw new Error(`unknown ext method ${request.method}`);
  }
}

async function controlledTab() {
  if (controlledTabId == null) {
    return null;
  }
  try {
    return await chrome.tabs.get(controlledTabId);
  } catch (_) {
    controlledTabId = null;
    return null;
  }
}

async function attachTo(tabId) {
  if (attachedTabId === tabId) {
    return;
  }
  await detachAttached();
  await chrome.debugger.attach({ tabId }, "1.3");
  attachedTabId = tabId;
  await cdp("Runtime.enable", {});
  await cdp("Network.enable", {});
  await cdp("Page.enable", {});
}

// Standalone bridge automation never takes over a human tab: when it has no
// controlled tab it creates a fresh background one inside the zode tab group.
// Side-panel turns set controlledTabId to the adjacent active page first.
async function ensureControlledTab() {
  const existing = await controlledTab();
  if (existing) {
    await attachTo(existing.id);
    return existing;
  }
  // Explicit about:blank: a URL-less create opens chrome://newtab/, and
  // chrome.debugger.attach cannot attach to chrome:// targets.
  const tab = await chrome.tabs.create({ active: false, url: "about:blank" });
  await groupZodeTab(tab);
  controlledTabId = tab.id;
  await attachTo(tab.id);
  return tab;
}

// Headful Chrome does not render captureScreenshot for hidden tabs:
// briefly activate the controlled tab, then give the user their tab back.
async function captureControlledTab(params) {
  const tab = await controlledTab();
  if (!tab || tab.active) {
    return await cdp("Page.captureScreenshot", params);
  }
  const [previous] = await chrome.tabs.query({ active: true, windowId: tab.windowId });
  await chrome.tabs.update(tab.id, { active: true });
  try {
    return await cdp("Page.captureScreenshot", params);
  } finally {
    if (previous && previous.id != null && previous.id !== tab.id) {
      try {
        await chrome.tabs.update(previous.id, { active: true });
      } catch (_) {
        // The previous tab may have closed mid-capture.
      }
    }
  }
}

async function detachAttached() {
  if (attachedTabId == null) {
    return;
  }
  const tabId = attachedTabId;
  attachedTabId = null;
  try {
    await chrome.debugger.detach({ tabId });
  } catch (_) {
    // The tab may already be closed or detached by Chrome.
  }
}

function cdp(method, params) {
  if (attachedTabId == null) {
    throw new Error("debugger is not attached");
  }
  return chrome.debugger.sendCommand({ tabId: attachedTabId }, method, params);
}

// DevTools-style element picker, driven entirely through CDP so it needs no
// host permissions or content script beyond the debugger access the bridge
// already holds. Overlay.setInspectMode draws the hover highlight; the click
// arrives as Overlay.inspectNodeRequested.
const INSPECT_HIGHLIGHT = {
  contentColor: { r: 111, g: 168, b: 220, a: 0.4 },
  paddingColor: { r: 147, g: 196, b: 125, a: 0.35 },
  borderColor: { r: 255, g: 229, b: 153, a: 0.4 },
  marginColor: { r: 246, g: 178, b: 107, a: 0.35 },
  showInfo: true,
  showExtensionLines: false,
};

// Runs in the page with `this` bound to the picked node. Returns the plain
// summary the side panel attaches to its next turn.
const DESCRIBE_ELEMENT_FN = `function () {
  var TEXT_CAP = ${ELEMENT_TEXT_CAP};
  var HTML_CAP = ${ELEMENT_HTML_CAP};
  var ATTR_VALUE_CAP = ${ELEMENT_ATTR_VALUE_CAP};
  var ATTR_CAP = ${ELEMENT_ATTR_CAP};
  var node = this;
  if (node && node.nodeType === 3) {
    node = node.parentElement;
  }
  if (!node || node.nodeType !== 1) {
    return null;
  }
  var doc = node.ownerDocument;
  var view = doc && doc.defaultView;

  function clamp(value, cap) {
    var text = String(value == null ? "" : value);
    return text.length > cap ? text.slice(0, cap) + "\\u2026" : text;
  }

  function esc(value) {
    if (view && view.CSS && typeof view.CSS.escape === "function") {
      return view.CSS.escape(String(value));
    }
    return String(value).replace(/[^a-zA-Z0-9_-]/g, function (ch) {
      return "\\\\" + ch;
    });
  }

  function isUnique(selector) {
    try {
      return doc.querySelectorAll(selector).length === 1;
    } catch (_) {
      return false;
    }
  }

  function selectorFor(el) {
    var parts = [];
    var current = el;
    var depth = 0;
    while (current && current.nodeType === 1 && depth < 12) {
      depth += 1;
      var id = current.getAttribute ? current.getAttribute("id") : null;
      if (id && isUnique("#" + esc(id))) {
        parts.unshift("#" + esc(id));
        var rooted = parts.join(" > ");
        return isUnique(rooted) ? rooted : "";
      }
      var part = current.tagName.toLowerCase();
      var parent = current.parentElement;
      if (parent) {
        var twins = [];
        for (var i = 0; i < parent.children.length; i += 1) {
          if (parent.children[i].tagName === current.tagName) {
            twins.push(parent.children[i]);
          }
        }
        if (twins.length > 1) {
          part += ":nth-of-type(" + (twins.indexOf(current) + 1) + ")";
        }
      }
      parts.unshift(part);
      var candidate = parts.join(" > ");
      if (isUnique(candidate)) {
        return candidate;
      }
      current = parent;
    }
    var fallback = parts.join(" > ");
    return isUnique(fallback) ? fallback : "";
  }

  function labelFor(el) {
    var label = el.tagName.toLowerCase();
    var id = el.getAttribute("id");
    if (id) {
      label += "#" + id;
    }
    var className = typeof el.className === "string" ? el.className : "";
    var classes = className.split(/\\s+/).filter(Boolean).slice(0, 3);
    for (var i = 0; i < classes.length; i += 1) {
      label += "." + classes[i];
    }
    return clamp(label, 120);
  }

  var attributes = [];
  var attrs = node.attributes || [];
  for (var i = 0; i < attrs.length && attributes.length < ATTR_CAP; i += 1) {
    var attr = attrs[i];
    if (!attr || !attr.name) {
      continue;
    }
    attributes.push({ name: clamp(attr.name, 64), value: clamp(attr.value, ATTR_VALUE_CAP) });
  }

  var isPassword = node.tagName === "INPUT" && String(node.type).toLowerCase() === "password";
  var value = "";
  if (!isPassword && typeof node.value === "string") {
    value = clamp(node.value, ATTR_VALUE_CAP);
  }

  var rect = null;
  try {
    var box = node.getBoundingClientRect();
    rect = {
      x: Math.round(box.left),
      y: Math.round(box.top),
      width: Math.round(box.width),
      height: Math.round(box.height),
    };
  } catch (_) {
    rect = null;
  }

  var text = String(node.innerText || node.textContent || "").replace(/\\s+/g, " ").trim();

  return {
    selector: selectorFor(node),
    label: labelFor(node),
    tag: node.tagName.toLowerCase(),
    text: clamp(text, TEXT_CAP),
    value: value,
    html: clamp(node.outerHTML || "", HTML_CAP),
    url: view && view.location ? String(view.location.href) : "",
    title: doc && doc.title ? clamp(doc.title, 200) : "",
    inFrame: Boolean(view && view.top && view.top !== view),
    rect: rect,
    attributes: attributes,
  };
}`;

function isPickableUrl(url) {
  const value = String(url || "");
  return PICKABLE_SCHEMES.some((scheme) => value.startsWith(scheme));
}

function clearElementPick() {
  if (!elementPick) {
    return null;
  }
  const pick = elementPick;
  elementPick = null;
  clearTimeout(pick.timeout);
  return pick;
}

async function stopInspectMode() {
  if (attachedTabId == null) {
    return;
  }
  try {
    await cdp("Overlay.setInspectMode", { mode: "none", highlightConfig: INSPECT_HIGHLIGHT });
  } catch (_) {
    // The tab may have navigated or detached while inspect mode was on.
  }
  try {
    await cdp("Overlay.disable", {});
  } catch (_) {
    // Disabling an already-gone overlay is best effort.
  }
}

async function cancelElementPick(reason, broadcast = true) {
  const pick = clearElementPick();
  if (!pick) {
    return false;
  }
  if (attachedTabId === pick.tabId) {
    await stopInspectMode();
  }
  if (broadcast) {
    broadcastRuntime({
      type: "zode-element-pick-canceled",
      reason: String(reason || "element pick canceled"),
    });
  }
  return true;
}

// Dropping the pick without touching the debugger: the tab is already gone or
// the session detached, so there is no inspect mode left to turn off.
function abandonElementPick(tabId, reason) {
  if (!elementPick || (tabId != null && elementPick.tabId !== tabId)) {
    return;
  }
  clearElementPick();
  broadcastRuntime({ type: "zode-element-pick-canceled", reason: String(reason) });
}

async function startElementPick() {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab || tab.id == null) {
    throw new Error("no active tab to pick an element from");
  }
  if (!isPickableUrl(tab.url)) {
    throw new Error("this page cannot be inspected; open a normal http(s) page first");
  }
  await cancelElementPick("element pick restarted", false);
  if (controlledTabId !== tab.id) {
    await detachAttached();
    controlledTabId = tab.id;
  }
  await attachTo(tab.id);
  await cdp("DOM.enable", {});
  await cdp("Overlay.enable", {});
  await cdp("Overlay.setInspectMode", {
    mode: "searchForNode",
    highlightConfig: INSPECT_HIGHLIGHT,
  });
  elementPick = {
    tabId: tab.id,
    timeout: setTimeout(() => {
      void cancelElementPick("element pick timed out").catch(() => {});
    }, ELEMENT_PICK_TIMEOUT_MS),
  };
  return { tabId: String(tab.id), url: tab.url || "", title: tab.title || "" };
}

async function describePickedNode(backendNodeId) {
  if (!Number.isInteger(backendNodeId)) {
    throw new Error("element pick returned no node");
  }
  const resolved = await cdp("DOM.resolveNode", { backendNodeId });
  const objectId = resolved && resolved.object ? resolved.object.objectId : null;
  if (!objectId) {
    throw new Error("the picked element could not be resolved");
  }
  try {
    const evaluated = await cdp("Runtime.callFunctionOn", {
      objectId,
      functionDeclaration: DESCRIBE_ELEMENT_FN,
      returnByValue: true,
    });
    if (evaluated && evaluated.exceptionDetails) {
      throw new Error("the picked element could not be described");
    }
    const value = evaluated && evaluated.result ? evaluated.result.value : null;
    if (!value || typeof value !== "object") {
      throw new Error("the picked element could not be described");
    }
    return value;
  } finally {
    try {
      await cdp("Runtime.releaseObject", { objectId });
    } catch (_) {
      // Releasing a handle from a navigated page is best effort.
    }
  }
}

async function completeElementPick(params) {
  const pick = clearElementPick();
  if (!pick) {
    return;
  }
  await stopInspectMode();
  try {
    const element = await describePickedNode(params && params.backendNodeId);
    broadcastRuntime({ type: "zode-element-picked", element });
  } catch (error) {
    broadcastRuntime({
      type: "zode-element-pick-canceled",
      reason: String((error && error.message) || error),
    });
  }
}

function toTabInfo(tab) {
  return {
    id: String(tab.id),
    url: tab.url || "",
    title: tab.title || "",
    active: Boolean(tab.active),
  };
}

function tail(values, limit) {
  return values.slice(-Math.max(0, limit));
}

function push(buf, entry) {
  buf.push(entry);
  while (buf.length > BUF_CAP) {
    buf.shift();
  }
}

function downloadStatus(state, error) {
  if (state === "complete") {
    return "complete";
  }
  if (state === "interrupted") {
    return error === "USER_CANCELED" ? "canceled" : "interrupted";
  }
  return "in_progress";
}

function downloadAttribution(item) {
  if (item.tabId != null && item.tabId === controlledTabId) {
    return "controlled_tab";
  }
  return item.tabId != null ? "profile" : "unknown";
}

function downloadEntry(item) {
  const error = item.error || null;
  const status = downloadStatus(item.state, error);
  return {
    id: item.id,
    status,
    path: status === "complete" && item.filename ? item.filename : null,
    url: item.finalUrl || item.url || "",
    received_bytes: Math.max(0, Number(item.bytesReceived) || 0),
    total_bytes: Math.max(0, Number(item.totalBytes) || 0),
    error,
    attribution: downloadAttribution(item),
  };
}

if (chrome.downloads && chrome.downloads.onCreated) {
  chrome.downloads.onCreated.addListener((item) => {
    if (!downloadSessionActive) {
      return;
    }
    push(downloadBuf, downloadEntry(item));
  });
  chrome.downloads.onChanged.addListener((delta) => {
    if (!downloadSessionActive) {
      return;
    }
    const entry = downloadBuf.find((candidate) => candidate.id === delta.id);
    if (!entry) {
      return;
    }
    const current = (field) => (delta[field] ? delta[field].current : undefined);
    const error = current("error");
    if (error !== undefined) {
      entry.error = error || null;
    }
    const state = current("state");
    if (state !== undefined) {
      entry.status = downloadStatus(state, entry.error);
    }
    const filename = current("filename");
    if (entry.status === "complete" && filename) {
      entry.path = filename;
    }
    const received = current("bytesReceived");
    if (received !== undefined) {
      entry.received_bytes = Math.max(0, Number(received) || 0);
    }
    const total = current("totalBytes");
    if (total !== undefined) {
      entry.total_bytes = Math.max(0, Number(total) || 0);
    }
  });
}

async function groupZodeTab(tab) {
  if (!tab || tab.id == null || !chrome.tabs.group || !chrome.tabGroups) {
    return;
  }
  if (zodeGroupId != null && (await addTabToGroup(tab.id, zodeGroupId))) {
    return;
  }
  const existingGroupId = await findZodeGroup(tab.windowId);
  if (existingGroupId != null && (await addTabToGroup(tab.id, existingGroupId))) {
    return;
  }
  try {
    zodeGroupId = await chrome.tabs.group({ tabIds: [tab.id] });
    await chrome.tabGroups.update(zodeGroupId, {
      title: ZODE_TAB_GROUP_TITLE,
      color: ZODE_TAB_GROUP_COLOR,
    });
  } catch (error) {
    zodeGroupId = null;
    console.debug("zode tab grouping failed", error);
  }
}

async function addTabToGroup(tabId, groupId) {
  try {
    await chrome.tabs.group({ groupId, tabIds: [tabId] });
    zodeGroupId = groupId;
    return true;
  } catch (_) {
    zodeGroupId = null;
    return false;
  }
}

async function findZodeGroup(windowId) {
  try {
    const groups = await chrome.tabGroups.query({
      windowId,
      title: ZODE_TAB_GROUP_TITLE,
    });
    return groups.length > 0 ? groups[0].id : null;
  } catch (_) {
    return null;
  }
}

chrome.debugger.onDetach.addListener((source, reason) => {
  if (source.tabId === attachedTabId) {
    attachedTabId = null;
  }
  // "Cancel" on Chrome's debugging bar means "stop driving my browser".
  if (reason === "canceled_by_user" && source.tabId === controlledTabId) {
    controlledTabId = null;
  }
  abandonElementPick(source.tabId, "debugger detached from the page");
});

chrome.debugger.onEvent.addListener((source, method, params) => {
  if (source.tabId !== attachedTabId) {
    return;
  }
  if (method === "Overlay.inspectNodeRequested") {
    void completeElementPick(params);
  } else if (method === "Runtime.consoleAPICalled") {
    push(consoleBuf, {
      level: params.type || "log",
      text: (params.args || []).map(argText).join(" "),
    });
  } else if (method === "Network.requestWillBeSent") {
    rememberRequest(params.requestId, {
      method: params.request && params.request.method ? params.request.method : "",
      url: params.request && params.request.url ? params.request.url : "",
    });
  } else if (method === "Network.responseReceived") {
    const request = pendingRequests.get(params.requestId) || {};
    pendingRequests.delete(params.requestId);
    push(networkBuf, {
      method: request.method || "",
      url: params.response.url || request.url || "",
      status: params.response.status || null,
      mime: params.response.mimeType || null,
    });
  } else if (method === "Network.loadingFailed") {
    pendingRequests.delete(params.requestId);
  }
});

function rememberRequest(requestId, request) {
  if (!requestId) {
    return;
  }
  pendingRequests.delete(requestId);
  pendingRequests.set(requestId, request);
  while (pendingRequests.size > PENDING_REQUEST_CAP) {
    const oldest = pendingRequests.keys().next().value;
    pendingRequests.delete(oldest);
  }
}

chrome.tabs.onRemoved.addListener((tabId) => {
  if (tabId === attachedTabId) {
    attachedTabId = null;
  }
  if (tabId === controlledTabId) {
    controlledTabId = null;
  }
  abandonElementPick(tabId, "the page was closed");
});

const HUMAN_TRANSITIONS = new Set(["typed", "auto_bookmark", "keyword", "generated"]);

function isHumanNavigation(details) {
  return (
    details.frameId === 0 &&
    (HUMAN_TRANSITIONS.has(details.transitionType) ||
      (details.transitionQualifiers || []).includes("from_address_bar"))
  );
}

if (chrome.webNavigation && chrome.webNavigation.onCommitted) {
  chrome.webNavigation.onCommitted.addListener((details) => {
    if (details.frameId === 0) {
      abandonElementPick(details.tabId, "the page navigated");
    }
    if (details.tabId === controlledTabId && isHumanNavigation(details)) {
      controlledTabId = null;
      detachAttached();
    }
  });
}

if (chrome.tabGroups && chrome.tabGroups.onRemoved) {
  chrome.tabGroups.onRemoved.addListener((group) => {
    if (group.id === zodeGroupId) {
      zodeGroupId = null;
    }
  });
}

function argText(arg) {
  if (arg.value != null) {
    return String(arg.value);
  }
  if (arg.description != null) {
    return String(arg.description);
  }
  if (arg.unserializableValue != null) {
    return String(arg.unserializableValue);
  }
  return "";
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (!message) {
    return false;
  }
  if (message.type === "zode-status") {
    connectionStatus()
      .then((status) => sendResponse({ ok: true, status }))
      .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
    return true;
  }
  if (message.type === "zode-pair") {
    connect(message.port, message.code || "")
      .then((status) => sendResponse({ ok: true, status }))
      .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
    return true;
  }
  if (message.type === "zode-reconnect") {
    reconnectOrStart(message.port)
      .then((status) => sendResponse({ ok: true, status }))
      .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
    return true;
  }
  if (message.type === "zode-task-request") {
    const request =
      message.method === "turn/start"
        ? targetActivePageForSidePanelTurn().then(() => requestTask(message.method, message.params))
        : requestTask(message.method, message.params);
    request
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => {
        const response = { ok: false, error: String(error.message || error) };
        if (error && error.code != null) {
          response.code = error.code;
        }
        sendResponse(response);
      });
    return true;
  }
  if (message.type === "zode-pick-element") {
    startElementPick()
      .then((target) => sendResponse({ ok: true, target }))
      .catch((error) => {
        void cancelElementPick("element pick failed", false).catch(() => {});
        sendResponse({ ok: false, error: String((error && error.message) || error) });
      });
    return true;
  }
  if (message.type === "zode-pick-cancel") {
    cancelElementPick("element pick canceled")
      .then((canceled) => sendResponse({ ok: true, canceled }))
      .catch((error) => sendResponse({ ok: false, error: String((error && error.message) || error) }));
    return true;
  }
  if (message.type === "zode-theme") {
    applyThemeIcon(Boolean(message.dark));
    return false;
  }
  return false;
});

enableActionOpenedSidePanel();
enableSidePanelCommand();
ensureThemeWatcher();
