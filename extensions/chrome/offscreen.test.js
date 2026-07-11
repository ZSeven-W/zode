#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const offscreenPath = path.join(__dirname, "offscreen.js");
const offscreenSource = fs.readFileSync(offscreenPath, "utf8");

function makeHarness({ dark }) {
  const sent = [];
  const runtimeListeners = [];
  const query = {
    matches: dark,
    listeners: [],
    addEventListener(type, listener) {
      if (type === "change") {
        this.listeners.push(listener);
      }
    },
    fireChange(matches) {
      this.matches = matches;
      this.listeners.forEach((listener) => listener({ matches }));
    },
  };
  const sandbox = {
    console,
    window: {
      matchMedia: (q) => {
        assert.equal(q, "(prefers-color-scheme: dark)");
        return query;
      },
    },
    chrome: {
      runtime: {
        sendMessage: (message) => {
          sent.push(JSON.parse(JSON.stringify(message)));
          return Promise.resolve();
        },
        onMessage: { addListener: (listener) => runtimeListeners.push(listener) },
      },
    },
  };
  vm.runInNewContext(offscreenSource, sandbox, { filename: offscreenPath });
  return { sent, runtimeListeners, query };
}

function testPostsInitialThemeOnLoad() {
  const h = makeHarness({ dark: true });
  assert.deepEqual(h.sent, [{ type: "zode-theme", dark: true }]);
}

function testRepostsOnMediaQueryChange() {
  const h = makeHarness({ dark: true });
  h.query.fireChange(false);
  assert.deepEqual(h.sent.at(-1), { type: "zode-theme", dark: false });
  assert.equal(h.sent.length, 2);
}

function testRepostsOnPing() {
  const h = makeHarness({ dark: false });
  const result = h.runtimeListeners[0]({ type: "zode-theme-ping" }, {}, () => {});
  assert.equal(result, false);
  assert.deepEqual(h.sent.at(-1), { type: "zode-theme", dark: false });
  assert.equal(h.sent.length, 2);
}

testPostsInitialThemeOnLoad();
testRepostsOnMediaQueryChange();
testRepostsOnPing();
console.log("offscreen tests passed");
