const connectedView = document.getElementById("connected-view");
const connectView = document.getElementById("connect-view");
const statusText = document.getElementById("status-text");
const statusDetail = document.getElementById("status-detail");
const versionText = document.getElementById("version");
const portInput = document.getElementById("port");
const codeInput = document.getElementById("code");
const button = document.getElementById("go");
const formStatus = document.getElementById("form-status");
const query = new URLSearchParams(window.location.search);

hydrateFromQuery();
renderVersion();

button.addEventListener("click", async () => {
  await connectFromInputs({ closeOnSuccess: true });
});

if (query.get("connect") === "1") {
  queueMicrotask(() => {
    connectFromInputs({ closeOnSuccess: true }).catch(() => {});
  });
} else {
  refreshStatus().catch(() => {
    renderDisconnected("Not connected");
  });
}

function hydrateFromQuery() {
  const port = query.get("port");
  const code = query.get("code");
  if (port) {
    portInput.value = port;
  }
  if (code) {
    codeInput.value = code;
  }
}

function renderVersion() {
  const manifest = chrome.runtime.getManifest ? chrome.runtime.getManifest() : null;
  versionText.textContent = manifest && manifest.version ? `Version v${manifest.version}` : "";
}

async function refreshStatus() {
  const response = await chrome.runtime.sendMessage({ type: "zode-status" });
  if (response && response.ok && response.status && response.status.connected) {
    renderConnected(response.status);
  } else if (response && response.ok && response.status && response.status.canReconnect) {
    await reconnectStored(response.status);
  } else {
    renderDisconnected("Not connected");
  }
}

async function reconnectStored(status) {
  renderDisconnected("Connecting...");
  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({
      type: "zode-reconnect",
      port: status.port,
    });
    if (response && response.ok && response.status && response.status.connected) {
      renderConnected(response.status);
    } else {
      renderDisconnected("Not connected");
      button.disabled = false;
    }
  } catch (_) {
    renderDisconnected("Not connected");
    button.disabled = false;
  }
}

function renderConnected(status) {
  connectedView.hidden = false;
  connectView.hidden = true;
  statusText.textContent = "Connected";
  statusDetail.textContent = status.port ? `Port ${status.port}` : "";
  document.body.classList.add("connected");
}

function renderDisconnected(message) {
  connectedView.hidden = true;
  connectView.hidden = false;
  formStatus.textContent = message;
  document.body.classList.remove("connected");
}

async function connectFromInputs({ closeOnSuccess }) {
  const port = Number(portInput.value.trim());
  const code = codeInput.value.trim();
  renderDisconnected("Connecting...");
  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({ type: "zode-pair", port, code });
    if (!response || !response.ok) {
      throw new Error((response && response.error) || "pairing failed");
    }
    renderConnected(response.status || { connected: true, port });
    if (closeOnSuccess) {
      await closeSelf();
    }
  } catch (error) {
    renderDisconnected(String(error.message || error));
    button.disabled = false;
  }
}

async function closeSelf() {
  try {
    const tab = await chrome.tabs.getCurrent();
    if (tab && tab.id != null) {
      await chrome.tabs.remove(tab.id);
      return;
    }
  } catch (_) {
    // Browser action popups have no tab; window.close handles that case.
  }
  window.close();
}
