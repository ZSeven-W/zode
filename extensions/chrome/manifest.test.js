#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const extensionDir = __dirname;
const manifestPath = path.join(extensionDir, "manifest.json");
const packScriptPath = path.join(extensionDir, "pack.sh");
const extensionReadmePath = path.join(extensionDir, "README.md");
const rootReadmePath = path.join(extensionDir, "..", "..", "README.md");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const packScript = fs.readFileSync(packScriptPath, "utf8");
const extensionReadme = fs.readFileSync(extensionReadmePath, "utf8");
const rootReadme = fs.readFileSync(rootReadmePath, "utf8");
const packageJson = JSON.parse(
  fs.readFileSync(path.join(extensionDir, "package.json"), "utf8"),
);
const workspaceCargo = fs.readFileSync(
  path.join(extensionDir, "..", "..", "Cargo.toml"),
  "utf8",
);
const zodeVersion = /\[workspace\.package\][\s\S]*?\nversion = "([^"]+)"/.exec(
  workspaceCargo,
)[1];
const expectedKey =
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAq75hQkQsowKh9E3NxJQ0BwYW1dxTQj0f76xEOgtIAaJIK/+OXGCBpaVREH/HMHf741lyW4SM5Ltz59R5trKVnraaeUEd+jbJxyfhyScejezm6HhRMgQ+r38rcJov3DG5m97KFhjlNncMYBpEREyapLPPoqTihQq7BMwemzdMlKGfkCNFS0DH7MnM3l1F22LMonwKkLWDAqMPLn0Xx8vs+/SufrAHwpe3kE9r7znUnOt2aNL+1BGsTWuI0H3V3ezo12FNrTUT9LZiYXd9bIKadl1XZMnVywM1khkbhcTYSBjNzk45QQrSjDhwmCMdPILmaaIYZH94wvOSBzxUPyecdQIDAQAB";

function readPngSize(relativePath) {
  const filePath = path.join(extensionDir, relativePath);
  const buffer = fs.readFileSync(filePath);
  assert.equal(buffer.toString("ascii", 1, 4), "PNG", `${relativePath} must be a PNG`);
  return {
    width: buffer.readUInt32BE(16),
    height: buffer.readUInt32BE(20),
  };
}

function testExtensionIconsAreDeclaredAndSized() {
  const expected = {
    16: "icons/zode-16.png",
    32: "icons/zode-32.png",
    48: "icons/zode-48.png",
    128: "icons/zode-128.png",
  };

  assert.deepEqual(manifest.icons, expected);
  assert.deepEqual(manifest.action.default_icon, expected);

  for (const [size, relativePath] of Object.entries(expected)) {
    const dimensions = readPngSize(relativePath);
    assert.deepEqual(dimensions, { width: Number(size), height: Number(size) });
  }
}

function testLightIconVariantsExistAndSized() {
  for (const size of [16, 32, 48, 128]) {
    const dimensions = readPngSize(`icons/zode-light-${size}.png`);
    assert.deepEqual(dimensions, { width: size, height: size });
  }
}

function testPackScriptCopiesDeclaredIconDirectory() {
  const iconPaths = Object.values(manifest.icons || {});
  if (iconPaths.some((iconPath) => iconPath.startsWith("icons/"))) {
    assert.match(packScript, /cp\s+-R\s+"\$dir\/icons"\s+"\$pack_dir\/"/);
  }
}

function testPackScriptCopiesOffscreenFiles() {
  assert.match(packScript, /offscreen\.html/);
  assert.match(packScript, /offscreen\.js/);
}

function testPackScriptCopiesEverySidePanelAsset() {
  for (const asset of [
    "sidepanel.html",
    "sidepanel-state.js",
    "sidepanel.js",
  ]) {
    assert.match(
      packScript,
      new RegExp(`\\"\\$dir/${asset.replaceAll(".", "\\.")}\\"`),
      `pack.sh must copy ${asset}`,
    );
  }
  assert.match(packScript, /cp\s+-R\s+"\$dir\/dist"\s+"\$pack_dir\/"/);
}

function testManifestDeclaresRequiredPermissions() {
  assert.deepEqual(manifest.permissions, [
    "debugger",
    "tabs",
    "storage",
    "tabGroups",
    "webNavigation",
    "offscreen",
    "downloads",
    "sidePanel",
    "nativeMessaging",
  ]);
}

