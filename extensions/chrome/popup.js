const portInput = document.getElementById("port");
const codeInput = document.getElementById("code");
const button = document.getElementById("go");
const statusText = document.getElementById("status");

button.addEventListener("click", async () => {
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
    window.close();
  } catch (error) {
    statusText.textContent = String(error.message || error);
    button.disabled = false;
  }
});
