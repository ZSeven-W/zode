const portInput = document.getElementById("port");
const codeInput = document.getElementById("code");
const button = document.getElementById("go");
const statusText = document.getElementById("status");
const query = new URLSearchParams(window.location.search);

hydrateFromQuery();

button.addEventListener("click", async () => {
  await connectFromInputs({ closeOnSuccess: true });
});

if (query.get("connect") === "1") {
  queueMicrotask(() => {
    connectFromInputs({ closeOnSuccess: true }).catch(() => {});
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

async function connectFromInputs({ closeOnSuccess }) {
  const port = Number(portInput.value.trim());
  const code = codeInput.value.trim();
  statusText.textContent = "Connecting...";
  button.disabled = true;
  try {
    const response = await chrome.runtime.sendMessage({ type: "zode-pair", port, code });
    if (!response || !response.ok) {
      throw new Error((response && response.error) || "pairing failed");
    }
    statusText.textContent = "Connected";
    if (closeOnSuccess) {
      await closeSelf();
    }
  } catch (error) {
    statusText.textContent = String(error.message || error);
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