function testNativeMessagingAutostartContract() {
  const background = fs.readFileSync(path.join(extensionDir, "background.js"), "utf8");
  assert.match(background, /ai\.zode\.browser_bridge/);
  assert.match(background, /chrome\.runtime\.connectNative\(NATIVE_HOST_NAME\)/);
  assert.match(background, /reconnectOrStart\(message\.port\)/);
}

// The extension ships with zode, so its version follows the workspace crate
// version. Chrome only accepts dotted integers in `version`, so a prerelease
// tag such as `0.1.0-beta.9` lives in `version_name` and `version` carries the
// numeric core.
function testExtensionVersionTracksTheZodeVersion() {
  assert.match(zodeVersion, /^\d+\.\d+\.\d+/, "workspace version is unreadable");
  const numericCore = /^(\d+(?:\.\d+){0,3})/.exec(zodeVersion)[1];
  assert.equal(packageJson.version, zodeVersion);
  assert.equal(manifest.version, numericCore);
  assert.match(manifest.version, /^\d+(\.\d+){0,3}$/, "Chrome rejects non-numeric versions");
  if (zodeVersion === numericCore) {
    assert.equal(Object.hasOwn(manifest, "version_name"), false);
  } else {
    assert.equal(manifest.version_name, zodeVersion);
  }
}

function testManifestUsesNativeActionOpenedSidePanel() {
  assert.equal(manifest.minimum_chrome_version, "116");
  assert.deepEqual(manifest.side_panel, { default_path: "sidepanel.html" });
  assert.equal(Object.hasOwn(manifest.action, "default_popup"), false);
  assert.equal(manifest.action.default_title, "zode bridge");
  assert.equal(manifest.key, expectedKey);
  assert.equal(fs.existsSync(path.join(extensionDir, "popup.html")), true);
  assert.equal(fs.existsSync(path.join(extensionDir, "popup.js")), true);
}

function testTaskSidePanelDocumentationCoversOperationAndCompatibility() {
  for (const readme of [extensionReadme, rootReadme]) {
    assert.match(readme, /\/browser pair/i);
    assert.match(readme, /toolbar (?:icon|button)[\s\S]{0,100}side panel/i);
    assert.match(readme, /start(?:s|ed)? zode automatically|auto-start/i);
    assert.match(readme, /shared[\s-]+(?:with|TUI)[\s\S]{0,80}(?:TUI )?sessions?/i);
    assert.match(readme, /`readOnly`[\s\S]{0,120}`prompt`[\s\S]{0,120}`auto`/i);
    assert.match(readme, /stop/i);
    assert.match(readme, /8 files/i);
    assert.match(readme, /5 MiB/i);
    assert.match(readme, /1 MiB/i);
    assert.match(readme, /20 MiB/i);
    assert.match(readme, /PNG[\s\S]{0,80}JPEG[\s\S]{0,80}GIF[\s\S]{0,80}WebP/i);
    assert.match(readme, /UTF-8 (?:text|code)/i);
    assert.match(readme, /reload/i);
    assert.match(
      readme,
      /older\s+extension|old\s+extension|older\s+version|previous\s+version/i,
    );
  }
  assert.match(extensionReadme, /Windows[\s\S]{0,400}Chrome directly/i);
  assert.match(extensionReadme, /Microsoft Store/i);
}

