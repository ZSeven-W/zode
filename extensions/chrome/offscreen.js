// zode browser bridge - offscreen theme watcher.
// Posts the current light/dark appearance to the service worker on load,
// whenever it changes, and when the worker asks after a restart.

const themeQuery = window.matchMedia("(prefers-color-scheme: dark)");

function postTheme() {
  chrome.runtime.sendMessage({ type: "zode-theme", dark: themeQuery.matches }).catch(() => {
    // The worker may be mid-restart; the next event or ping resyncs.
  });
}

themeQuery.addEventListener("change", postTheme);

chrome.runtime.onMessage.addListener((message) => {
  if (message && message.type === "zode-theme-ping") {
    postTheme();
  }
  return false;
});

postTheme();
