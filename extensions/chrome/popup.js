const portInput = document.getElementById("port");
const codeInput = document.getElementById("code");
const button = document.getElementById("go");
const formStatus = document.getElementById("form-status");
const query = new URLSearchParams(window.location.search);

hydrateFromQuery();

button.addEventListener("click", async () => {
  await connectFromInputs();
});

if (query.get("connect") === "1") {
  queueMicrotask(() => {
    connectFromInputs().catch(() => {});
  });
} else {
  refreshPairing().catch(() => {
    renderStatus("Not connected");
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

async function refreshPairing() {
  const response = await chrome.runtime.sendMessage({ type: "zode-status" });
  if (response && response.ok && response.status && response.status.connected) {
    await closeSelf();
    return;
  }
  if (response && response.ok && response.status && response.status.canReconnect) {
    await reconnectStored(response.status);
    return;
  }
  renderStatus("Not connected");
}

async function reconnectStored(status) {
  renderStatus("Connecting...");
  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({
      type: "zode-reconnect",
      port: status.port,
    });
    if (!response || !response.ok || !response.status || !response.status.connected) {
      throw new Error((response && response.error) || "reconnect failed");
    }
    await closeSelf();
  } catch (error) {
    renderStatus(String(error.message || error));
    button.disabled = false;
  }
}

function renderStatus(message) {
  formStatus.textContent = message;
}

async function connectFromInputs() {
  const port = Number(portInput.value.trim());
  const code = codeInput.value.trim();
  renderStatus("Connecting...");
  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({ type: "zode-pair", port, code });
    if (!response || !response.ok) {
      throw new Error((response && response.error) || "pairing failed");
    }
    await closeSelf();
  } catch (error) {
    renderStatus(String(error.message || error));
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
    // Pairing opened in a popup has no tab; window.close handles that case.
  }
  window.close();
}