function testSidePanelShellIsAccessibleAndCspSafe() {
  const assetPaths = [
    "sidepanel.html",
    "dist/sidepanel-react.css",
    "dist/sidepanel-react.js",
    "sidepanel-state.js",
    "sidepanel.js",
  ];
  for (const asset of assetPaths) {
    assert.equal(fs.existsSync(path.join(extensionDir, asset)), true, `${asset} must exist`);
  }

  const html = fs.readFileSync(path.join(extensionDir, "sidepanel.html"), "utf8");
  const appSource = fs.readFileSync(path.join(extensionDir, "src", "App.tsx"), "utf8");
  const reactStyles = fs.readFileSync(path.join(extensionDir, "src", "styles.css"), "utf8");
  const reactBundle = fs.readFileSync(
    path.join(extensionDir, "dist", "sidepanel-react.js"),
    "utf8",
  );
  const stateSource = fs.readFileSync(path.join(extensionDir, "sidepanel-state.js"), "utf8");
  const panelSource = fs.readFileSync(path.join(extensionDir, "sidepanel.js"), "utf8");
  const scriptTags = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)];
  const scriptSources = scriptTags.map((match) => {
    assert.equal(match[2].trim(), "", "side panel scripts must not contain inline code");
    const source = match[1].match(/\bsrc=["']([^"']+)["']/i);
    assert.notEqual(source, null, "every side panel script must use src");
    return source[1];
  });

  assert.match(
    html,
    /<link\b[^>]*rel=["']stylesheet["'][^>]*href=["']dist\/sidepanel-react\.css["']/i,
  );
  assert.deepEqual(scriptSources, [
    "sidepanel-state.js",
    "sidepanel.js",
    "dist/sidepanel-react.js",
  ]);
  assert.doesNotMatch(html, /<style\b/i);
  assert.doesNotMatch(html, /\sstyle\s*=/i);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
  assert.doesNotMatch(html, /(?:src|href)=["'](?:https?:)?\/\//i);
  assert.match(html, /<div\b[^>]*id=["']zode-react-root["']/i);
  assert.match(appSource, /<h1\b[^>]*id=["'][^"']+["']/i);
  assert.match(appSource, /<main\b[^>]*aria-labelledby=["'][^"']+["']/i);
  assert.match(appSource, /\brole=["']status["']/i);
  assert.match(appSource, /\baria-live=["']polite["']/i);
  assert.doesNotMatch(appSource, /dangerouslySetInnerHTML/);
  for (const token of [
    "background",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-foreground",
    "secondary",
    "secondary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    "border",
    "input",
    "ring",
    "chart-1",
    "sidebar",
    "sidebar-foreground",
  ]) {
    assert.match(reactStyles, new RegExp(`--${token}:`), `missing shadcn token --${token}`);
  }
  assert.doesNotMatch(reactBundle, /\bprocess\.env\b/);
  assert.match(stateSource, /globalThis\.ZodePanelState\s*=/);
  assert.match(panelSource, /type:\s*["']zode-status["']/);
  for (const source of [stateSource, panelSource]) {
    assert.doesNotMatch(source, /\b(?:eval|Function)\s*\(/);
    assert.doesNotMatch(source, /\.innerHTML\b/);
  }
}

async function testSidePanelStatusRequestFailuresRenderDisconnectedState() {
  const panelSource = fs.readFileSync(path.join(extensionDir, "sidepanel.js"), "utf8");
  const cases = [
    {
      name: "synchronous throw",
      sendMessage() {
        throw new Error("runtime unavailable");
      },
    },
    {
      name: "asynchronous rejection",
      sendMessage() {
        return Promise.reject(new Error("no receiver"));
      },
    },
  ];

  for (const testCase of cases) {
    const status = { dataset: {}, textContent: "" };
    const state = {
      connection: "checking",
      message: "正在连接…",
      setConnection(connection, message) {
        this.connection = connection;
        this.message = message;
      },
    };
    const sandbox = {
      ZodePanelState: state,
      document: {
        querySelector(selector) {
          assert.equal(selector, "#connection-status");
          return status;
        },
      },
      chrome: { runtime: { sendMessage: testCase.sendMessage } },
    };

    assert.doesNotThrow(
      () => vm.runInNewContext(panelSource, sandbox, { filename: "sidepanel.js" }),
      `${testCase.name} escaped sidepanel startup`,
    );
    await new Promise((resolve) => setImmediate(resolve));

    assert.equal(status.dataset.state, "disconnected", `${testCase.name} left status checking`);
    assert.equal(status.textContent, "无法读取连接状态");
  }
}

function testManifestNameIsZodeAndDeclaresSidePanelCommand() {
  assert.equal(manifest.name, "zode");
  const command = manifest.commands && manifest.commands["open-side-panel"];
  assert.ok(command, "manifest must declare the open-side-panel command");
  assert.ok(command.description, "command needs a description for chrome://extensions/shortcuts");
  assert.ok(
    command.suggested_key && command.suggested_key.default,
    "command needs a default suggested key",
  );
}

(async () => {
  testExtensionIconsAreDeclaredAndSized();
  testLightIconVariantsExistAndSized();
  testManifestNameIsZodeAndDeclaresSidePanelCommand();
  testManifestDeclaresRequiredPermissions();
  testNativeMessagingAutostartContract();
  testExtensionVersionTracksTheZodeVersion();
  testManifestUsesNativeActionOpenedSidePanel();
  testTaskSidePanelDocumentationCoversOperationAndCompatibility();
  testPackScriptCopiesDeclaredIconDirectory();
  testPackScriptCopiesOffscreenFiles();
  testPackScriptCopiesEverySidePanelAsset();
  testSidePanelShellIsAccessibleAndCspSafe();
  await testSidePanelStatusRequestFailuresRenderDisconnectedState();
  console.log("manifest tests passed");
})();
