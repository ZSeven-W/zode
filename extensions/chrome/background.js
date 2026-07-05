// zode browser bridge - MV3 service worker.
// Connects to the local zode BridgeServer, authenticates with a pairing code
// or stored token, and relays RPC frames to chrome.debugger / chrome.tabs.

const STORAGE_TOKEN = "zodeToken";
const STORAGE_PORT = "zodePort";
const KEEPALIVE_MS = 20_000;
const RECONNECT_MS = 1_000;
const BUF_CAP = 500;

let ws = null;
let keepalive = null;
let reconnectTimer = null;
let lastPort = null;
let attachedTabId = null;
let consoleBuf = [];
let networkBuf = [];
let pendingRequests = new Map();

async function getStored(keys) {
  return await chrome.storage.local.get(keys);
}

async function getToken() {
  const data = await getStored(STORAGE_TOKEN);
  return data[STORAGE_TOKEN] || null;
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

async function getPort() {
  if (lastPort != null) {
    return lastPort;
  }
  const data = await getStored(STORAGE_PORT);
  lastPort = Number(data[STORAGE_PORT]) || null;
  return lastPort;
}

function status() {
  return {
    connected: ws != null && ws.readyState === WebSocket.OPEN,
    port: lastPort,
    attachedTabId,
  };
}

async function connect(port, code) {
  const numericPort = Number(port);
  if (!Number.isInteger(numericPort) || numericPort <= 0 || numericPort > 65535) {
    throw new Error("enter the WS port shown by /browser pair");
  }
  const token = await getToken();
  if (!code && !token) {
    throw new Error("enter the pairing code shown by /browser pair");
  }
  await setPort(numericPort);
  clearReconnect();
  closeSocket();

  ws = new WebSocket(`ws://127.0.0.1:${numericPort}`);
  ws.onopen = async () => {
    try {
      if (code) {
        send({ type: "pair", code: String(code) });
      } else if (token) {
        send({ type: "auth", token });
      }
      keepalive = setInterval(() => {
        try {
          send({ type: "ping" });
        } catch (_) {
          // The close handler schedules reconnects.
        }
      }, KEEPALIVE_MS);
    } catch (error) {
      console.error("zode bridge open failed", error);
      closeSocket();
    }
  };
  ws.onmessage = (event) => {
    handleMessage(event.data).catch((error) => {
      console.error("zode bridge message failed", error);
    });
  };
  ws.onerror = () => {};
  ws.onclose = () => {
    clearInterval(keepalive);
    keepalive = null;
    ws = null;
    scheduleReconnect();
  };
}

function closeSocket() {
  if (ws != null) {
    const old = ws;
    ws = null;
    old.onclose = null;
    try {
      old.close();
    } catch (_) {
      // Closing a half-open socket is best effort.
    }
  }
  clearInterval(keepalive);
  keepalive = null;
}

function clearReconnect() {
  if (reconnectTimer != null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

function scheduleReconnect() {
  clearReconnect();
  reconnectTimer = setTimeout(async () => {
    reconnectTimer = null;
    const token = await getToken();
    const port = await getPort();
    if (token && port) {
      connect(port, "").catch((error) => console.error("zode bridge reconnect failed", error));
    }
  }, RECONNECT_MS);
}

function send(value) {
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    throw new Error("zode bridge is not connected");
  }
  ws.send(JSON.stringify(value));
}

async function handleMessage(raw) {
  const msg = JSON.parse(raw);
  if (msg.type === "paired") {
    await setToken(msg.token);
    return;
  }
  if (msg.type === "ok" || msg.type === "ping") {
    return;
  }
  if (msg.type === "rejected") {
    await clearToken();
    throw new Error(msg.reason || "zode bridge rejected authentication");
  }
  await handleRpc(msg);
}

async function handleRpc(request) {
  try {
    const result = await dispatch(request);
    send({ id: request.id, result });
  } catch (error) {
    send({ id: request.id, error: String((error && error.message) || error) });
  }
}

async function dispatch(request) {
  if (request.kind === "cdp") {
    await ensureAttached();
    return await cdp(request.method, request.params || {});
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
      return toTabInfo(tab);
    }
    case "tabs.close": {
      await chrome.tabs.remove(Number(params.id));
      if (attachedTabId === Number(params.id)) {
        attachedTabId = null;
      }
      return null;
    }
    case "tabs.select": {
      const tabId = Number(params.id);
      await chrome.tabs.update(tabId, { active: true });
      attachedTabId = null;
      await ensureAttached();
      return null;
    }
    case "tabs.current": {
      const tab = await activeTab();
      return tab.url || "";
    }
    case "logs.console":
      return tail(consoleBuf, Number(params.limit) || 100);
    case "logs.network":
      return tail(networkBuf, Number(params.limit) || 100);
    case "debugger.detach":
      await detachAttached();
      return null;
    default:
      throw new Error(`unknown ext method ${request.method}`);
  }
}

async function activeTab() {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  const tab = tabs[0];
  if (!tab || tab.id == null) {
    throw new Error("no active Chrome tab");
  }
  return tab;
}

async function ensureAttached() {
  const tab = await activeTab();
  if (attachedTabId !== tab.id) {
    await detachAttached();
    await chrome.debugger.attach({ tabId: tab.id }, "1.3");
    attachedTabId = tab.id;
    await cdp("Runtime.enable", {});
    await cdp("Network.enable", {});
    await cdp("Page.enable", {});
  }
  return tab;
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

chrome.debugger.onDetach.addListener((source) => {
  if (source.tabId === attachedTabId) {
    attachedTabId = null;
  }
});

chrome.debugger.onEvent.addListener((source, method, params) => {
  if (source.tabId !== attachedTabId) {
    return;
  }
  if (method === "Runtime.consoleAPICalled") {
    push(consoleBuf, {
      level: params.type || "log",
      text: (params.args || []).map(argText).join(" "),
    });
  } else if (method === "Network.requestWillBeSent") {
    pendingRequests.set(params.requestId, {
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
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  if (tabId === attachedTabId) {
    attachedTabId = null;
  }
});

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
  if (!message || message.type !== "zode-pair") {
    return false;
  }
  connect(message.port, message.code || "")
    .then(() => sendResponse({ ok: true, status: status() }))
    .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
  return true;
});

getPort().catch(() => {});
