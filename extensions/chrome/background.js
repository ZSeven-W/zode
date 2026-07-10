// zode browser bridge - MV3 service worker.
// Connects to the local zode BridgeServer, authenticates with a pairing code
// or stored token, and relays RPC frames to chrome.debugger / chrome.tabs.

const STORAGE_TOKEN = "zodeToken";
const STORAGE_PORT = "zodePort";
const KEEPALIVE_MS = 20_000;
const BUF_CAP = 500;
const PENDING_REQUEST_CAP = 500;
const ZODE_TAB_GROUP_TITLE = "zode";
const ZODE_TAB_GROUP_COLOR = "blue";

let ws = null;
let keepalive = null;
let lastPort = null;
let attachedTabId = null;
let controlledTabId = null;
let zodeGroupId = null;
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

function statusSnapshot(port = lastPort, token = null) {
  const connected = ws != null && ws.readyState === WebSocket.OPEN;
  return {
    connected,
    port,
    attachedTabId,
    canReconnect: !connected && port != null && token != null,
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
  closeSocket();

  return await new Promise((resolve, reject) => {
    let authenticated = false;
    let settled = false;

    const resolveAuth = () => {
      authenticated = true;
      if (!settled) {
        settled = true;
        resolve(statusSnapshot(numericPort, token));
      }
    };
    const rejectAuth = (error) => {
      if (!settled) {
        settled = true;
        reject(error);
      }
    };

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
            // The close handler clears socket state.
          }
        }, KEEPALIVE_MS);
      } catch (error) {
        console.error("zode bridge open failed", error);
        rejectAuth(error);
        closeSocket();
      }
    };
    ws.onmessage = (event) => {
      handleMessage(event.data)
        .then((result) => {
          if (result === "authenticated") {
            resolveAuth();
          }
        })
        .catch((error) => {
          console.error("zode bridge message failed", error);
          rejectAuth(error);
        });
    };
    ws.onerror = () => {
      rejectAuth(new Error("zode bridge websocket error"));
    };
    ws.onclose = () => {
      clearInterval(keepalive);
      keepalive = null;
      ws = null;
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

async function reconnectStored(port) {
  const targetPort = port || (await getPort());
  if (!targetPort) {
    throw new Error("no stored zode bridge port");
  }
  return await connect(targetPort, "");
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
    return "authenticated";
  }
  if (msg.type === "ok") {
    return "authenticated";
  }
  if (msg.type === "ping") {
    return null;
  }
  if (msg.type === "rejected") {
    await clearToken();
    throw new Error(msg.reason || "zode bridge rejected authentication");
  }
  await handleRpc(msg);
  return null;
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
    await ensureControlledTab();
    let params = request.params || {};
    if (request.method === "Page.navigate") {
      // CDP defaults transitionType to "typed", which would look like a
      // human address-bar navigation to the handoff listener below.
      params = { transitionType: "link", ...params };
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

// zode never takes over a human tab: when it has no controlled tab it
// always creates a fresh background one inside the zode tab group.
async function ensureControlledTab() {
  const existing = await controlledTab();
  if (existing) {
    await attachTo(existing.id);
    return existing;
  }
  const tab = await chrome.tabs.create({ active: false });
  await groupZodeTab(tab);
  controlledTabId = tab.id;
  await attachTo(tab.id);
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
    reconnectStored(message.port)
      .then((status) => sendResponse({ ok: true, status }))
      .catch((error) => sendResponse({ ok: false, error: String(error.message || error) }));
    return true;
  }
  return false;
});
