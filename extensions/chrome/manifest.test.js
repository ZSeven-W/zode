#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const extensionDir = __dirname;
const manifestPath = path.join(extensionDir, "manifest.json");
const packScriptPath = path.join(extensionDir, "pack.sh");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const packScript = fs.readFileSync(packScriptPath, "utf8");

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

function testPackScriptCopiesDeclaredIconDirectory() {
  const iconPaths = Object.values(manifest.icons || {});
  if (iconPaths.some((iconPath) => iconPath.startsWith("icons/"))) {
    assert.match(packScript, /cp\s+-R\s+"\$dir\/icons"\s+"\$pack_dir\/"/);
  }
}

function testManifestDeclaresRequiredPermissions() {
  assert.deepEqual(manifest.permissions, [
    "debugger",
    "tabs",
    "storage",
    "tabGroups",
    "webNavigation",
  ]);
}

testExtensionIconsAreDeclaredAndSized();
testManifestDeclaresRequiredPermissions();
testPackScriptCopiesDeclaredIconDirectory();
console.log("manifest tests passed");
