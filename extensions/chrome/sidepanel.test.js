#!/usr/bin/env node
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const extensionDir = __dirname;
const stateSource = fs.readFileSync(path.join(extensionDir, "sidepanel-state.js"), "utf8");
const panelSource = fs.readFileSync(path.join(extensionDir, "sidepanel.js"), "utf8");
const html = fs.readFileSync(path.join(extensionDir, "sidepanel.html"), "utf8");
const css = fs.readFileSync(path.join(extensionDir, "src", "styles.css"), "utf8");
const reactSource = fs.readFileSync(path.join(extensionDir, "src", "App.tsx"), "utf8");

function plain(value) {
  return JSON.parse(JSON.stringify(value));
}

function loadApis() {
  const sandbox = {
    console: { ...console, debug: () => {} },
    Promise,
    setTimeout,
    clearTimeout,
    btoa: (value) => Buffer.from(value, "binary").toString("base64"),
  };
  sandbox.globalThis = sandbox;
  vm.runInNewContext(stateSource, sandbox, { filename: "sidepanel-state.js" });
  vm.runInNewContext(panelSource, sandbox, { filename: "sidepanel.js" });
  return {
    State: sandbox.ZodePanelState,
    App: sandbox.ZodePanelApp,
  };
}

function applyPure(State, state, action) {
  const before = plain(state);
  const next = State.reduce(state, action);
  assert.deepEqual(plain(state), before, `${action.type} mutated its previous state`);
  assert.notEqual(next, state, `${action.type} returned its previous state reference`);
  return next;
}

function testInitialStateHasNoSharedMutableReferences(State) {
  const first = State.initialState();
  const second = State.initialState();

  for (const key of [
    "tasks",
    "models",
    "messages",
    "tools",
    "approvals",
    "collapsedToolIds",
    "errorsByTask",
    "terminalsByTask",
  ]) {
    assert.notEqual(first[key], second[key], `${key} is shared between initial states`);
  }
  assert.notEqual(first.drafts, second.drafts);
  first.tasks.push({ id: "leak" });
  first.drafts.leak = "draft";
  first.collapsedToolIds.push("tool-leak");
  assert.deepEqual(plain(second.tasks), []);
  assert.deepEqual(plain(second.drafts), {});
  assert.deepEqual(plain(second.collapsedToolIds), []);
}

function testSnapshotAuthoritativelyReplacesServerState(State) {
  const original = {
    ...State.initialState(),
    connection: "connected",
    workspace: { name: "old", path: "/old" },
    tasks: [{ id: "old", title: "old", status: "running", activeTurnId: "turn-old" }],
    currentTaskId: "old",
    models: [{ id: "old-model", label: "Old" }],
    messages: [{ id: "old-message", taskId: "old", role: "assistant", text: "garbage" }],
    tools: [{ id: "old-tool", taskId: "old", status: "running" }],
    approvals: [{ id: "old-approval", taskId: "old", status: "pending" }],
    terminal: { taskId: "old", status: "failed" },
    error: { message: "old error" },
    drafts: { old: "keep old draft", s1: "keep new draft" },
    collapsedToolIds: ["old-tool"],
  };
  const next = applyPure(State, original, {
    type: "snapshot",
    snapshot: {
      workspace: { name: "zode", path: "/workspace" },
      tasks: [{ id: "s1", title: "Inspect auth", status: "idle", model: "m1", access: "prompt" }],
      currentTaskId: "s1",
      models: ["m1", "m2"],
      messages: [{ id: "u1", taskId: "s1", role: "user", text: "inspect" }],
      tools: [],
      approvals: [],
    },
  });

  assert.deepEqual(plain(next.tasks), [
    { id: "s1", title: "Inspect auth", status: "idle", model: "m1", access: "prompt" },
  ]);
  assert.equal(next.currentTaskId, "s1");
  assert.deepEqual(plain(next.models), ["m1", "m2"]);
  assert.deepEqual(plain(next.messages), [
    { id: "u1", taskId: "s1", role: "user", text: "inspect", order: 0 },
  ]);
  assert.deepEqual(plain(next.workspace), { name: "zode", path: "/workspace" });
  assert.deepEqual(plain(next.tools), []);
  assert.deepEqual(plain(next.approvals), []);
  assert.equal(next.terminal, null);
  assert.equal(next.error, null);
  assert.deepEqual(plain(next.drafts), plain(original.drafts));
  assert.deepEqual(plain(next.collapsedToolIds), ["old-tool"]);
  assert.notEqual(next.tasks, next.messages);
}

function testReducerCoversCoreTaskLifecycleWithoutMutation(State) {
  let state = State.initialState();
  state = applyPure(State, state, {
    type: "task/created",
    params: { task: { id: "s1", title: "First task", status: "idle", model: "m1", access: "prompt" } },
  });
  state = applyPure(State, state, {
    type: "task/updated",
    params: { task: { id: "s1", title: "Renamed task" } },
  });
  assert.equal(state.tasks[0].title, "Renamed task");

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-1" },
  });
  assert.equal(State.primaryAction(state, "s1"), "stop");
  assert.equal(state.tasks[0].status, "running");

  const userMessage = {
    type: "message/added",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      messageId: "s1:turn-1:user",
      role: "user",
      text: "hello from the panel",
    },
  };
  state = applyPure(State, state, userMessage);
  state = applyPure(State, state, userMessage);
  assert.equal(
    state.messages.filter((message) => message.id === "s1:turn-1:user").length,
    1,
    "locally acknowledged user messages must be idempotent",
  );

  state = applyPure(State, state, {
    type: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      messageId: "a1",
      role: "assistant",
      delta: "hel",
    },
  });
  state = applyPure(State, state, {
    type: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      messageId: "a1",
      role: "assistant",
      delta: "lo",
    },
  });
  assert.equal(state.messages.find((message) => message.id === "a1").text, "hello");

  const beforeStale = state;
  state = applyPure(State, state, {
    type: "message/delta",
    params: { taskId: "s1", turnId: "turn-stale", messageId: "stale", delta: "ignore" },
  });
  assert.equal(state.messages.some((message) => message.id === "stale"), false);
  assert.deepEqual(plain(state.messages), plain(beforeStale.messages));

  state = applyPure(State, state, {
    type: "tool/started",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      toolId: "tool-1",
      tool: "shell",
      summary: "Run tests",
    },
  });
  assert.equal(state.tools[0].status, "running");
  state = applyPure(State, state, {
    type: "tool/completed",
    params: { taskId: "s1", turnId: "turn-1", toolId: "tool-1", failed: false },
  });
  assert.equal(state.tools[0].status, "completed");

  state = applyPure(State, state, {
    type: "approval/requested",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      approvalId: "approval-1",
      toolId: "tool-1",
      summary: "Run command",
    },
  });
  assert.equal(state.approvals[0].status, "pending");
  assert.deepEqual(
    [state.messages[0].order, state.messages[1].order, state.tools[0].order, state.approvals[0].order],
    [0, 1, 2, 3],
    "messages, tools, and approvals share one chronological sequence",
  );
  state = applyPure(State, state, {
    type: "approval/resolved",
    params: {
      taskId: "s1",
      turnId: "turn-1",
      approvalId: "approval-1",
      decision: "allow",
    },
  });
  assert.equal(state.approvals[0].status, "resolved");
  assert.equal(state.approvals[0].decision, "allow");

  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-1", status: "completed" },
  });
  assert.equal(state.tasks[0].status, "idle");
  assert.equal(State.primaryAction(state, "s1"), "send");
  assert.equal(state.terminal.status, "completed");

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-2" },
  });
  state = applyPure(State, state, {
    type: "turn/error",
    params: { taskId: "s1", turnId: "turn-2", code: "provider_error", message: "provider failed" },
  });
  assert.equal(state.tasks[0].status, "error");
  assert.equal(state.error.message, "provider failed");

  state = applyPure(State, { ...state, drafts: { s1: "unsent" }, currentTaskId: "s1" }, {
    type: "disconnected",
    message: "Start zode",
  });
  assert.equal(state.connection, "disconnected");
  assert.equal(state.currentTaskId, "s1");
  assert.equal(state.drafts.s1, "unsent");
}

function testPrimaryActionTreatsStoppingAsStop(State) {
  const state = {
    ...State.initialState(),
    tasks: [
      { id: "running", status: "running" },
      { id: "stopping", status: "stopping" },
      { id: "switching", status: "switching" },
      { id: "idle", status: "idle" },
    ],
  };
  assert.equal(State.primaryAction(state, "running"), "stop");
  assert.equal(State.primaryAction(state, "stopping"), "stop");
  assert.equal(State.primaryAction(state, "switching"), "loading");
  assert.equal(State.primaryAction(state, "idle"), "send");
  assert.equal(State.primaryAction(state, "missing"), "send");
}

function testStrictTurnFencingAndConnectionErrors(State) {
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: snapshot("s1") });
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-live" },
  });

  for (const action of [
    {
      type: "message/delta",
      params: { taskId: "s1", turnId: "turn-old", messageId: "late-message", delta: "late" },
    },
    {
      type: "tool/started",
      params: { taskId: "s1", turnId: "turn-old", toolId: "late-tool" },
    },
    {
      type: "approval/requested",
      params: { taskId: "s1", turnId: "turn-old", approvalId: "late-approval" },
    },
    {
      type: "turn/completed",
      params: { taskId: "s1", turnId: "turn-old", status: "completed" },
    },
    {
      type: "turn/error",
      params: { taskId: "s1", turnId: "turn-old", message: "late failure" },
    },
  ]) {
    state = applyPure(State, state, action);
  }
  assert.equal(state.messages.length, 0);
  assert.equal(state.tools.length, 0);
  assert.equal(state.approvals.length, 0);
  assert.equal(state.terminal, null);
  assert.equal(state.error, null);
  assert.equal(State.taskById(state, "s1").activeTurnId, "turn-live");

  state = applyPure(State, state, {
    type: "message/delta",
    params: { taskId: "s1", turnId: "turn-live", messageId: "live-message", delta: "ok" },
  });
  state = applyPure(State, state, {
    type: "tool/started",
    params: { taskId: "s1", turnId: "turn-live", toolId: "live-tool" },
  });
  state = applyPure(State, state, {
    type: "approval/requested",
    params: { taskId: "s1", turnId: "turn-live", approvalId: "live-approval" },
  });
  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-live", status: "completed" },
  });
  assert.equal(state.messages.length, 1);
  assert.equal(state.tools.length, 1);
  assert.equal(state.approvals.length, 1);
  assert.equal(state.approvals[0].status, "resolved");
  assert.equal(state.approvals[0].decision, "cancelled");
  assert.equal(State.taskById(state, "s1").activeTurnId, null);
  assert.equal(State.taskById(state, "s1").lastTerminalTurnId, "turn-live");

  const afterTerminal = plain(state);
  for (const action of [
    {
      type: "message/delta",
      params: { taskId: "s1", turnId: "turn-live", messageId: "post-terminal", delta: "late" },
    },
    {
      type: "tool/completed",
      params: { taskId: "s1", turnId: "turn-live", toolId: "live-tool", output: "late" },
    },
    {
      type: "approval/resolved",
      params: { taskId: "s1", turnId: "turn-live", approvalId: "live-approval" },
    },
    {
      type: "turn/error",
      params: { taskId: "s1", turnId: "turn-live", message: "late failure" },
    },
  ]) {
    state = applyPure(State, state, action);
  }
  assert.deepEqual(plain(state), afterTerminal);

  state = applyPure(State, state, {
    type: "connection/error",
    params: { taskId: "s1", turnId: "turn-live", message: "bridge lost" },
  });
  assert.equal(state.error.message, "bridge lost");
  assert.equal(state.error.taskId, null);
}

function testApprovalResolutionIsStrictlyScopedAndValidated(State) {
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: snapshot("s1") });
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "approval-turn" },
  });
  state = applyPure(State, state, {
    type: "approval/requested",
    params: {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId: "approval-strict",
      summary: "Run the strict command",
    },
  });

  for (const params of [
    {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId: "approval-strict",
      decision: "allowOnce",
    },
    {
      taskId: "s1",
      turnId: "stale-turn",
      approvalId: "approval-strict",
      decision: "allow",
    },
    {
      taskId: "s2",
      turnId: "approval-turn",
      approvalId: "approval-strict",
      decision: "deny",
    },
  ]) {
    state = applyPure(State, state, { type: "approval/resolved", params });
    assert.equal(State.taskById(state, "s1").activeTurnId, "approval-turn");
    assert.equal(state.approvals[0].status, "pending");
  }

  state = applyPure(State, state, {
    type: "approval/resolved",
    params: {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId: "approval-strict",
      decision: "allowAlways",
    },
  });
  assert.equal(state.approvals[0].status, "resolved");
  assert.equal(state.approvals[0].decision, "allowAlways");
  const resolved = plain(state);
  state = applyPure(State, state, {
    type: "approval/resolved",
    params: {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId: "approval-strict",
      decision: "allowAlways",
    },
  });
  assert.deepEqual(plain(state), resolved);
}

function testFeedbackSelectorsFilterBackgroundTasks(State) {
  const background = {
    ...State.initialState(),
    currentTaskId: "s1",
    error: { taskId: "s2", message: "background failed" },
    terminal: { taskId: "s2", status: "completed" },
  };
  assert.equal(State.errorForCurrent(background), null);
  assert.equal(State.terminalForCurrent(background), null);

  const selected = { ...background, currentTaskId: "s2" };
  assert.equal(State.errorForCurrent(selected).message, "background failed");
  assert.equal(State.terminalForCurrent(selected).status, "completed");

  const globalError = { ...background, error: { taskId: null, message: "connection failed" } };
  assert.equal(State.errorForCurrent(globalError).message, "connection failed");
}

function testConcurrentTaskFeedbackIsIsolatedAndSnapshotClearsIt(State) {
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: snapshot("s1") });
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-s1" },
  });
  state = applyPure(State, state, {
    type: "turn/error",
    params: { taskId: "s1", turnId: "turn-s1", message: "s1 failed" },
  });
  assert.equal(State.errorForCurrent(state).message, "s1 failed");
  assert.equal(State.terminalForCurrent(state).status, "failed");

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s2", turnId: "turn-s2" },
  });
  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s2", turnId: "turn-s2", status: "completed" },
  });

  const s1 = { ...state, currentTaskId: "s1" };
  const s2 = { ...state, currentTaskId: "s2" };
  assert.equal(State.errorForCurrent(s1).message, "s1 failed");
  assert.equal(State.terminalForCurrent(s1).status, "failed");
  assert.equal(State.errorForCurrent(s2), null);
  assert.equal(State.terminalForCurrent(s2).status, "completed");

  state = applyPure(State, state, { type: "snapshot", snapshot: snapshot("s1") });
  assert.equal(State.errorForCurrent(state), null);
  assert.equal(State.terminalForCurrent(state), null);
  assert.equal(State.errorForCurrent({ ...state, currentTaskId: "s2" }), null);
  assert.equal(State.terminalForCurrent({ ...state, currentTaskId: "s2" }), null);
}

function testRunningSnapshotTurnBindingAndMissingTurnIds(State) {
  const runningSnapshot = snapshot("s1");
  runningSnapshot.tasks[0] = {
    ...runningSnapshot.tasks[0],
    status: "running",
    activeTurnId: null,
  };
  runningSnapshot.tasks[1] = {
    ...runningSnapshot.tasks[1],
    status: "running",
    activeTurnId: null,
  };
  let state = State.reduce(State.initialState(), {
    type: "snapshot",
    snapshot: runningSnapshot,
  });

  const beforeMissing = plain(state);
  for (const action of [
    { type: "message/delta", params: { taskId: "s1", messageId: "missing", delta: "no" } },
    { type: "tool/started", params: { taskId: "s1", toolId: "missing-tool" } },
    {
      type: "approval/requested",
      params: { taskId: "s1", approvalId: "missing-approval" },
    },
    { type: "turn/completed", params: { taskId: "s1", status: "completed" } },
    { type: "turn/error", params: { taskId: "s1", message: "missing turn" } },
  ]) {
    state = applyPure(State, state, action);
  }
  assert.deepEqual(plain(state), beforeMissing);

  state = applyPure(State, state, {
    type: "message/delta",
    params: {
      taskId: "s1",
      turnId: "snapshot-turn",
      messageId: "bound-message",
      delta: "bound",
    },
  });
  assert.equal(State.taskById(state, "s1").activeTurnId, "snapshot-turn");
  assert.equal(state.messages.at(-1).text, "bound");

  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "different-turn", status: "completed" },
  });
  assert.equal(State.taskById(state, "s1").status, "running");
  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "snapshot-turn", status: "completed" },
  });
  assert.equal(State.taskById(state, "s1").status, "idle");
  assert.equal(State.taskById(state, "s1").lastTerminalTurnId, "snapshot-turn");

  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s2", turnId: "terminal-first", status: "completed" },
  });
  assert.equal(State.taskById(state, "s2").status, "idle");
  assert.equal(State.taskById(state, "s2").lastTerminalTurnId, "terminal-first");

  const afterTerminal = plain(state);
  for (const action of [
    { type: "message/delta", params: { taskId: "s1", messageId: "late", delta: "no" } },
    { type: "tool/started", params: { taskId: "s1", toolId: "late-tool" } },
    { type: "approval/requested", params: { taskId: "s1", approvalId: "late-approval" } },
    { type: "turn/completed", params: { taskId: "s1", status: "completed" } },
    { type: "turn/error", params: { taskId: "s1", message: "late" } },
    {
      type: "message/delta",
      params: { taskId: "s1", turnId: "new-unstarted", messageId: "late-id", delta: "no" },
    },
  ]) {
    state = applyPure(State, state, action);
  }
  assert.deepEqual(plain(state), afterTerminal);
}

function testTurnStartedRejectsTerminalAndConflictingIds(State) {
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: snapshot("s1") });
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-old" },
  });
  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-old", status: "completed" },
  });
  const terminal = plain(state);
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-old" },
  });
  assert.deepEqual(plain(state), terminal);

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-new" },
  });
  const active = plain(state);
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-conflict" },
  });
  assert.deepEqual(plain(state), active);
  assert.equal(State.taskById(state, "s1").activeTurnId, "turn-new");

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1" },
  });
  assert.deepEqual(plain(state), active);
}

function testStoppingSnapshotBindsFirstTurnAndStartedDoesNotUndoStopping(State) {
  const value = snapshot("s1");
  value.tasks[0] = {
    ...value.tasks[0],
    status: "stopping",
    activeTurnId: null,
  };
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: value });
  state = applyPure(State, state, {
    type: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-stopping",
      messageId: "stopping-message",
      delta: "bound while stopping",
    },
  });
  assert.equal(State.taskById(state, "s1").activeTurnId, "turn-stopping");
  assert.equal(State.taskById(state, "s1").status, "stopping");
  assert.equal(state.messages.at(-1).text, "bound while stopping");

  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-stopping" },
  });
  assert.equal(State.taskById(state, "s1").status, "stopping");
  state = applyPure(State, state, {
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-stopping", status: "completed" },
  });
  assert.equal(State.taskById(state, "s1").status, "idle");
  assert.equal(State.taskById(state, "s1").lastTerminalTurnId, "turn-stopping");
}

function testTerminatedTurnHistoryIsBoundedAndSurvivesSnapshots(State) {
  let state = State.reduce(State.initialState(), { type: "snapshot", snapshot: snapshot("s1") });
  for (let index = 1; index <= 40; index += 1) {
    const turnId = `turn-${index}`;
    state = State.reduce(state, {
      type: "turn/started",
      params: { taskId: "s1", turnId },
    });
    state = State.reduce(state, {
      type: "turn/completed",
      params: { taskId: "s1", turnId, status: "completed" },
    });
  }
  const task = State.taskById(state, "s1");
  assert.deepEqual(plain(task.terminatedTurnIds),
    Array.from({ length: 32 }, (_, index) => `turn-${index + 9}`));
  assert.equal(task.lastTerminalTurnId, "turn-40");

  const beforeLate = plain(state);
  for (const action of [
    { type: "turn/started", params: { taskId: "s1", turnId: "turn-10" } },
    {
      type: "message/delta",
      params: { taskId: "s1", turnId: "turn-10", messageId: "late-history", delta: "late" },
    },
    {
      type: "turn/error",
      params: { taskId: "s1", turnId: "turn-10", message: "late failure" },
    },
  ]) {
    state = applyPure(State, state, action);
  }
  assert.deepEqual(plain(state), beforeLate);

  const refreshed = snapshot("s1");
  state = applyPure(State, state, { type: "snapshot", snapshot: refreshed });
  assert.deepEqual(
    plain(State.taskById(state, "s1").terminatedTurnIds),
    Array.from({ length: 32 }, (_, index) => `turn-${index + 9}`),
  );
  const afterSnapshot = plain(state);
  state = applyPure(State, state, {
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-10" },
  });
  assert.deepEqual(plain(state), afterSnapshot);
}

function testMarkdownBlocksAndSafeLinks(State) {
  const blocks = State.markdownBlocks([
    "# Result",
    "",
    "Paragraph with `inline` and [docs](https://example.com/docs).",
    "",
    "<script>alert(1)</script>",
    "",
    "- first",
    "- second",
    "",
    "```js",
    "const answer = 42;",
    "```",
    "",
    "[unsafe](javascript:alert(1))",
  ].join("\n"));

  assert.deepEqual(plain(blocks.map((block) => block.kind)), [
    "heading",
    "paragraph",
    "paragraph",
    "list",
    "code",
    "paragraph",
  ]);
  assert.equal(blocks[0].level, 1);
  assert.equal(blocks[2].text, "<script>alert(1)</script>");
  assert.equal(blocks[3].items.length, 2);
  assert.equal(blocks[4].language, "js");
  assert.match(blocks[4].text, /answer = 42/);
  assert.equal(
    blocks[1].inlines.some((part) => part.kind === "code" && part.text === "inline"),
    true,
  );
  assert.equal(
    blocks[1].inlines.some(
      (part) => part.kind === "link" && part.href === "https://example.com/docs",
    ),
    true,
  );
  assert.equal(blocks[5].inlines.some((part) => part.kind === "link"), false);
  assert.equal(State.safeUrl("javascript:alert(1)"), null);
  assert.equal(State.safeUrl("data:text/html,bad"), null);
  assert.equal(State.safeUrl("https://example.com"), "https://example.com");
  assert.equal(State.safeUrl("mailto:hello@example.com"), "mailto:hello@example.com");

  const table = State.markdownBlocks([
    "Tool permissions:",
    "",
    "| Tool | Safety | Mode |",
    "| --- | --- | --- |",
    "| `browser_read` | ReadOnly | No approval |",
    "| `browser_act` | Mutating | Prompt |",
  ].join("\n"));
  assert.deepEqual(plain(table.map((block) => block.kind)), ["paragraph", "table"]);
  assert.deepEqual(plain(table[1].headers.map((cell) => cell.text)), [
    "Tool",
    "Safety",
    "Mode",
  ]);
  assert.equal(table[1].rows.length, 2);
  assert.equal(table[1].rows[0][0].inlines[0].kind, "code");
  assert.equal(table[1].rows[1][2].text, "Prompt");
}

async function testCommonMarkAndGfmRenderer() {
  const React = require("react");
  const { renderToStaticMarkup } = require("react-dom/server");
  const [{ default: ReactMarkdown }, { default: remarkGfm }] = await Promise.all([
    import("react-markdown"),
    import("remark-gfm"),
  ]);
  const markdown = [
    "---",
    "",
    "**总样本量: 74,314 人**（初二、高三除外）",
    "",
    "**样本构成**：*分层抽样*与~~旧数据~~",
    "",
    "> 引用内容",
    "",
    "1. 第一项",
    "2. 第二项",
    "",
    "- [x] 已完成",
    "- [ ] 待处理",
    "",
    "| 分类 | 人数 |",
    "| --- | ---: |",
    "| **男生** | 37,147 |",
    "",
    "[危险链接](javascript:alert(1))",
    "",
    "<script>alert('blocked')</script>",
  ].join("\n");
  const rendered = renderToStaticMarkup(
    React.createElement(ReactMarkdown, { remarkPlugins: [remarkGfm], skipHtml: true }, markdown),
  );

  assert.match(rendered, /<hr\/>/);
  assert.match(rendered, /<strong>总样本量: 74,314 人<\/strong>/);
  assert.match(rendered, /<strong>男生<\/strong>/);
  assert.match(rendered, /<em>分层抽样<\/em>/);
  assert.match(rendered, /<del>旧数据<\/del>/);
  assert.match(rendered, /<blockquote>/);
  assert.match(rendered, /<ol>/);
  assert.match(rendered, /type="checkbox"/);
  assert.match(rendered, /<table>/);
  assert.doesNotMatch(rendered, /javascript:/);
  assert.doesNotMatch(rendered, /<script/);
}

function testHtmlAndSourceContracts() {
  for (const label of ["New task", "Attach files", "Send task", "Retry connection"]) {
    assert.match(reactSource, new RegExp(`aria-label=["'{][^\n]*${label}`));
  }
  assert.match(html, /id=["']zode-react-root["']/);
  assert.match(html, /href=["']dist\/sidepanel-react\.css["']/);
  assert.match(html, /src=["']dist\/sidepanel-react\.js["']/);
  assert.match(reactSource, /id=["']panel-title-react["']/);
  assert.match(reactSource, /aria-labelledby=["']panel-title-react["']/);
  assert.doesNotMatch(html, /\son[a-z]+\s*=/i);
  assert.doesNotMatch(html, /<script\b(?![^>]*\bsrc=)[^>]*>/i);
  assert.doesNotMatch(html, /(?:src|href)\s*=\s*["'](?:https?:)?\/\//i);
  for (const source of [stateSource, panelSource]) {
    assert.doesNotMatch(source, /\.innerHTML\b/);
    assert.doesNotMatch(source, /\b(?:eval|Function)\s*\(/);
  }
  assert.match(panelSource, /Stop task/);
  assert.match(panelSource, /attachment\/begin/);
  assert.match(panelSource, /attachment\/chunk/);
  assert.match(panelSource, /attachment\/finish/);
  assert.match(panelSource, /attachment\/cancel/);
  assert.match(
    reactSource,
    /<input[\s\S]{0,200}type=["']file["'][\s\S]{0,100}multiple[\s\S]{0,100}hidden/,
  );
  assert.match(reactSource, /ACCEPTED_ATTACHMENTS/);
  assert.match(css, /@media\s*\(prefers-color-scheme:\s*dark\)/);
  assert.match(css, /@media\s*\(max-width:\s*380px\)/);
  assert.doesNotMatch(css, /body\s*\{[^}]*min-width:\s*(?:3\d\d|[4-9]\d\d)px/s);
}

class FakeNode {
  constructor(tagName = "#text", text = "") {
    this.tagName = tagName.toUpperCase();
    this.children = [];
    this.attributes = {};
    this.dataset = {};
    this.listeners = {};
    this._text = String(text);
    this.value = "";
    this.disabled = false;
    this.hidden = false;
    this.selected = false;
    this.open = false;
    this.className = "";
    this.replaceCount = 0;
    this.focusCount = 0;
    this.clickCount = 0;
    this.files = [];
  }

  set textContent(value) {
    this._text = String(value == null ? "" : value);
    this.children = [];
  }

  get textContent() {
    return this._text + this.children.map((child) => child.textContent).join("");
  }

  append(...nodes) {
    this.children.push(...nodes);
  }

  appendChild(node) {
    this.children.push(node);
    return node;
  }

  replaceChildren(...nodes) {
    this._text = "";
    this.children = [...nodes];
    this.replaceCount += 1;
  }

  setAttribute(name, value) {
    this.attributes[name] = String(value);
  }

  addEventListener(type, listener) {
    if (!this.listeners[type]) {
      this.listeners[type] = [];
    }
    this.listeners[type].push(listener);
  }

  emit(type, event = {}) {
    const emitted = {
      key: "",
      shiftKey: false,
      isComposing: false,
      defaultPrevented: false,
      preventDefault() {
        this.defaultPrevented = true;
      },
      ...event,
      target: this,
      currentTarget: this,
    };
    for (const listener of this.listeners[type] || []) {
      listener(emitted);
    }
    return emitted;
  }

  focus() {
    this.focusCount += 1;
  }

  click() {
    this.clickCount += 1;
    this.emit("click");
  }
}

class FakeDocument {
  createElement(tagName) {
    return new FakeNode(tagName);
  }

  createTextNode(text) {
    return new FakeNode("#text", text);
  }
}

class FakeUiDocument extends FakeDocument {
  constructor() {
    super();
    this.nodes = new Map();
    for (const id of [
      "connection-status",
      "connection-banner",
      "connection-banner-message",
      "retry-button",
      "workspace-label",
      "task-menu",
      "new-task-button",
      "more-button",
      "message-stream",
      "message-list",
      "empty-state",
      "tool-region",
      "approval-region",
      "error-region",
      "composer-form",
      "composer",
      "attach-button",
      "attachment-input",
      "attachment-list",
      "attachment-status",
      "access-select",
      "model-select",
      "send-button",
    ]) {
      this.nodes.set(id, new FakeNode(id.includes("select") || id === "task-menu" ? "select" : "div"));
    }
  }

  getElementById(id) {
    return this.nodes.get(id) || null;
  }
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function flushAsync() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function descendants(node) {
  return [node, ...node.children.flatMap(descendants)];
}

function testMarkdownDomRendererUsesTextNodesOnly(State, App) {
  const document = new FakeDocument();
  const root = document.createElement("div");
  App.renderMarkdown(
    document,
    root,
    "<script>alert(1)</script> [bad](javascript:alert(2)) [good](https://example.com)",
  );

  const nodes = descendants(root);
  assert.equal(nodes.some((node) => node.tagName === "SCRIPT"), false);
  assert.match(root.textContent, /<script>alert\(1\)<\/script>/);
  const links = nodes.filter((node) => node.tagName === "A");
  assert.equal(links.length, 1);
  assert.equal(links[0].attributes.href, "https://example.com");
  assert.equal(State.safeUrl(links[0].attributes.href), "https://example.com");
}

function makeStorage(initial = {}, options = {}) {
  const data = plain(initial);
  const sets = [];
  return {
    data,
    sets,
    get(keys) {
      if (options.getSyncError) {
        throw options.getSyncError;
      }
      if (options.getAsyncError) {
        return Promise.reject(options.getAsyncError);
      }
      const selected = {};
      for (const key of Array.isArray(keys) ? keys : [keys]) {
        if (Object.hasOwn(data, key)) {
          selected[key] = data[key];
        }
      }
      return Promise.resolve(plain(selected));
    },
    set(values) {
      if (options.setSyncError) {
        throw options.setSyncError;
      }
      if (options.setAsyncError) {
        return Promise.reject(options.setAsyncError);
      }
      sets.push(plain(values));
      Object.assign(data, plain(values));
      return Promise.resolve();
    },
  };
}

function makeRuntime(handler, options = {}) {
  const calls = [];
  const listeners = [];
  return {
    calls,
    listeners,
    sendMessage(message) {
      calls.push(plain(message));
      if (options.syncError) {
        throw options.syncError;
      }
      return handler(message);
    },
    onMessage: {
      addListener(listener) {
        if (options.listenerSyncError) {
          throw options.listenerSyncError;
        }
        listeners.push(listener);
      },
    },
  };
}

function snapshot(currentTaskId = "s1") {
  return {
    workspace: { name: "zode", path: "/workspace" },
    tasks: [
      { id: "s1", title: "First", status: "idle", model: "m1", access: "prompt" },
      { id: "s2", title: "Second", status: "idle", model: "m1", access: "readOnly" },
    ],
    currentTaskId,
    models: ["m1", "m2"],
    messages: [],
    tools: [],
    approvals: [],
  };
}

function dispatchRunningApproval(controller, approvalId, summary = "Run a command") {
  controller.dispatch({
    type: "approval/requested",
    params: {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId,
      summary,
    },
  });
}

function prepareRunningApprovalController(controller) {
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "approval-turn" },
  });
}

async function testControllerStartupReconnectsHydratesAndRegistersOnce(App) {
  const storage = makeStorage({
    zodePanelCurrentTask: "s1",
    zodePanelDrafts: { s1: "draft one", s2: "draft two" },
    zodePanelCollapsedTools: ["tool-old"],
  });
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: false, canReconnect: true } });
    }
    if (message.type === "zode-reconnect") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.type === "zode-task-request" && message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const renders = [];
  const controller = App.createController({
    runtime,
    storage,
    render: (state) => renders.push(plain(state)),
  });

  await controller.start();
  assert.deepEqual(runtime.calls.slice(0, 3).map((call) => call.type), [
    "zode-status",
    "zode-reconnect",
    "zode-task-request",
  ]);
  assert.equal(runtime.calls[2].method, "snapshot/read");
  assert.equal(controller.getState().connection, "connected");
  assert.equal(controller.getState().currentTaskId, "s1");
  assert.equal(controller.currentDraft(), "draft one");
  assert.deepEqual(plain(controller.getState().collapsedToolIds), ["tool-old"]);
  assert.equal(runtime.listeners.length, 1);
  const callCount = runtime.calls.length;
  await controller.start();
  assert.equal(runtime.listeners.length, 1);
  assert.equal(runtime.calls.length, callCount);
  assert.ok(renders.length > 1);

  await controller.selectTask("s2");
  assert.equal(runtime.calls.at(-1).method, "task/select");
  assert.deepEqual(runtime.calls.at(-1).params, { taskId: "s2" });
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(controller.currentDraft(), "draft two");
  assert.equal(storage.data.zodePanelCurrentTask, "s2");

  await controller.setDraft("changed second draft");
  assert.equal(storage.data.zodePanelDrafts.s2, "changed second draft");
  await controller.toggleTool("tool-2");
  assert.equal(storage.data.zodePanelCollapsedTools.includes("tool-2"), true);

  runtime.listeners[0]({ type: "zode-task-disconnected" });
  assert.equal(controller.getState().connection, "disconnected");
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(controller.currentDraft(), "changed second draft");
}

async function testControllerDispatchesEventsAndCoreRequestPaths(App) {
  const storage = makeStorage();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.type !== "zode-task-request") {
      return Promise.resolve({ ok: true });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    if (message.method === "task/create") {
      return Promise.resolve({
        ok: true,
        result: { task: { id: "s3", title: "New task", status: "idle", model: "m1", access: "prompt" } },
      });
    }
    if (message.method === "turn/start") {
      return Promise.resolve({ ok: true, result: { turnId: "turn-request" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage, render: () => {} });
  await controller.start();

  runtime.listeners[0]({
    type: "zode-task-event",
    event: "turn/started",
    params: { taskId: "s1", turnId: "turn-1" },
  });
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "message/delta",
    params: { taskId: "s1", turnId: "turn-1", messageId: "a1", delta: "streamed" },
  });
  assert.equal(controller.getState().messages[0].text, "streamed");
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "turn/completed",
    params: { taskId: "s1", turnId: "turn-1", status: "completed" },
  });

  await controller.createTask();
  assert.equal(runtime.calls.at(-1).method, "task/create");
  assert.equal(controller.getState().tasks.some((task) => task.id === "s3"), true);

  await controller.selectTask("s1");
  await controller.setModel("m2");
  assert.deepEqual(runtime.calls.at(-1), {
    type: "zode-task-request",
    method: "model/set",
    params: { taskId: "s1", model: "m2" },
  });
  await controller.setAccess("auto");
  assert.deepEqual(runtime.calls.at(-1), {
    type: "zode-task-request",
    method: "permission/set",
    params: { taskId: "s1", mode: "auto" },
  });

  await controller.setDraft("inspect the repo");
  await controller.submit();
  assert.deepEqual(runtime.calls.at(-1), {
    type: "zode-task-request",
    method: "turn/start",
    params: { taskId: "s1", input: "inspect the repo" },
  });
  assert.equal(controller.currentDraft(), "");
  assert.equal(controller.primaryAction(), "stop");

  await controller.submit();
  const interrupt = runtime.calls.at(-1);
  assert.equal(interrupt.method, "turn/interrupt");
  assert.equal(interrupt.params.taskId, "s1");
}

async function testRuntimeSnapshotEventUsesParamsAsSnapshot(App) {
  const storage = makeStorage({ zodePanelCurrentTask: "s1" });
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage, render: () => {} });
  await controller.start();

  const pushed = snapshot("s2");
  pushed.tasks[1].title = "Updated by event";
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "snapshot",
    params: pushed,
  });
  await flushAsync();

  assert.equal(controller.getState().tasks.length, 2);
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(controller.getState().tasks[1].title, "Updated by event");
  assert.equal(storage.data.zodePanelCurrentTask, "s2");
}

async function testConnectionStaysCheckingUntilSnapshotSucceeds(App) {
  const snapshotGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return snapshotGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);

  const starting = controller.start();
  await flushAsync();
  assert.equal(controller.getState().connection, "checking");
  assert.equal(view.elements.taskMenu.disabled, true);
  assert.equal(view.elements.newTask.disabled, true);
  assert.equal(view.elements.model.disabled, true);
  assert.equal(view.elements.access.disabled, true);
  assert.equal(view.elements.send.disabled, true);

  snapshotGate.resolve({ ok: true, result: snapshot("s1") });
  await starting;
  assert.equal(controller.getState().connection, "connected");
}

async function testInitialSnapshotFailureDisconnectsAndKeepsActionsLocked(App) {
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: false, error: "authoritative snapshot failed" });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);

  await controller.start();
  assert.equal(controller.getState().connection, "disconnected");
  assert.equal(view.elements.taskMenu.disabled, true);
  assert.equal(view.elements.newTask.disabled, true);
  assert.equal(view.elements.model.disabled, true);
  assert.equal(view.elements.access.disabled, true);
  assert.equal(view.elements.send.disabled, true);
  await assert.rejects(() => controller.createTask(), /browser pair/i);
  assert.equal(runtime.calls.some((call) => call.method === "task/create"), false);
}

async function testSnapshotCorrectionPersistsCurrentTask(App) {
  const storage = makeStorage({
    zodePanelCurrentTask: "s1",
    zodePanelDrafts: { s1: "first", s2: "second" },
  });
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s2") });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage, render: () => {} });

  await controller.start();
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(storage.data.zodePanelCurrentTask, "s2");
  assert.equal(controller.currentDraft(), "second");
}

async function testControllerFailurePathsAreActionableAndChromeCallsAreSafe(App) {
  const storage = makeStorage({
    zodePanelCurrentTask: "s1",
    zodePanelDrafts: { s1: "do not lose" },
  });
  const failingSnapshotRuntime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    return Promise.resolve({ ok: false, error: "snapshot exploded" });
  });
  const failed = App.createController({
    runtime: failingSnapshotRuntime,
    storage,
    render: () => {},
  });
  await failed.start();
  assert.match(failed.getState().error.message, /snapshot exploded/i);
  assert.match(failed.getState().error.message, /\/browser pair/);
  assert.equal(failed.currentDraft(), "do not lose");

  const throwingRuntime = makeRuntime(
    () => Promise.resolve({ ok: true }),
    { syncError: new Error("extension context invalidated") },
  );
  const throwingStorage = makeStorage(
    { zodePanelDrafts: { s1: "fallback" } },
    { getAsyncError: new Error("storage unavailable"), setSyncError: new Error("cannot persist") },
  );
  const safe = App.createController({ runtime: throwingRuntime, storage: throwingStorage, render: () => {} });
  await assert.doesNotReject(() => safe.start());
  assert.equal(safe.getState().connection, "disconnected");
  await assert.doesNotReject(() => safe.setDraft("still editable"));
  assert.equal(safe.currentDraft(), "still editable");
}

async function testDisconnectedAndCheckingGuardsIncludingEnter(App) {
  const methods = [
    ["createTask", []],
    ["selectTask", ["s2"]],
    ["setModel", ["m2"]],
    ["setAccess", ["auto"]],
    ["submit", []],
  ];

  for (const connection of ["checking", "disconnected"]) {
    for (const [method, args] of methods) {
      const storage = makeStorage();
      const runtime = makeRuntime((message) => {
        if (message.method === "task/create") {
          return Promise.resolve({
            ok: true,
            result: { task: { id: "s3", title: "Third", status: "idle" } },
          });
        }
        if (message.method === "turn/start") {
          return Promise.resolve({ ok: true, result: { turnId: "turn-offline" } });
        }
        return Promise.resolve({ ok: true, result: {} });
      });
      const controller = App.createController({ runtime, storage, render: () => {} });
      controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
      if (connection === "disconnected") {
        controller.dispatch({ type: "disconnected", message: "offline" });
      }
      await controller.setDraft("keep this draft");
      await Promise.allSettled([Promise.resolve().then(() => controller[method](...args))]);
      assert.equal(
        runtime.calls.filter((call) => call.type === "zode-task-request").length,
        0,
        `${method} emitted a request while ${connection}`,
      );
      assert.equal(controller.getState().drafts.s1, "keep this draft");
    }
  }

  const document = new FakeUiDocument();
  const runtime = makeRuntime(() =>
    Promise.resolve({ ok: true, result: { turnId: "turn-from-enter" } }),
  );
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  view.elements.composer.value = "draft from enter";
  view.elements.composer.emit("input");
  const keyEvent = view.elements.composer.emit("keydown", {
    key: "Enter",
    shiftKey: false,
    isComposing: false,
  });
  await flushAsync();
  assert.equal(keyEvent.defaultPrevented, true);
  assert.equal(runtime.calls.length, 0);
  assert.equal(controller.getState().drafts.s1, "draft from enter");
}

async function testTurnStartSingleFlightAndDraftOwnership(App) {
  let activeGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.method === "turn/start") {
      return activeGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  await controller.setDraft("original draft");

  const first = controller.submit();
  await flushAsync();
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-from-event" },
  });
  const second = controller.submit();
  await flushAsync();
  const requestCountWhileFirstPending = runtime.calls.filter(
    (call) => call.type === "zode-task-request",
  ).length;
  const exposesSubmitting = typeof controller.isSubmitting === "function";
  const pendingWhileSending = exposesSubmitting && controller.isSubmitting("s1");
  controller.dispatch({ type: "selection/set", taskId: "s2" });
  await controller.setDraft("second task draft");
  activeGate.resolve({ ok: true, result: { turnId: "turn-1" } });
  await Promise.all([first, second]);

  controller.dispatch({
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-from-event", status: "completed" },
  });
  controller.dispatch({ type: "selection/set", taskId: "s1" });
  await controller.setDraft("draft before send");
  activeGate = deferred();
  const third = controller.submit();
  await flushAsync();
  await controller.setDraft("edited while sending");
  activeGate.resolve({ ok: true, result: { turnId: "turn-2" } });
  await third;

  const turnStarts = runtime.calls.filter((call) => call.method === "turn/start");
  const sentMessages = controller
    .getState()
    .messages.filter((message) => message.role === "user" && message.taskId === "s1");
  assert.equal(exposesSubmitting, true);
  assert.equal(pendingWhileSending, true);
  assert.equal(requestCountWhileFirstPending, 1);
  assert.equal(controller.isSubmitting("s1"), false);
  assert.equal(turnStarts.length, 2, "two submit clicks must share the first request");
  assert.deepEqual(
    plain(sentMessages.map((message) => [message.turnId, message.text])),
    [
      ["turn-1", "original draft"],
      ["turn-2", "draft before send"],
    ],
    "each acknowledged turn must render its submitted user text once",
  );
  assert.equal(controller.getState().drafts.s2, "second task draft");
  assert.equal(controller.getState().drafts.s1, "edited while sending");
}

async function testTurnStartResponseCannotReviveTerminalOrReplaceNewerTurn(App, State) {
  let activeGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "turn/start"
      ? activeGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  await controller.setDraft("first race");

  const terminalRace = controller.submit();
  await flushAsync();
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-terminal" },
  });
  controller.dispatch({
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-terminal", status: "completed" },
  });
  activeGate.resolve({ ok: true, result: { turnId: "turn-terminal" } });
  await terminalRace;
  assert.equal(State.taskById(controller.getState(), "s1").status, "idle");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, null);
  assert.equal(State.taskById(controller.getState(), "s1").lastTerminalTurnId, "turn-terminal");

  activeGate = deferred();
  await controller.setDraft("second race");
  const newerRace = controller.submit();
  await flushAsync();
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-response-old" },
  });
  controller.dispatch({
    type: "turn/completed",
    params: { taskId: "s1", turnId: "turn-response-old", status: "completed" },
  });
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "turn-newer" },
  });
  activeGate.resolve({ ok: true, result: { turnId: "turn-response-old" } });
  await newerRace;
  assert.equal(State.taskById(controller.getState(), "s1").status, "running");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "turn-newer");
  assert.equal(State.taskById(controller.getState(), "s1").lastTerminalTurnId, "turn-response-old");
}

async function testTurnStartResponseDoesNotLoseItsFirstDelta(App, State) {
  const turnGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    return message.method === "turn/start"
      ? turnGate.promise
      : Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.setDraft("stream immediately");

  const submitting = controller.submit();
  await flushAsync();
  turnGate.resolve({ ok: true, result: { turnId: "turn-immediate" } });
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-immediate",
      messageId: "assistant-immediate",
      delta: "first token",
    },
  });
  await submitting;

  runtime.listeners[0]({
    type: "zode-task-event",
    event: "turn/started",
    params: { taskId: "s1", turnId: "turn-immediate" },
  });
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "turn/interrupted",
    params: { taskId: "s1", turnId: "turn-stale", status: "interrupted" },
  });
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "turn/completed",
    params: { taskId: "s1", turnId: "turn-stale", status: "completed" },
  });

  assert.deepEqual(
    plain(controller.getState().messages),
    [
      {
        id: "s1:turn-immediate:user",
        taskId: "s1",
        turnId: "turn-immediate",
        role: "user",
        text: "stream immediately",
        order: 0,
      },
      {
        id: "assistant-immediate",
        taskId: "s1",
        turnId: "turn-immediate",
        role: "assistant",
        text: "first token",
        order: 1,
      },
    ],
  );
  assert.equal(State.taskById(controller.getState(), "s1").status, "running");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "turn-immediate");
}

async function testAuthoritativeSnapshotInvalidatesPendingTurnStart(App, State) {
  const turnGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: { connected: true, taskConnectionId: "snapshot-socket" },
      });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "snapshot-upload" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "snapshot-attachment" } });
    }
    if (message.method === "turn/start") {
      return turnGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.addFiles([fakeFile("snapshot.rs", "text/plain", 4)]);
  await controller.setDraft("survive the authoritative snapshot");

  const submitting = controller.submit();
  await flushAsync();
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-before-snapshot",
      messageId: "old-snapshot-message",
      delta: "must be discarded",
    },
  });
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "snapshot",
    params: snapshot("s1"),
  });
  const settlement = await Promise.race([
    submitting.then(
      () => ({ status: "resolved" }),
      (error) => ({ status: "rejected", error }),
    ),
    flushAsync().then(() => ({ status: "pending" })),
  ]);
  assert.equal(settlement.status, "rejected", "snapshot must release the pending flight");
  assert.equal(settlement.error.code, "stale_turn_start");
  assert.equal(controller.isSubmitting("s1"), false);
  turnGate.resolve({ ok: true, result: { turnId: "turn-before-snapshot" } });
  await flushAsync();
  assert.equal(controller.currentDraft(), "survive the authoritative snapshot");
  assert.deepEqual(plain(controller.getState().messages), []);
  assert.equal(State.taskById(controller.getState(), "s1").status, "idle");
  assert.deepEqual(
    plain(controller.getAttachments()).map((attachment) => ({
      status: attachment.status,
      attachmentId: attachment.attachmentId,
    })),
    [{ status: "ready", attachmentId: "snapshot-attachment" }],
  );
  assert.equal(controller.getState().error, null);
}

async function testDisconnectInvalidatesPendingTurnStart(App, State) {
  const turnGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: { connected: true, taskConnectionId: "disconnect-socket" },
      });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    if (message.method === "turn/start") {
      return turnGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.setDraft("survive disconnect");

  const submitting = controller.submit();
  await flushAsync();
  runtime.listeners[0]({
    type: "zode-task-event",
    event: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-before-disconnect",
      messageId: "old-disconnect-message",
      delta: "must be discarded",
    },
  });
  runtime.listeners[0]({ type: "zode-task-disconnected" });
  const settlement = await Promise.race([
    submitting.then(
      () => ({ status: "resolved" }),
      (error) => ({ status: "rejected", error }),
    ),
    flushAsync().then(() => ({ status: "pending" })),
  ]);
  assert.equal(settlement.status, "rejected", "disconnect must release the pending flight");
  assert.equal(settlement.error.code, "stale_turn_start");
  assert.equal(controller.isSubmitting("s1"), false);
  turnGate.resolve({ ok: true, result: { turnId: "turn-before-disconnect" } });
  await flushAsync();
  assert.equal(controller.getState().connection, "disconnected");
  assert.equal(controller.currentDraft(), "survive disconnect");
  assert.deepEqual(plain(controller.getState().messages), []);
  assert.equal(State.taskById(controller.getState(), "s1").status, "idle");
  assert.equal(controller.getState().error, null);
}

async function testApprovalResponsesAreSingleFlightAndIndependent(App) {
  const gates = new Map([
    ["approval-one", deferred()],
    ["approval-two", deferred()],
  ]);
  const runtime = makeRuntime((message) => {
    if (message.method === "approval/respond") {
      return gates.get(message.params.approvalId).promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  prepareRunningApprovalController(controller);
  dispatchRunningApproval(controller, "approval-one", "Read secrets");
  dispatchRunningApproval(controller, "approval-two", "Delete temp file");

  await assert.rejects(
    () => controller.respondApproval("approval-one", "allowOnce"),
    /decision/i,
  );
  const first = controller.respondApproval("approval-one", "allow");
  const duplicate = controller.respondApproval("approval-one", "allowAlways");
  const second = controller.respondApproval("approval-two", "deny");
  assert.equal(first, duplicate);
  await flushAsync();

  assert.equal(controller.isRespondingApproval("approval-one"), true);
  assert.equal(controller.isRespondingApproval("approval-two"), true);
  assert.deepEqual(
    plain(runtime.calls.filter((call) => call.method === "approval/respond")),
    [
      {
        type: "zode-task-request",
        method: "approval/respond",
        params: {
          taskId: "s1",
          turnId: "approval-turn",
          approvalId: "approval-one",
          decision: "allow",
        },
      },
      {
        type: "zode-task-request",
        method: "approval/respond",
        params: {
          taskId: "s1",
          turnId: "approval-turn",
          approvalId: "approval-two",
          decision: "deny",
        },
      },
    ],
  );

  gates.get("approval-two").resolve({ ok: true, result: {} });
  await second;
  gates.get("approval-one").reject(new Error("approval backend failed"));
  const firstResults = await Promise.allSettled([first, duplicate]);

  assert.equal(firstResults.every((result) => result.status === "rejected"), true);
  assert.equal(controller.isRespondingApproval("approval-one"), false);
  assert.equal(controller.isRespondingApproval("approval-two"), false);
  const approvalOne = controller.getState().approvals.find((item) => item.id === "approval-one");
  const approvalTwo = controller.getState().approvals.find((item) => item.id === "approval-two");
  assert.equal(approvalOne.status, "pending");
  assert.equal(approvalTwo.status, "resolved");
  assert.equal(approvalTwo.decision, "deny");
  assert.match(controller.getState().errorsByTask.s1.message, /无法响应审批/);
  assert.match(controller.getState().errorsByTask.s1.message, /approval backend failed/);
}

async function testServerStaleApprovalIsAnExpectedFence(App) {
  const runtime = makeRuntime((message) => {
    if (message.method === "approval/respond") {
      return Promise.resolve({
        ok: false,
        code: "stale_approval",
        error: "approval is stale",
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  prepareRunningApprovalController(controller);
  dispatchRunningApproval(controller, "approval-stale");

  await assert.rejects(
    () => controller.respondApproval("approval-stale", "allow"),
    (error) => error && error.code === "stale_approval_response",
  );

  assert.equal(controller.isRespondingApproval("approval-stale"), false);
  assert.equal(controller.getState().error, null);
  assert.equal(controller.getState().errorsByTask.s1, undefined);
}

async function testApprovalResponseIsFencedByDisconnectAndTerminal(App, State) {
  for (const invalidation of ["disconnect", "terminal"]) {
    const gate = deferred();
    const runtime = makeRuntime((message) =>
      message.method === "approval/respond"
        ? gate.promise
        : Promise.resolve({ ok: true, result: {} }),
    );
    const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
    prepareRunningApprovalController(controller);
    dispatchRunningApproval(controller, `approval-${invalidation}`);

    const responding = controller.respondApproval(`approval-${invalidation}`, "allowAlways");
    await flushAsync();
    if (invalidation === "disconnect") {
      controller.dispatch({ type: "disconnected", message: "offline" });
    } else {
      controller.dispatch({
        type: "turn/completed",
        params: { taskId: "s1", turnId: "approval-turn", status: "completed" },
      });
      controller.dispatch({
        type: "turn/started",
        params: { taskId: "s1", turnId: "new-turn" },
      });
    }

    const settlement = await Promise.race([
      responding.then(
        () => ({ status: "resolved" }),
        (error) => ({ status: "rejected", error }),
      ),
      flushAsync().then(() => ({ status: "pending" })),
    ]);
    assert.equal(settlement.status, "rejected", `${invalidation} must release the approval flight`);
    assert.equal(settlement.error.code, "stale_approval_response");
    assert.equal(controller.isRespondingApproval(`approval-${invalidation}`), false);

    gate.resolve({ ok: true, result: {} });
    await flushAsync();
    const approval = controller
      .getState()
      .approvals.find((item) => item.id === `approval-${invalidation}`);
    if (invalidation === "disconnect") {
      assert.equal(controller.getState().connection, "disconnected");
      assert.equal(approval.status, "pending");
      assert.equal(approval.decision, undefined);
    } else {
      assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "new-turn");
      assert.equal(approval.status, "resolved");
      assert.equal(approval.decision, "cancelled");
    }
    assert.equal(controller.getState().error, null);
  }
}

async function testApprovalResponseIsFencedByAuthoritativeSnapshot(App, State) {
  const responseGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "approval/respond"
      ? responseGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  prepareRunningApprovalController(controller);
  dispatchRunningApproval(controller, "approval-before-snapshot");

  const responding = controller.respondApproval("approval-before-snapshot", "allow");
  await flushAsync();
  const authoritative = snapshot("s1");
  authoritative.tasks[0] = {
    ...authoritative.tasks[0],
    status: "running",
    activeTurnId: "snapshot-new-turn",
  };
  authoritative.approvals = [
    {
      id: "snapshot-new-approval",
      taskId: "s1",
      turnId: "snapshot-new-turn",
      status: "pending",
      summary: "Authoritative approval",
      order: 0,
    },
  ];
  controller.dispatch({ type: "snapshot", snapshot: authoritative });

  const settlement = await Promise.race([
    responding.then(
      () => ({ status: "resolved" }),
      (error) => ({ status: "rejected", error }),
    ),
    flushAsync().then(() => ({ status: "pending" })),
  ]);
  assert.equal(settlement.status, "rejected");
  assert.equal(settlement.error.code, "stale_approval_response");
  assert.equal(controller.isRespondingApproval("approval-before-snapshot"), false);

  responseGate.resolve({ ok: true, result: {} });
  await flushAsync();
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "snapshot-new-turn");
  assert.deepEqual(
    plain(controller.getState().approvals),
    plain(authoritative.approvals),
  );
  assert.equal(controller.getState().error, null);
}

async function testApprovalResponseIsFencedByReplacementSocket(App, State) {
  const responseGate = deferred();
  const replacementSnapshotGate = deferred();
  let snapshotReads = 0;
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: { connected: true, taskConnectionId: "approval-socket-old" },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      return snapshotReads === 1
        ? Promise.resolve({ ok: true, result: snapshot("s1") })
        : replacementSnapshotGate.promise;
    }
    if (message.method === "approval/respond") {
      return responseGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  controller.dispatch({
    type: "turn/started",
    params: { taskId: "s1", turnId: "approval-turn" },
  });
  dispatchRunningApproval(controller, "approval-before-replacement");

  const responding = controller.respondApproval("approval-before-replacement", "allowAlways");
  await flushAsync();
  runtime.listeners[0]({
    type: "zode-task-connected",
    protocolVersion: 1,
    connectionId: "approval-socket-new",
  });

  const settlement = await Promise.race([
    responding.then(
      () => ({ status: "resolved" }),
      (error) => ({ status: "rejected", error }),
    ),
    flushAsync().then(() => ({ status: "pending" })),
  ]);
  assert.equal(settlement.status, "rejected");
  assert.equal(settlement.error.code, "stale_approval_response");
  assert.equal(controller.isRespondingApproval("approval-before-replacement"), false);

  responseGate.resolve({ ok: true, result: {} });
  await flushAsync();
  const replacement = snapshot("s1");
  replacement.tasks[0] = {
    ...replacement.tasks[0],
    status: "running",
    activeTurnId: "replacement-new-turn",
  };
  replacementSnapshotGate.resolve({ ok: true, result: replacement });
  await flushAsync();
  await flushAsync();

  assert.equal(snapshotReads, 2);
  assert.equal(controller.getState().connection, "connected");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "replacement-new-turn");
  assert.deepEqual(plain(controller.getState().approvals), []);
  assert.equal(controller.getState().error, null);
}

async function testServerApprovalResolvedSettlesPendingResponse(App) {
  const responseGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "approval/respond"
      ? responseGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  prepareRunningApprovalController(controller);
  dispatchRunningApproval(controller, "approval-server-event");

  const responding = controller.respondApproval("approval-server-event", "allow");
  await flushAsync();
  controller.dispatch({
    type: "approval/resolved",
    params: {
      taskId: "s1",
      turnId: "approval-turn",
      approvalId: "approval-server-event",
      decision: "allow",
    },
  });
  const settlement = await Promise.race([
    responding.then(
      () => "resolved",
      () => "rejected",
    ),
    flushAsync().then(() => "pending"),
  ]);

  assert.equal(settlement, "resolved");
  assert.equal(controller.isRespondingApproval("approval-server-event"), false);
  const approval = controller
    .getState()
    .approvals.find((item) => item.id === "approval-server-event");
  assert.equal(approval.status, "resolved");
  assert.equal(approval.decision, "allow");
  responseGate.resolve({ ok: true, result: {} });
  await flushAsync();
  assert.equal(approval.status, "resolved");
}

async function testApprovalCardsExposeIndependentDecisionControls(App) {
  const gates = new Map([
    ["approval-ui-one", deferred()],
    ["approval-ui-two", deferred()],
  ]);
  const runtime = makeRuntime((message) =>
    message.method === "approval/respond"
      ? gates.get(message.params.approvalId).promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  prepareRunningApprovalController(controller);
  dispatchRunningApproval(controller, "approval-ui-one", "<img src=x onerror=alert(1)>");
  dispatchRunningApproval(controller, "approval-ui-two", "Run tests");

  let cards = view.elements.approvalRegion.children;
  assert.equal(cards.length, 2);
  assert.equal(descendants(cards[0]).some((node) => node.tagName === "IMG"), false);
  assert.match(cards[0].textContent, /<img src=x onerror=alert\(1\)>/);
  for (const card of cards) {
    assert.deepEqual(
      descendants(card)
        .filter((node) => node.tagName === "BUTTON")
        .map((button) => button.textContent),
      ["Allow once", "Always allow", "Deny"],
    );
  }

  let firstButtons = descendants(cards[0]).filter((node) => node.tagName === "BUTTON");
  firstButtons[0].emit("click");
  firstButtons[1].emit("click");
  await flushAsync();
  cards = view.elements.approvalRegion.children;
  firstButtons = descendants(cards[0]).filter((node) => node.tagName === "BUTTON");
  const secondButtons = descendants(cards[1]).filter((node) => node.tagName === "BUTTON");
  assert.equal(firstButtons.every((button) => button.disabled), true);
  assert.equal(secondButtons.every((button) => !button.disabled), true);
  assert.equal(
    runtime.calls.filter(
      (call) =>
        call.method === "approval/respond" && call.params.approvalId === "approval-ui-one",
    ).length,
    1,
  );

  gates.get("approval-ui-one").reject(new Error("try approval again"));
  await flushAsync();
  await flushAsync();
  cards = view.elements.approvalRegion.children;
  firstButtons = descendants(cards[0]).filter((node) => node.tagName === "BUTTON");
  assert.equal(firstButtons.every((button) => !button.disabled), true);
  assert.match(view.elements.errorRegion.textContent, /try approval again/);

  const deny = descendants(cards[1]).find(
    (node) => node.tagName === "BUTTON" && node.textContent === "Deny",
  );
  deny.emit("click");
  await flushAsync();
  gates.get("approval-ui-two").resolve({ ok: true, result: {} });
  await flushAsync();
  await flushAsync();
  assert.equal(view.elements.approvalRegion.children.length, 1);
  assert.match(view.elements.approvalRegion.textContent, /<img src=x onerror=alert\(1\)>/);
}

async function testDomShowsSendingAndUsesLightDraftRender(App) {
  const document = new FakeUiDocument();
  const gate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "turn/start"
      ? gate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  controller.dispatch({ type: "connected" });
  const richSnapshot = snapshot("s1");
  richSnapshot.messages = [{ id: "m1", taskId: "s1", role: "assistant", text: "hello" }];
  richSnapshot.tools = [{ id: "tool-1", taskId: "s1", status: "completed" }];
  controller.dispatch({ type: "snapshot", snapshot: richSnapshot });

  const messageRenders = view.elements.messageList.replaceCount;
  const toolRenders = view.elements.toolRegion.replaceCount;
  await controller.setDraft("typing");
  assert.equal(view.elements.composer.value, "typing");
  assert.equal(view.elements.messageList.replaceCount, messageRenders);
  assert.equal(view.elements.toolRegion.replaceCount, toolRenders);

  const sending = controller.submit();
  await flushAsync();
  assert.equal(view.elements.send.textContent, "Sending");
  assert.equal(view.elements.send.disabled, true);
  gate.resolve({ ok: true, result: { turnId: "turn-ui" } });
  await sending;
}

async function testMessageDeltaUpdatesOnlyItsDomNodeAndEmptyStateIsTaskScoped(App) {
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({
    runtime: makeRuntime(() => Promise.resolve({ ok: true, result: {} })),
    storage: makeStorage(),
    view,
  });
  view.bind(controller);
  controller.dispatch({ type: "connected" });

  const backgroundToolOnly = snapshot("s1");
  backgroundToolOnly.tools = [
    { id: "background-tool", taskId: "s2", status: "completed", summary: "Background" },
  ];
  controller.dispatch({ type: "snapshot", snapshot: backgroundToolOnly });
  assert.equal(view.elements.emptyState.hidden, false);

  const streaming = snapshot("s1");
  streaming.tasks[0] = {
    ...streaming.tasks[0],
    status: "running",
    activeTurnId: "turn-stream",
  };
  streaming.messages = [
    { id: "m1", taskId: "s1", role: "assistant", text: "first" },
    { id: "m2", taskId: "s1", role: "assistant", text: "second" },
  ];
  streaming.tools = [
    { id: "current-tool", taskId: "s1", status: "completed", summary: "Current" },
  ];
  controller.dispatch({ type: "snapshot", snapshot: streaming });
  const listRenderCount = view.elements.messageList.replaceCount;
  const toolRenderCount = view.elements.toolRegion.replaceCount;
  const firstArticle = view.elements.messageList.children[0];
  const secondArticle = view.elements.messageList.children[1];
  const firstBody = firstArticle.children[1];
  const secondBody = secondArticle.children[1];
  const firstBodyRenderCount = firstBody.replaceCount;
  const secondBodyRenderCount = secondBody.replaceCount;

  controller.dispatch({
    type: "message/delta",
    params: {
      taskId: "s1",
      turnId: "turn-stream",
      messageId: "m2",
      role: "assistant",
      delta: " appended",
    },
  });
  assert.equal(view.elements.messageList.replaceCount, listRenderCount);
  assert.equal(view.elements.toolRegion.replaceCount, toolRenderCount);
  assert.equal(view.elements.messageList.children[0], firstArticle);
  assert.equal(view.elements.messageList.children[1], secondArticle);
  assert.equal(firstBody.replaceCount, firstBodyRenderCount);
  assert.equal(secondBody.replaceCount, secondBodyRenderCount + 1);
  assert.equal(secondBody.textContent, "second appended");

  const renderCountAfterAcceptedDelta = secondBody.replaceCount;
  controller.dispatch({
    type: "message/delta",
    params: { taskId: "s1", messageId: "m2", delta: " rejected" },
  });
  assert.equal(secondBody.replaceCount, renderCountAfterAcceptedDelta);
  assert.equal(secondBody.textContent, "second appended");
}

async function testSelectionSingleFlightFailureAndSuccess(App) {
  let activeGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    return message.method === "task/select"
      ? activeGate.promise
      : Promise.resolve({ ok: true, result: {} });
  });
  const storage = makeStorage({ zodePanelCurrentTask: "s1" });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage, view });
  view.bind(controller);
  await controller.start();

  const first = controller.selectTask("s2");
  const second = controller.selectTask("s2");
  const firstSettled = first.then(
    () => ({ status: "fulfilled" }),
    (error) => ({ status: "rejected", error }),
  );
  const secondSettled = second.then(
    () => ({ status: "fulfilled" }),
    (error) => ({ status: "rejected", error }),
  );
  await flushAsync();
  const exposesSelecting = typeof controller.isSelecting === "function";
  const selectingWhilePending = exposesSelecting && controller.isSelecting();
  const pendingMenuValue = view.elements.taskMenu.value;
  const pendingMenuDisabled = view.elements.taskMenu.disabled;
  const requestsBeforeFailure = runtime.calls.filter((call) => call.method === "task/select").length;
  activeGate.reject(new Error("selection rejected"));
  const failures = await Promise.all([firstSettled, secondSettled]);

  assert.equal(exposesSelecting, true);
  assert.equal(selectingWhilePending, true);
  assert.equal(pendingMenuValue, "s1");
  assert.equal(pendingMenuDisabled, true);
  assert.equal(requestsBeforeFailure, 1);
  assert.equal(failures.every((result) => result.status === "rejected"), true);
  assert.equal(controller.getState().currentTaskId, "s1");
  assert.equal(storage.data.zodePanelCurrentTask, "s1");
  assert.match(controller.getState().error.message, /selection rejected/);
  assert.equal(controller.isSelecting(), false);

  activeGate = deferred();
  const success = controller.selectTask("s2");
  await flushAsync();
  assert.equal(controller.getState().currentTaskId, "s1");
  activeGate.resolve({ ok: true, result: {} });
  await success;
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(storage.data.zodePanelCurrentTask, "s2");
}

async function testCreateModelAndAccessSingleFlight(App, State) {
  const gates = {
    "task/create": deferred(),
    "model/set": deferred(),
    "permission/set": deferred(),
  };
  const runtime = makeRuntime((message) =>
    gates[message.method]
      ? gates[message.method].promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });

  const createFirst = controller.createTask();
  const createSecond = controller.createTask();
  const createSettled = [createFirst, createSecond].map((promise) =>
    promise.then(
      () => ({ status: "fulfilled" }),
      (error) => ({ status: "rejected", error }),
    ),
  );
  await flushAsync();
  const exposesCreating = typeof controller.isCreating === "function";
  const creatingPending = exposesCreating && controller.isCreating();
  const createButtonDisabled = view.elements.newTask.disabled;
  controller.dispatch({ type: "selection/set", taskId: "s2" });
  gates["task/create"].reject(new Error("create rejected"));
  await Promise.all(createSettled);
  assert.equal(exposesCreating, true);
  assert.equal(creatingPending, true);
  assert.equal(createButtonDisabled, true);
  assert.equal(runtime.calls.filter((call) => call.method === "task/create").length, 1);
  assert.equal(controller.isCreating(), false);
  assert.equal(controller.getState().error.taskId, "s1");

  controller.dispatch({ type: "selection/set", taskId: "s1" });
  const modelFirst = controller.setModel("m2");
  const modelSecond = controller.setModel("ignored-model");
  await flushAsync();
  assert.equal(typeof controller.isSettingModel, "function");
  assert.equal(controller.isSettingModel("s1"), true);
  assert.equal(view.elements.model.value, "m1");
  assert.equal(view.elements.model.disabled, true);
  assert.equal(runtime.calls.filter((call) => call.method === "model/set").length, 1);
  controller.dispatch({ type: "selection/set", taskId: "s2" });
  gates["model/set"].resolve({ ok: true, result: {} });
  await Promise.all([modelFirst, modelSecond]);
  assert.equal(controller.isSettingModel("s1"), false);
  assert.equal(State.taskById(controller.getState(), "s1").model, "m2");
  assert.equal(State.taskById(controller.getState(), "s2").model, "m1");
  assert.equal(controller.getState().currentTaskId, "s2");

  const accessFirst = controller.setAccess("auto");
  const accessSecond = controller.setAccess("ignored-access");
  const accessSettled = [accessFirst, accessSecond].map((promise) =>
    promise.then(
      () => ({ status: "fulfilled" }),
      (error) => ({ status: "rejected", error }),
    ),
  );
  await flushAsync();
  assert.equal(typeof controller.isSettingAccess, "function");
  assert.equal(controller.isSettingAccess("s2"), true);
  assert.equal(view.elements.access.value, "readOnly");
  assert.equal(view.elements.access.disabled, true);
  assert.equal(runtime.calls.filter((call) => call.method === "permission/set").length, 1);
  controller.dispatch({ type: "selection/set", taskId: "s1" });
  gates["permission/set"].reject(new Error("access rejected"));
  await Promise.all(accessSettled);
  assert.equal(controller.isSettingAccess("s2"), false);
  assert.equal(controller.getState().error.taskId, "s2");
  assert.equal(controller.getState().currentTaskId, "s1");
}

async function testNavigationAndMutationFlightsAreShared(App, State) {
  const navigationGates = {
    "task/select": deferred(),
    "task/create": deferred(),
  };
  const navigationRuntime = makeRuntime((message) =>
    navigationGates[message.method]
      ? navigationGates[message.method].promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const navigationDocument = new FakeUiDocument();
  const navigationView = App.createDomView(navigationDocument);
  const navigation = App.createController({
    runtime: navigationRuntime,
    storage: makeStorage(),
    view: navigationView,
  });
  navigationView.bind(navigation);
  navigation.dispatch({ type: "connected" });
  navigation.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  await navigation.setDraft("navigation draft");

  const selecting = navigation.selectTask("s2");
  const creating = Promise.resolve().then(() => navigation.createTask());
  const navigationSettled = [selecting, creating].map((promise) =>
    promise.then(
      () => ({ status: "fulfilled" }),
      (error) => ({ status: "rejected", error }),
    ),
  );
  await flushAsync();
  const navigationRequestCount = navigationRuntime.calls.filter(
    (call) => call.method === "task/select" || call.method === "task/create",
  ).length;
  const exposesNavigation = typeof navigation.isNavigating === "function";
  const navigationPending = exposesNavigation && navigation.isNavigating();
  const navigationControlsDisabled = [
    navigationView.elements.taskMenu.disabled,
    navigationView.elements.newTask.disabled,
    navigationView.elements.model.disabled,
    navigationView.elements.access.disabled,
    navigationView.elements.send.disabled,
  ];
  navigationGates["task/select"].resolve({ ok: true, result: {} });
  navigationGates["task/create"].resolve({
    ok: true,
    result: { task: { id: "s3", title: "Third", status: "idle" } },
  });
  await Promise.all(navigationSettled);
  assert.equal(exposesNavigation, true);
  assert.equal(navigationPending, true);
  assert.equal(navigationRequestCount, 1);
  assert.equal(navigationControlsDisabled.every(Boolean), true);
  assert.equal(navigation.isNavigating(), false);

  const mutationGates = {
    "model/set": deferred(),
    "permission/set": deferred(),
    "turn/start": deferred(),
  };
  const mutationRuntime = makeRuntime((message) =>
    mutationGates[message.method]
      ? mutationGates[message.method].promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const mutationDocument = new FakeUiDocument();
  const mutationView = App.createDomView(mutationDocument);
  const mutation = App.createController({
    runtime: mutationRuntime,
    storage: makeStorage(),
    view: mutationView,
  });
  mutationView.bind(mutation);
  mutation.dispatch({ type: "connected" });
  mutation.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  await mutation.setDraft("mutation draft");

  const model = mutation.setModel("m2");
  await flushAsync();
  const exposesMutation = typeof mutation.isMutating === "function";
  const mutationPending = exposesMutation && mutation.isMutating("s1");
  const mutationControlsDisabled = [
    mutationView.elements.taskMenu.disabled,
    mutationView.elements.newTask.disabled,
    mutationView.elements.model.disabled,
    mutationView.elements.access.disabled,
    mutationView.elements.send.disabled,
  ];
  const access = Promise.resolve().then(() => mutation.setAccess("auto"));
  const submit = Promise.resolve().then(() => mutation.submit());
  const mutationSettled = [model, access, submit].map((promise) =>
    promise.then(
      () => ({ status: "fulfilled" }),
      (error) => ({ status: "rejected", error }),
    ),
  );
  await flushAsync();
  const mutationRequestCount = mutationRuntime.calls.filter((call) =>
    ["model/set", "permission/set", "turn/start"].includes(call.method),
  ).length;
  mutationGates["model/set"].resolve({ ok: true, result: {} });
  mutationGates["permission/set"].resolve({ ok: true, result: {} });
  mutationGates["turn/start"].resolve({ ok: true, result: { turnId: "blocked-turn" } });
  await Promise.all(mutationSettled);
  assert.equal(exposesMutation, true);
  assert.equal(mutationPending, true);
  assert.equal(mutationControlsDisabled.every(Boolean), true);
  assert.equal(mutationRequestCount, 1);
  assert.equal(mutation.isMutating("s1"), false);
  assert.equal(State.taskById(mutation.getState(), "s1").model, "m2");
  assert.equal(mutation.getState().drafts.s1, "mutation draft");
}

async function testInterruptIsSingleFlightAndRollsBackOnce(App, State) {
  const interruptGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "turn/interrupt"
      ? interruptGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const actions = [];
  const controller = App.createController({
    runtime,
    storage: makeStorage(),
    render: (state, activeController, action) => {
      actions.push(action);
      view.render(state, activeController, action);
    },
  });
  view.bind(controller);
  const running = snapshot("s1");
  running.tasks[0] = { ...running.tasks[0], status: "running", activeTurnId: "turn-stop" };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: running });

  const first = controller.submit();
  const second = controller.submit();
  const settled = [first, second].map((promise) =>
    promise.then(
      () => ({ status: "fulfilled" }),
      (error) => ({ status: "rejected", error }),
    ),
  );
  await flushAsync();
  const exposesInterrupting = typeof controller.isInterrupting === "function";
  const interrupting = exposesInterrupting && controller.isInterrupting("s1");
  const requestCount = runtime.calls.filter((call) => call.method === "turn/interrupt").length;
  const sendLabel = view.elements.send.textContent;
  const sendDisabled = view.elements.send.disabled;
  interruptGate.reject(new Error("interrupt failed once"));
  const results = await Promise.all(settled);

  assert.equal(exposesInterrupting, true);
  assert.equal(interrupting, true);
  assert.equal(requestCount, 1);
  assert.equal(sendLabel, "Stopping");
  assert.equal(sendDisabled, true);
  assert.equal(results.every((result) => result.status === "rejected"), true);
  assert.equal(actions.filter((action) => action.type === "task/updated").length, 1);
  assert.equal(actions.filter((action) => action.type === "error/set").length, 1);
  assert.equal(State.taskById(controller.getState(), "s1").status, "running");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "turn-stop");
  assert.equal(controller.isInterrupting("s1"), false);
}

async function testAcknowledgedInterruptCannotBeSentAgainWhileStopping(App, State) {
  const interruptGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "turn/interrupt"
      ? interruptGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  const running = snapshot("s1");
  running.tasks[0] = { ...running.tasks[0], status: "running", activeTurnId: "turn-stop" };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: running });

  const stopping = controller.submit();
  await flushAsync();
  interruptGate.resolve({ ok: true, result: {} });
  await stopping;
  assert.equal(State.taskById(controller.getState(), "s1").status, "stopping");
  assert.equal(view.elements.send.textContent, "Stopping");
  assert.equal(view.elements.send.disabled, true);
  await controller.submit();
  assert.equal(runtime.calls.filter((call) => call.method === "turn/interrupt").length, 1);
}

async function testInterruptRequiresAnAuthoritativeTurnId(App, State) {
  const runtime = makeRuntime(() => Promise.resolve({ ok: true, result: {} }));
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  const withoutTurn = snapshot("s1");
  withoutTurn.tasks[0] = {
    ...withoutTurn.tasks[0],
    status: "running",
    activeTurnId: null,
  };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: withoutTurn });

  await assert.rejects(() => controller.submit(), /turn id/i);
  assert.equal(runtime.calls.some((call) => call.method === "turn/interrupt"), false);
  assert.match(State.errorForCurrent(controller.getState()).message, /刷新任务快照/);
  assert.equal(State.taskById(controller.getState(), "s1").status, "running");
  assert.equal(controller.isInterrupting("s1"), false);
}

async function testStoppingWithoutTurnIdIsAnIdempotentNoOp(App, State) {
  const runtime = makeRuntime(() => Promise.resolve({ ok: true, result: {} }));
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  const stopping = snapshot("s1");
  stopping.tasks[0] = {
    ...stopping.tasks[0],
    status: "stopping",
    activeTurnId: null,
  };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: stopping });

  assert.equal(await controller.submit(), null);
  assert.equal(await controller.submit(), null);
  assert.equal(runtime.calls.some((call) => call.method === "turn/interrupt"), false);
  assert.equal(State.errorForCurrent(controller.getState()), null);
  assert.equal(State.taskById(controller.getState(), "s1").status, "stopping");
  assert.equal(controller.isInterrupting("s1"), false);
}

async function testTaskSettingsRejectWhileTurnStartIsPending(App) {
  const turnGate = deferred();
  const runtime = makeRuntime((message) =>
    message.method === "turn/start"
      ? turnGate.promise
      : Promise.resolve({ ok: true, result: {} }),
  );
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  await controller.setDraft("pending turn");
  const submit = controller.submit();
  await flushAsync();
  await assert.rejects(() => controller.setModel("m2"), /busy/i);
  await assert.rejects(() => controller.setAccess("auto"), /busy/i);
  assert.equal(runtime.calls.some((call) => call.method === "model/set"), false);
  assert.equal(runtime.calls.some((call) => call.method === "permission/set"), false);
  turnGate.resolve({ ok: true, result: { turnId: "pending-turn" } });
  await submit;
}

async function testInterruptFailureRollsBackRunning(App, State) {
  const runtime = makeRuntime((message) =>
    message.method === "turn/interrupt"
      ? Promise.reject(new Error("interrupt rejected"))
      : Promise.resolve({ ok: true, result: {} }),
  );
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  const running = snapshot("s1");
  running.tasks[0] = { ...running.tasks[0], status: "running", activeTurnId: "turn-live" };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: running });
  await assert.rejects(() => controller.submit(), /interrupt rejected/);
  assert.equal(State.taskById(controller.getState(), "s1").status, "running");
  assert.equal(State.taskById(controller.getState(), "s1").activeTurnId, "turn-live");
  assert.equal(controller.primaryAction(), "stop");
}

async function testSlowHydrationAndNewTaskDraftMigration(App, State) {
  const hydration = deferred();
  const hydrationData = {};
  const hydrationSets = [];
  const storage = {
    data: hydrationData,
    get() {
      return hydration.promise;
    },
    set(values) {
      hydrationSets.push(plain(values));
      Object.assign(hydrationData, plain(values));
      return Promise.resolve();
    },
  };
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: false } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage, render: () => {} });
  await controller.setDraft("fresh new-task draft");
  await controller.toggleTool("fresh-tool");
  assert.equal(hydrationSets.length, 0);
  const starting = controller.start();
  hydration.resolve({
    zodePanelCurrentTask: "s1",
    zodePanelDrafts: {
      s1: "persisted draft",
      s2: "other persisted draft",
      [State.NEW_TASK_DRAFT]: "stale draft",
    },
    zodePanelCollapsedTools: ["persisted-tool"],
  });
  await starting;
  assert.equal(controller.getState().drafts.s1, "fresh new-task draft");
  assert.equal(controller.getState().drafts.s2, "other persisted draft");
  assert.equal(Object.hasOwn(controller.getState().drafts, State.NEW_TASK_DRAFT), false);
  assert.equal(hydrationSets.length, 1);
  assert.deepEqual(hydrationData.zodePanelDrafts, {
    s1: "fresh new-task draft",
    s2: "other persisted draft",
  });
  assert.deepEqual(hydrationData.zodePanelCollapsedTools.sort(), ["fresh-tool", "persisted-tool"]);
  assert.equal(hydrationData.zodePanelCurrentTask, "s1");

  const createStorage = makeStorage();
  const createRuntime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot(null) });
    }
    return message.method === "task/create"
      ? Promise.resolve({
          ok: true,
          result: { task: { id: "s3", title: "Third", status: "idle", model: "m1" } },
        })
      : Promise.resolve({ ok: true, result: {} });
  });
  const creating = App.createController({
    runtime: createRuntime,
    storage: createStorage,
    render: () => {},
  });
  await creating.start();
  await creating.setDraft("prompt for created task");
  await creating.createTask();
  assert.equal(creating.getState().currentTaskId, "s3");
  assert.equal(creating.getState().drafts.s3, "prompt for created task");
  assert.equal(Object.hasOwn(creating.getState().drafts, State.NEW_TASK_DRAFT), false);
  assert.equal(createStorage.data.zodePanelDrafts.s3, "prompt for created task");
}

async function testRetryConnectionIsSingleFlightAndPreservesDraft(App) {
  const statusGate = deferred();
  const snapshotGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return statusGate.promise;
    }
    if (message.type === "zode-reconnect") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return snapshotGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  controller.dispatch({ type: "disconnected", message: "offline" });
  await controller.setDraft("keep while retrying");

  const first = controller.retryConnection();
  const second = controller.retryConnection();
  await flushAsync();
  assert.equal(typeof controller.isRetrying, "function");
  assert.equal(controller.isRetrying(), true);
  assert.equal(runtime.calls.filter((call) => call.type === "zode-status").length, 1);
  assert.equal(view.elements.retry.disabled, true);
  assert.equal(controller.currentDraft(), "keep while retrying");

  statusGate.resolve({ ok: true, status: { connected: false, canReconnect: true } });
  await flushAsync();
  assert.equal(controller.getState().connection, "checking");
  assert.equal(view.elements.send.disabled, true);
  snapshotGate.resolve({ ok: true, result: snapshot("s1") });
  await Promise.all([first, second]);
  assert.deepEqual(
    runtime.calls.map((call) => call.type === "zode-task-request" ? call.method : call.type),
    ["zode-status", "zode-reconnect", "snapshot/read"],
  );
  assert.equal(controller.getState().connection, "connected");
  assert.equal(controller.currentDraft(), "keep while retrying");
  assert.equal(controller.isRetrying(), false);
  assert.equal(view.elements.connectionBanner.hidden, true);

  const failingRuntime = makeRuntime((message) =>
    message.type === "zode-status"
      ? Promise.resolve({ ok: true, status: { connected: true } })
      : Promise.resolve({ ok: false, error: "still offline" }),
  );
  const failing = App.createController({ failing: true, runtime: failingRuntime, render: () => {} });
  failing.dispatch({ type: "disconnected", message: "offline" });
  await assert.doesNotReject(() => failing.retryConnection());
  assert.equal(failing.getState().connection, "disconnected");
  assert.match(failing.getState().connectionMessage, /still offline/);
}

async function testAuthenticatedEventAndReconnectStatusShareSnapshotRead(App) {
  const snapshotGate = deferred();
  let snapshotReads = 0;
  let runtime;
  runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: false, canReconnect: true } });
    }
    if (message.type === "zode-reconnect") {
      runtime.listeners[0]({
        type: "zode-task-connected",
        connectionId: "connection-1",
        protocolVersion: 1,
      });
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: true,
          taskConnectionId: "connection-1",
        },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      return snapshotGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });

  const starting = controller.start();
  await flushAsync();
  assert.equal(
    snapshotReads,
    1,
    "connected event and matching reconnect status did not share their snapshot read",
  );

  snapshotGate.resolve({ ok: true, result: snapshot("s1") });
  await starting;
  assert.equal(controller.getState().connection, "connected");
}

async function testReconnectStatusAndLaterAuthenticatedEventShareSnapshotRead(App) {
  const snapshotGate = deferred();
  let snapshotReads = 0;
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: false, canReconnect: true } });
    }
    if (message.type === "zode-reconnect") {
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: true,
          taskConnectionId: "connection-1",
        },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      return snapshotGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });

  const starting = controller.start();
  for (let attempt = 0; attempt < 20 && snapshotReads < 1; attempt += 1) {
    await flushAsync();
  }
  assert.equal(snapshotReads, 1);
  runtime.listeners[0]({
    type: "zode-task-connected",
    connectionId: "connection-1",
    protocolVersion: 1,
  });
  await flushAsync();
  assert.equal(
    snapshotReads,
    1,
    "matching connected event did not reuse the status response's snapshot read",
  );

  snapshotGate.resolve({ ok: true, result: snapshot("s1") });
  await starting;
  assert.equal(controller.getState().connection, "connected");
}

function testMessageDeltasAreCoalescedWithAnimationFrames(App) {
  const frames = [];
  const document = new FakeUiDocument();
  document.defaultView = {
    requestAnimationFrame(callback) {
      frames.push(callback);
      return frames.length;
    },
  };
  const view = App.createDomView(document);
  const controller = App.createController({ runtime: makeRuntime(() => Promise.resolve({ ok: true })), view });
  view.bind(controller);
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  controller.dispatch({ type: "turn/started", params: { taskId: "s1", turnId: "turn-frame" } });

  controller.dispatch({
    type: "message/delta",
    params: { taskId: "s1", turnId: "turn-frame", messageId: "a-frame", delta: "hel" },
  });
  controller.dispatch({
    type: "message/delta",
    params: { taskId: "s1", turnId: "turn-frame", messageId: "a-frame", delta: "lo" },
  });

  assert.equal(frames.length, 1);
  assert.equal(view.elements.messageList.children.length, 0);
  frames.shift()();
  assert.equal(view.elements.messageList.children.length, 1);
  assert.match(view.elements.messageList.textContent, /hello/);
}

function testUnavailableMoreActionStaysHiddenAndDisabled(App) {
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime: makeRuntime(() => Promise.resolve({ ok: true })), view });
  view.bind(controller);
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  assert.equal(view.elements.more.hidden, true);
  assert.equal(view.elements.more.disabled, true);
  assert.equal((view.elements.more.listeners.click || []).length, 0);
}

async function testSwitchingTaskLocksMutationsButPreservesDraft(App) {
  const runtime = makeRuntime(() => Promise.resolve({ ok: true, result: {} }));
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  const switching = snapshot("s1");
  switching.tasks[0] = { ...switching.tasks[0], status: "switching" };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: switching });
  await controller.setDraft("keep switching draft");

  await assert.rejects(() => controller.setModel("m2"), /switching|busy/i);
  await assert.rejects(() => controller.setAccess("auto"), /switching|busy/i);
  await assert.rejects(() => controller.submit(), /switching|busy/i);
  assert.equal(runtime.calls.some((call) => call.type === "zode-task-request"), false);
  assert.equal(controller.currentDraft(), "keep switching draft");
  assert.equal(controller.primaryAction(), "loading");
}

async function testSwitchingTaskDomDisablesSendAndEnterButKeepsComposerEditable(App) {
  const runtime = makeRuntime(() => Promise.resolve({ ok: true, result: {} }));
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime, storage: makeStorage(), view });
  view.bind(controller);
  const switching = snapshot("s1");
  switching.tasks[0] = { ...switching.tasks[0], status: "switching" };
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: switching });

  view.elements.composer.value = "editable while loading";
  view.elements.composer.emit("input");
  await flushAsync();
  assert.equal(view.elements.composer.disabled, false);
  assert.equal(view.elements.model.disabled, true);
  assert.equal(view.elements.access.disabled, true);
  assert.equal(view.elements.send.disabled, true);
  assert.equal(view.elements.send.textContent, "Loading");
  assert.equal(view.elements.send.attributes["aria-label"], "Task is loading");
  assert.equal(view.elements.taskMenu.disabled, false);
  assert.equal(view.elements.newTask.disabled, false);

  const enter = view.elements.composer.emit("keydown", {
    key: "Enter",
    shiftKey: false,
    isComposing: false,
  });
  view.elements.composerForm.emit("submit");
  await flushAsync();
  assert.equal(enter.defaultPrevented, true);
  assert.equal(runtime.calls.some((call) => call.type === "zode-task-request"), false);
  assert.equal(controller.currentDraft(), "editable while loading");
}

async function testCreateAndSelectCanReturnSwitchingTasks(App) {
  const runtime = makeRuntime((message) => {
    if (message.method === "task/create") {
      return Promise.resolve({
        ok: true,
        result: { task: { id: "s3", title: "Restoring", status: "switching", model: "m1" } },
      });
    }
    if (message.method === "task/select") {
      const selected = snapshot("s2");
      selected.tasks[1] = { ...selected.tasks[1], status: "switching" };
      return Promise.resolve({ ok: true, result: { snapshot: selected } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });

  await controller.createTask();
  assert.equal(controller.getState().currentTaskId, "s3");
  assert.equal(controller.primaryAction(), "loading");
  await assert.rejects(() => controller.submit(), /switching|busy/i);

  await controller.selectTask("s2");
  assert.equal(controller.getState().currentTaskId, "s2");
  assert.equal(controller.primaryAction(), "loading");
  await assert.rejects(() => controller.setModel("m2"), /switching|busy/i);
}

async function testDomFiltersBackgroundFeedback(App) {
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = App.createController({ runtime: makeRuntime(() => Promise.resolve({ ok: true })), view });
  view.bind(controller);
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  controller.dispatch({ type: "error/set", taskId: "s2", message: "background failed" });
  assert.equal(view.elements.errorRegion.hidden, true);
  controller.dispatch({ type: "selection/set", taskId: "s2" });
  assert.equal(view.elements.errorRegion.hidden, false);
  assert.match(view.elements.errorRegion.textContent, /background failed/);
}

function fakeFile(name, type, size, options = {}) {
  let bytes = options.bytes || null;
  return {
    name,
    type,
    size,
    arrayBuffer() {
      if (options.arrayBuffer) {
        return options.arrayBuffer();
      }
      if (!bytes) {
        bytes = new Uint8Array(size);
        for (let index = 0; index < bytes.length; index += 1) {
          bytes[index] = index % 251;
        }
      }
      return Promise.resolve(bytes.slice().buffer);
    },
  };
}

function connectedAttachmentController(App, runtime, view = null) {
  const controller = App.createController({
    runtime,
    storage: makeStorage(),
    ...(view ? { view } : { render: () => {} }),
  });
  if (view) {
    view.bind(controller);
  }
  controller.dispatch({ type: "connected" });
  controller.dispatch({ type: "snapshot", snapshot: snapshot("s1") });
  return controller;
}

async function captureAttachmentBegin(App, file) {
  let begunMetadata = null;
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      begunMetadata = plain(message.params);
      return Promise.resolve({
        ok: true,
        result: {
          uploadId: "capture-upload",
          name: message.params.name,
          mediaType: message.params.mediaType,
          size: message.params.size,
        },
      });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({
        ok: true,
        result: { uploadId: message.params.uploadId, nextSequence: message.params.sequence + 1 },
      });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({
        ok: true,
        result: {
          attachmentId: "capture-attachment",
          name: begunMetadata.name,
          mediaType: begunMetadata.mediaType,
          size: begunMetadata.size,
        },
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  await controller.addFiles([file]);
  return {
    attachment: controller.getAttachments()[0],
    begin: runtime.calls.find((call) => call.method === "attachment/begin") || null,
  };
}

async function testAttachmentMimeNormalizationIsAllowlistDriven(App) {
  const textTypes = [
    ["rs", "application/octet-stream"],
    ["js", "text/javascript"],
    ["ts", "video/mp2t"],
    ["tsx", "application/octet-stream"],
    ["jsx", "text/jsx"],
    ["json", "application/json"],
    ["md", "text/markdown"],
    ["toml", "application/toml"],
    ["yaml", "application/yaml"],
    ["yml", "text/yaml"],
    ["css", "text/css"],
    ["html", "text/html"],
    ["sh", "application/x-sh"],
    ["py", "text/x-python"],
    ["go", "text/x-go"],
    ["java", "text/x-java-source"],
    ["kt", "text/x-kotlin"],
  ];
  for (const [extension, typicalType] of textTypes) {
    for (const browserType of ["", typicalType, "application/x-zode-weird"]) {
      const { begin, attachment } = await captureAttachmentBegin(
        App,
        fakeFile(`source.${extension}`, browserType, 1),
      );
      assert.ok(begin, `.${extension} (${browserType || "empty"}) was rejected`);
      assert.equal(
        begin.params.mediaType,
        "text/plain",
        `.${extension} forwarded browser MIME ${browserType}`,
      );
      assert.equal(attachment.status, "ready");
      assert.equal(attachment.mime, "text/plain");
    }
  }

  const imageTypes = [
    ["png", "image/png"],
    ["jpg", "image/jpeg"],
    ["jpeg", "image/jpeg"],
    ["gif", "image/gif"],
    ["webp", "image/webp"],
  ];
  for (const [extension, canonicalType] of imageTypes) {
    for (const browserType of ["", canonicalType, "application/octet-stream"]) {
      const { begin, attachment } = await captureAttachmentBegin(
        App,
        fakeFile(`image.${extension}`, browserType, 1),
      );
      assert.ok(begin, `.${extension} (${browserType || "empty"}) was rejected`);
      assert.equal(begin.params.mediaType, canonicalType);
      assert.equal(attachment.mime, canonicalType);
    }
  }
  const conflicted = await captureAttachmentBegin(
    App,
    fakeFile("conflict.png", "image/jpeg", 1),
  );
  assert.equal(conflicted.begin.params.mediaType, "image/png");

  for (const [name, browserType, accepted] of [
    ["notes.txt", "text/plain", true],
    ["rows.csv", "text/csv", true],
    ["notes.rtf", "text/rtf", false],
    ["notes.rtf", "text/plain", false],
    ["blob.bin", "text/plain; charset=utf-8", false],
    ["blob.bin", " text/plain ", false],
    ["blob.bin", "text/pla in", false],
    ["blob.bin", "application/octet-stream", false],
  ]) {
    const { begin, attachment } = await captureAttachmentBegin(
      App,
      fakeFile(name, browserType, 1),
    );
    assert.equal(Boolean(begin), accepted, `${name} (${browserType}) acceptance mismatch`);
    if (accepted) {
      assert.equal(begin.params.mediaType, "text/plain");
      assert.equal(attachment.status, "ready");
    } else {
      assert.equal(attachment.status, "error");
      assert.match(attachment.error, /unsupported/i);
    }
  }
}

async function testAttachmentUploadChunksSequentiallyAndSubmitsIds(App) {
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({
        ok: true,
        result: {
          uploadId: "upload-1",
          name: message.params.name,
          mediaType: message.params.mediaType,
          size: message.params.size,
        },
      });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({
        ok: true,
        result: { uploadId: message.params.uploadId, nextSequence: message.params.sequence + 1 },
      });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "attachment-1" } });
    }
    if (message.method === "turn/start") {
      return Promise.resolve({ ok: true, result: { turnId: "turn-with-file" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  const size = 600 * 1024;

  await controller.addFiles([fakeFile("main.rs", "text/plain", size)]);

  const taskCalls = runtime.calls.filter((call) => call.type === "zode-task-request");
  assert.deepEqual(taskCalls[0], {
    type: "zode-task-request",
    method: "attachment/begin",
    params: { taskId: "s1", name: "main.rs", mediaType: "text/plain", size },
  });
  const chunks = taskCalls.filter((call) => call.method === "attachment/chunk");
  assert.deepEqual(chunks.map((call) => call.params.sequence), [0, 1, 2]);
  assert.deepEqual(
    chunks.map((call) => Buffer.from(call.params.data, "base64").length),
    [256 * 1024, 256 * 1024, 88 * 1024],
  );
  assert.equal(chunks.every((call) => call.params.uploadId === "upload-1"), true);
  assert.deepEqual(
    plain(taskCalls.find((call) => call.method === "attachment/finish").params),
    { uploadId: "upload-1" },
  );
  assert.deepEqual(
    plain(controller.getAttachments()),
    [
      {
        localId: "file-1",
        name: "main.rs",
        mime: "text/plain",
        size,
        status: "ready",
        uploadId: "upload-1",
        attachmentId: "attachment-1",
        error: null,
      },
    ],
  );

  await controller.setDraft("inspect this file");
  await controller.submit();
  assert.deepEqual(runtime.calls.at(-1), {
    type: "zode-task-request",
    method: "turn/start",
    params: {
      taskId: "s1",
      input: "inspect this file",
      attachmentIds: ["attachment-1"],
    },
  });
  assert.equal(controller.currentDraft(), "");
  assert.deepEqual(plain(controller.getAttachments()), []);
}

async function testUnsupportedAttachmentsKeepDraftAndReadyIdsForRetry(App) {
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "unsupported-upload" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "unsupported-attachment" } });
    }
    if (message.method === "turn/start") {
      return Promise.resolve({
        ok: false,
        code: "attachments_not_supported",
        error: "attachments are not supported by this zode version",
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  await controller.addFiles([fakeFile("retry.rs", "text/plain", 4)]);
  await controller.setDraft("keep the retry payload");

  await assert.rejects(() => controller.submit(), (error) => {
    assert.equal(error.code, "attachments_not_supported");
    return true;
  });

  assert.equal(controller.currentDraft(), "keep the retry payload");
  assert.deepEqual(
    plain(controller.getAttachments()).map((attachment) => ({
      name: attachment.name,
      status: attachment.status,
      attachmentId: attachment.attachmentId,
    })),
    [
      {
        name: "retry.rs",
        status: "ready",
        attachmentId: "unsupported-attachment",
      },
    ],
  );
  assert.match(controller.getState().errorsByTask.s1.message, /upgrade|升级/i);
  assert.match(controller.getState().errorsByTask.s1.message, /remove|移除/i);
}

async function testAttachmentChunkBoundaries(App) {
  let uploadSequence = 0;
  const uploads = new Map();
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      const uploadId = `boundary-${++uploadSequence}`;
      uploads.set(uploadId, plain(message.params));
      return Promise.resolve({
        ok: true,
        result: { uploadId, ...plain(message.params) },
      });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({
        ok: true,
        result: { uploadId: message.params.uploadId, nextSequence: message.params.sequence + 1 },
      });
    }
    if (message.method === "attachment/finish") {
      const metadata = uploads.get(message.params.uploadId);
      return Promise.resolve({
        ok: true,
        result: {
          attachmentId: `attachment-${message.params.uploadId}`,
          name: metadata.name,
          mediaType: metadata.mediaType,
          size: metadata.size,
        },
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  const chunk = 256 * 1024;

  await controller.addFiles([
    fakeFile("one.rs", "application/octet-stream", 1),
    fakeFile("exact.rs", "application/octet-stream", chunk),
    fakeFile("over.rs", "application/octet-stream", chunk + 1),
  ]);

  const chunksByUpload = new Map();
  for (const call of runtime.calls.filter((item) => item.method === "attachment/chunk")) {
    const chunks = chunksByUpload.get(call.params.uploadId) || [];
    chunks.push({
      sequence: call.params.sequence,
      size: Buffer.from(call.params.data, "base64").length,
    });
    chunksByUpload.set(call.params.uploadId, chunks);
  }
  assert.deepEqual(plain(chunksByUpload.get("boundary-1")), [{ sequence: 0, size: 1 }]);
  assert.deepEqual(plain(chunksByUpload.get("boundary-2")), [
    { sequence: 0, size: chunk },
  ]);
  assert.deepEqual(plain(chunksByUpload.get("boundary-3")), [
    { sequence: 0, size: chunk },
    { sequence: 1, size: 1 },
  ]);
}

async function testAttachmentFailureKeepsDraftAndOtherReadyChips(App) {
  let uploadNumber = 0;
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      uploadNumber += 1;
      return Promise.resolve({ ok: true, result: { uploadId: `upload-${uploadNumber}` } });
    }
    if (message.method === "attachment/chunk" && message.params.uploadId === "upload-2") {
      return Promise.resolve({ ok: false, code: "attachment_rejected", error: "chunk rejected" });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: message.params.sequence + 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "attachment-good" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = connectedAttachmentController(App, runtime, view);
  await controller.setDraft("keep this prompt");

  await controller.addFiles([
    fakeFile("good.ts", "text/plain", 4),
    fakeFile("bad.ts", "text/plain", 4),
  ]);

  assert.equal(controller.currentDraft(), "keep this prompt");
  const attachments = plain(controller.getAttachments());
  assert.deepEqual(attachments.map((item) => item.status), ["ready", "error"]);
  assert.equal(attachments[0].attachmentId, "attachment-good");
  assert.match(attachments[1].error, /chunk rejected/);
  assert.match(view.elements.attachmentList.textContent, /good\.ts/);
  assert.match(view.elements.attachmentList.textContent, /bad\.ts/);
  assert.match(view.elements.attachmentList.textContent, /chunk rejected/);
  assert.equal(
    runtime.calls.some(
      (call) =>
        call.method === "attachment/cancel" && call.params.uploadId === "upload-2",
    ),
    true,
  );
}

async function testAttachmentRemovalCancelsFinishedUpload(App) {
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "upload-remove" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "attachment-remove" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  await controller.addFiles([fakeFile("remove.md", "text/markdown", 3)]);
  const localId = controller.getAttachments()[0].localId;

  await controller.removeAttachment(localId);

  assert.deepEqual(plain(controller.getAttachments()), []);
  assert.deepEqual(runtime.calls.at(-1), {
    type: "zode-task-request",
    method: "attachment/cancel",
    params: { uploadId: "upload-remove" },
  });
}

async function testAttachmentClientValidationAndBounds(App) {
  const runtime = makeRuntime(() => Promise.resolve({ ok: true, result: {} }));
  const controller = connectedAttachmentController(App, runtime);
  const neverRead = () => Promise.reject(new Error("invalid file bytes must not be read"));

  await controller.addFiles([
    fakeFile("report.pdf", "application/pdf", 10, { arrayBuffer: neverRead }),
    fakeFile("huge.txt", "text/plain", 1024 * 1024 + 1, { arrayBuffer: neverRead }),
    fakeFile("huge.png", "image/png", 5 * 1024 * 1024 + 1, { arrayBuffer: neverRead }),
  ]);

  const rejected = plain(controller.getAttachments());
  assert.deepEqual(rejected.map((item) => item.status), ["error", "error", "error"]);
  assert.match(rejected[0].error, /unsupported/i);
  assert.match(rejected[1].error, /1 MiB/i);
  assert.match(rejected[2].error, /5 MiB/i);
  assert.equal(runtime.calls.some((call) => call.method === "attachment/begin"), false);

  const inferredRuntime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "inferred-image" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "inferred-attachment" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const inferred = connectedAttachmentController(App, inferredRuntime);
  await inferred.addFiles([fakeFile("SCREENSHOT.PNG", "", 1)]);
  assert.equal(
    inferredRuntime.calls.find((call) => call.method === "attachment/begin").params.mediaType,
    "image/png",
  );
  assert.equal(inferred.getAttachments()[0].status, "ready");

  let uploadNumber = 0;
  const boundedRuntime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      uploadNumber += 1;
      return Promise.resolve({ ok: true, result: { uploadId: `bounded-${uploadNumber}` } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: message.params.sequence + 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({
        ok: true,
        result: { attachmentId: `attachment-${message.params.uploadId}` },
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const bounded = connectedAttachmentController(App, boundedRuntime);
  await bounded.addFiles(
    Array.from({ length: 9 }, (_, index) =>
      fakeFile(`file-${index}.txt`, "text/plain", 0),
    ),
  );
  assert.equal(bounded.getAttachments().length, 8);
  assert.match(bounded.getAttachmentNotice(), /8/);
  assert.equal(
    boundedRuntime.calls.filter((call) => call.method === "attachment/begin").length,
    8,
  );

  const totalGate = deferred();
  const total = connectedAttachmentController(App, makeRuntime(() => Promise.resolve({ ok: true })));
  void total.addFiles([
    fakeFile("one.png", "image/png", 5 * 1024 * 1024, {
      arrayBuffer: () => totalGate.promise,
    }),
    fakeFile("two.webp", "image/webp", 5 * 1024 * 1024, {
      arrayBuffer: () => totalGate.promise,
    }),
    fakeFile("three.gif", "image/gif", 5 * 1024 * 1024, {
      arrayBuffer: () => totalGate.promise,
    }),
    fakeFile("four.jpg", "image/jpeg", 5 * 1024 * 1024, {
      arrayBuffer: () => totalGate.promise,
    }),
    fakeFile("five.txt", "text/plain", 1, { arrayBuffer: neverRead }),
  ]);
  const totalAttachments = plain(total.getAttachments());
  assert.deepEqual(totalAttachments.map((item) => item.status), [
    "queued",
    "queued",
    "queued",
    "queued",
    "error",
  ]);
  assert.match(totalAttachments[4].error, /20 MiB/i);
}

async function testAttachmentFailureRendersBeforeCleanupFinishes(App) {
  const cancelGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "slow-cancel" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: false, error: "upload rejected immediately" });
    }
    if (message.method === "attachment/cancel") {
      return cancelGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = connectedAttachmentController(App, runtime);
  await controller.setDraft("draft survives cleanup");
  const adding = controller.addFiles([fakeFile("bad.rs", "text/plain", 2)]);
  await flushAsync();

  assert.equal(controller.getAttachments()[0].status, "error");
  assert.match(controller.getAttachments()[0].error, /upload rejected immediately/);
  assert.equal(controller.currentDraft(), "draft survives cleanup");

  cancelGate.resolve({ ok: true, result: {} });
  await adding;
}

async function testAttachmentDisconnectInvalidatesFinishedIds(App) {
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "disconnect-upload" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "stale-attachment" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.setDraft("keep after disconnect");
  await controller.addFiles([fakeFile("stale.md", "text/markdown", 3)]);
  assert.equal(controller.getAttachments()[0].status, "ready");

  runtime.listeners[0]({ type: "zode-task-disconnected" });

  const stale = controller.getAttachments()[0];
  assert.equal(stale.status, "error");
  assert.equal(stale.attachmentId, null);
  assert.match(stale.error, /re-attach|connection/i);
  assert.equal(controller.currentDraft(), "keep after disconnect");
}

async function testConnectedSocketRefreshesAuthoritativeStateAndPreservesLocalWork(App) {
  const first = snapshot("s1");
  first.tasks[0] = {
    id: "s1",
    title: "Stale task",
    status: "idle",
    model: "m1",
    access: "prompt",
  };
  first.models = ["m1"];
  first.messages = [{ id: "old-message", role: "assistant", text: "stale" }];
  const fresh = snapshot("s1");
  fresh.tasks = [
    {
      id: "s1",
      title: "Authoritative task",
      status: "running",
      model: "m2",
      access: "auto",
      activeTurnId: "turn-fresh",
    },
  ];
  fresh.models = ["m2", "m3"];
  fresh.messages = [{ id: "fresh-message", role: "assistant", text: "fresh", order: 0 }];
  let snapshotReads = 0;
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: true,
          taskConnectionId: "connection-1",
        },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      return Promise.resolve({ ok: true, result: snapshotReads === 1 ? first : fresh });
    }
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "reconnect-upload" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "connection-1-attachment" } });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.setDraft("keep this draft across reconnect");
  await controller.addFiles([fakeFile("keep.rs", "text/plain", 2)]);
  assert.equal(controller.getAttachments()[0].status, "ready");

  runtime.listeners[0]({
    type: "zode-task-connected",
    connectionId: "connection-2",
    protocolVersion: 1,
  });
  for (let attempt = 0; attempt < 20 && snapshotReads < 2; attempt += 1) {
    await flushAsync();
  }

  assert.equal(snapshotReads, 2, "connected socket did not request a fresh snapshot");
  assert.equal(controller.getState().connection, "connected");
  assert.deepEqual(plain(controller.getState().models), ["m2", "m3"]);
  assert.deepEqual(plain(controller.getState().messages), fresh.messages);
  assert.deepEqual(plain(controller.getState().tasks), fresh.tasks);
  assert.equal(controller.getState().tasks[0].status, "running");
  assert.equal(controller.getState().tasks[0].access, "auto");
  assert.equal(controller.currentDraft(), "keep this draft across reconnect");
  const attachment = controller.getAttachments()[0];
  assert.equal(attachment.name, "keep.rs", "attachment chip disappeared after snapshot");
  assert.equal(attachment.status, "error");
  assert.equal(attachment.attachmentId, null);
  assert.match(attachment.error, /connection|re-attach/i);
}

async function testUnsupportedTaskProtocolShowsCompatibilityMessage(App) {
  const storage = makeStorage({ zodePanelDrafts: { s1: "draft remains editable" } });
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: false,
          taskConnectionId: null,
        },
      });
    }
    return Promise.resolve({ ok: true, result: snapshot("s1") });
  });
  const controller = App.createController({ runtime, storage, render: () => {} });

  await controller.start();

  assert.equal(controller.getState().connection, "disconnected");
  assert.equal(
    controller.getState().connectionMessage,
    "Current zode version does not support the task client",
  );
  assert.equal(
    runtime.calls.some((message) => message.method === "snapshot/read"),
    false,
    "unsupported core received a task request",
  );
  assert.equal(controller.getState().drafts.s1, "draft remains editable");
}

async function testDisconnectedSocketFencesItsPendingAuthoritativeSnapshot(App) {
  const staleSnapshot = snapshot("s1");
  staleSnapshot.tasks[0].title = "Must never revive";
  const reconnectSnapshot = deferred();
  let snapshotReads = 0;
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: true,
          taskConnectionId: null,
        },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      return snapshotReads === 1
        ? Promise.resolve({ ok: true, result: snapshot("s1") })
        : reconnectSnapshot.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();

  runtime.listeners[0]({
    type: "zode-task-connected",
    protocolVersion: 1,
  });
  await flushAsync();
  runtime.listeners[0]({ type: "zode-task-disconnected" });
  reconnectSnapshot.resolve({ ok: true, result: staleSnapshot });
  await flushAsync();

  assert.equal(controller.getState().connection, "disconnected");
  assert.notEqual(controller.getState().tasks[0].title, "Must never revive");
}

async function testReplacementSocketFencesItsPredecessorWithoutConnectionIds(App) {
  const predecessorSnapshot = snapshot("s1");
  predecessorSnapshot.tasks[0].title = "Superseded socket";
  const replacementSnapshot = snapshot("s1");
  replacementSnapshot.tasks[0].title = "Replacement socket";
  const predecessorRead = deferred();
  const replacementRead = deferred();
  let snapshotReads = 0;
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({
        ok: true,
        status: {
          connected: true,
          taskClientSupported: true,
          taskConnectionId: null,
        },
      });
    }
    if (message.method === "snapshot/read") {
      snapshotReads += 1;
      if (snapshotReads === 1) {
        return Promise.resolve({ ok: true, result: snapshot("s1") });
      }
      return snapshotReads === 2 ? predecessorRead.promise : replacementRead.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();

  runtime.listeners[0]({ type: "zode-task-connected", protocolVersion: 1 });
  await flushAsync();
  runtime.listeners[0]({ type: "zode-task-connected", protocolVersion: 1 });
  await flushAsync();

  assert.equal(snapshotReads, 3, "replacement socket reused its predecessor's snapshot read");
  replacementRead.resolve({ ok: true, result: replacementSnapshot });
  await flushAsync();
  predecessorRead.resolve({ ok: true, result: predecessorSnapshot });
  await flushAsync();

  assert.equal(controller.getState().connection, "connected");
  assert.equal(controller.getState().tasks[0].title, "Replacement socket");
}

async function waitForTaskMethod(runtime, method) {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (runtime.calls.some((call) => call.method === method)) {
      return;
    }
    await flushAsync();
  }
  assert.fail(`timed out waiting for ${method}`);
}

function attachmentGateResponse(stage, params) {
  if (stage === "attachment/begin") {
    return {
      ok: true,
      result: {
        uploadId: "gated-upload",
        name: params.name,
        mediaType: params.mediaType,
        size: params.size,
      },
    };
  }
  if (stage === "attachment/chunk") {
    return {
      ok: true,
      result: { uploadId: params.uploadId, nextSequence: params.sequence + 1 },
    };
  }
  return {
    ok: true,
    result: {
      attachmentId: "gated-attachment",
      name: "gated.rs",
      mediaType: "text/plain",
      size: 2,
    },
  };
}

async function createAttachmentGateScenario(App, stage) {
  const gate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.type === "zode-status") {
      return Promise.resolve({ ok: true, status: { connected: true, canReconnect: false } });
    }
    if (message.method === "snapshot/read") {
      return Promise.resolve({ ok: true, result: snapshot("s1") });
    }
    if (message.method === stage) {
      return gate.promise;
    }
    if (message.method === "attachment/begin") {
      return Promise.resolve(attachmentGateResponse("attachment/begin", message.params));
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve(attachmentGateResponse("attachment/chunk", message.params));
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve(attachmentGateResponse("attachment/finish", message.params));
    }
    if (message.method === "attachment/cancel") {
      return Promise.resolve({
        ok: true,
        result: { uploadId: message.params.uploadId, cancelled: true },
      });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const controller = App.createController({ runtime, storage: makeStorage(), render: () => {} });
  await controller.start();
  await controller.setDraft("preserve gated draft");
  const adding = controller.addFiles([
    fakeFile("gated.rs", "application/octet-stream", 2),
  ]);
  await waitForTaskMethod(runtime, stage);
  const call = runtime.calls.find((item) => item.method === stage);
  return { adding, call, controller, gate, runtime };
}

async function testConnectionInvalidationCannotReviveGatedUploads(App) {
  for (const stage of ["attachment/begin", "attachment/chunk", "attachment/finish"]) {
    const scenario = await createAttachmentGateScenario(App, stage);
    scenario.runtime.listeners[0]({ type: "zode-task-disconnected" });
    scenario.gate.resolve(attachmentGateResponse(stage, scenario.call.params));
    await scenario.adding;

    const attachment = scenario.controller.getAttachments("s1")[0];
    assert.equal(attachment.status, "error", `${stage} revived a disconnected upload`);
    assert.equal(attachment.attachmentId, null);
    assert.match(attachment.error, /connection|re-attach/i);
    assert.equal(scenario.controller.currentDraft(), "preserve gated draft");
    assert.equal(
      scenario.runtime.calls.some((call) => call.method === "attachment/cancel"),
      false,
      "disconnect cleanup is owned by the server connection",
    );
    if (stage === "attachment/begin") {
      assert.equal(
        scenario.runtime.calls.some((call) => call.method === "attachment/chunk"),
        false,
      );
    }
    if (stage !== "attachment/finish") {
      assert.equal(
        scenario.runtime.calls.some((call) => call.method === "attachment/finish"),
        false,
      );
    }
  }
}

async function testRemovalCancelsUploadsAcrossProtocolGates(App) {
  for (const stage of ["attachment/begin", "attachment/chunk", "attachment/finish"]) {
    const scenario = await createAttachmentGateScenario(App, stage);
    const localId = scenario.controller.getAttachments("s1")[0].localId;
    const removing = scenario.controller.removeAttachment(localId, "s1");
    scenario.gate.resolve(attachmentGateResponse(stage, scenario.call.params));
    await Promise.all([scenario.adding, removing]);

    assert.deepEqual(plain(scenario.controller.getAttachments("s1")), []);
    const cancels = scenario.runtime.calls.filter(
      (call) => call.method === "attachment/cancel",
    );
    assert.equal(cancels.length, 1, `${stage} did not cancel exactly once`);
    assert.deepEqual(cancels[0].params, { uploadId: "gated-upload" });
    if (stage === "attachment/begin") {
      assert.equal(
        scenario.runtime.calls.some((call) => call.method === "attachment/chunk"),
        false,
      );
    }
    if (stage !== "attachment/finish") {
      assert.equal(
        scenario.runtime.calls.some((call) => call.method === "attachment/finish"),
        false,
      );
    }
  }
}

async function testAttachmentUploadsAreSerialAndPickerRendersRemovableChips(App) {
  const firstFinish = deferred();
  let uploadNumber = 0;
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      uploadNumber += 1;
      return Promise.resolve({ ok: true, result: { uploadId: `serial-${uploadNumber}` } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: message.params.sequence + 1 } });
    }
    if (message.method === "attachment/finish" && message.params.uploadId === "serial-1") {
      return firstFinish.promise;
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "serial-attachment-2" } });
    }
    if (message.method === "attachment/cancel") {
      return Promise.resolve({ ok: true, result: {} });
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = connectedAttachmentController(App, runtime, view);

  view.elements.attach.emit("click");
  assert.equal(view.elements.attachmentInput.clickCount, 1);
  view.elements.attachmentInput.files = [
    fakeFile("first.js", "text/javascript", 2),
    fakeFile("second.js", "text/javascript", 2),
  ];
  view.elements.attachmentInput.emit("change");
  await flushAsync();
  assert.deepEqual(
    runtime.calls.filter((call) => call.method === "attachment/begin").map((call) => call.params.name),
    ["first.js"],
  );

  firstFinish.resolve({ ok: true, result: { attachmentId: "serial-attachment-1" } });
  await flushAsync();
  await flushAsync();
  assert.deepEqual(
    runtime.calls.filter((call) => call.method === "attachment/begin").map((call) => call.params.name),
    ["first.js", "second.js"],
  );
  assert.equal(view.elements.attach.disabled, false);
  assert.equal(view.elements.attachmentList.hidden, false);
  assert.match(view.elements.attachmentList.textContent, /first\.js/);
  assert.match(view.elements.attachmentList.textContent, /second\.js/);
  const removeButtons = descendants(view.elements.attachmentList).filter(
    (node) => node.tagName === "BUTTON" && /^Remove /.test(node.attributes["aria-label"] || ""),
  );
  assert.equal(removeButtons.length, 2);
  removeButtons[0].emit("click");
  await flushAsync();
  assert.equal(controller.getAttachments().some((item) => item.name === "first.js"), false);
  assert.equal(
    runtime.calls.some(
      (call) => call.method === "attachment/cancel" && call.params.uploadId === "serial-1",
    ),
    true,
  );
}

async function testSubmittedAttachmentsCannotBeCancelledInFlight(App) {
  const turnGate = deferred();
  const runtime = makeRuntime((message) => {
    if (message.method === "attachment/begin") {
      return Promise.resolve({ ok: true, result: { uploadId: "committed-upload" } });
    }
    if (message.method === "attachment/chunk") {
      return Promise.resolve({ ok: true, result: { nextSequence: 1 } });
    }
    if (message.method === "attachment/finish") {
      return Promise.resolve({ ok: true, result: { attachmentId: "committed-attachment" } });
    }
    if (message.method === "turn/start") {
      return turnGate.promise;
    }
    return Promise.resolve({ ok: true, result: {} });
  });
  const document = new FakeUiDocument();
  const view = App.createDomView(document);
  const controller = connectedAttachmentController(App, runtime, view);
  await controller.addFiles([fakeFile("committed.ts", "text/plain", 2)]);
  await controller.setDraft("send committed attachment");
  const submitting = controller.submit();
  await flushAsync();

  const localId = controller.getAttachments()[0].localId;
  const remove = descendants(view.elements.attachmentList).find(
    (node) => node.tagName === "BUTTON" && node.attributes["aria-label"] === "Remove committed.ts",
  );
  assert.equal(remove.disabled, true);
  await assert.rejects(() => controller.removeAttachment(localId), /busy|submitting/i);
  await assert.rejects(
    () => controller.addFiles([fakeFile("late.ts", "text/plain", 1)]),
    /busy|submitting/i,
  );
  assert.equal(
    runtime.calls.some((call) => call.method === "attachment/cancel"),
    false,
  );
  assert.equal(
    runtime.calls.filter((call) => call.method === "attachment/begin").length,
    1,
  );

  turnGate.resolve({ ok: true, result: { turnId: "committed-turn" } });
  await submitting;
  assert.deepEqual(plain(controller.getAttachments()), []);
  await assert.rejects(
    () => controller.addFiles([fakeFile("while-running.ts", "text/plain", 1)]),
    /busy|running/i,
  );
}

(async () => {
  const { State, App } = loadApis();
  testInitialStateHasNoSharedMutableReferences(State);
  testSnapshotAuthoritativelyReplacesServerState(State);
  testReducerCoversCoreTaskLifecycleWithoutMutation(State);
  testStrictTurnFencingAndConnectionErrors(State);
  testApprovalResolutionIsStrictlyScopedAndValidated(State);
  testRunningSnapshotTurnBindingAndMissingTurnIds(State);
  testTurnStartedRejectsTerminalAndConflictingIds(State);
  testStoppingSnapshotBindsFirstTurnAndStartedDoesNotUndoStopping(State);
  testTerminatedTurnHistoryIsBoundedAndSurvivesSnapshots(State);
  testFeedbackSelectorsFilterBackgroundTasks(State);
  testConcurrentTaskFeedbackIsIsolatedAndSnapshotClearsIt(State);
  testPrimaryActionTreatsStoppingAsStop(State);
  testMarkdownBlocksAndSafeLinks(State);
  await testCommonMarkAndGfmRenderer();
  testHtmlAndSourceContracts();
  testMarkdownDomRendererUsesTextNodesOnly(State, App);
  await testControllerStartupReconnectsHydratesAndRegistersOnce(App);
  await testControllerDispatchesEventsAndCoreRequestPaths(App);
  await testRuntimeSnapshotEventUsesParamsAsSnapshot(App);
  await testConnectionStaysCheckingUntilSnapshotSucceeds(App);
  await testInitialSnapshotFailureDisconnectsAndKeepsActionsLocked(App);
  await testSnapshotCorrectionPersistsCurrentTask(App);
  await testControllerFailurePathsAreActionableAndChromeCallsAreSafe(App);
  await testDisconnectedAndCheckingGuardsIncludingEnter(App);
  await testTurnStartSingleFlightAndDraftOwnership(App);
  await testTurnStartResponseCannotReviveTerminalOrReplaceNewerTurn(App, State);
  await testTurnStartResponseDoesNotLoseItsFirstDelta(App, State);
  await testAuthoritativeSnapshotInvalidatesPendingTurnStart(App, State);
  await testDisconnectInvalidatesPendingTurnStart(App, State);
  await testApprovalResponsesAreSingleFlightAndIndependent(App);
  await testServerStaleApprovalIsAnExpectedFence(App);
  await testApprovalResponseIsFencedByDisconnectAndTerminal(App, State);
  await testApprovalResponseIsFencedByAuthoritativeSnapshot(App, State);
  await testApprovalResponseIsFencedByReplacementSocket(App, State);
  await testServerApprovalResolvedSettlesPendingResponse(App);
  await testApprovalCardsExposeIndependentDecisionControls(App);
  await testDomShowsSendingAndUsesLightDraftRender(App);
  await testMessageDeltaUpdatesOnlyItsDomNodeAndEmptyStateIsTaskScoped(App);
  await testSelectionSingleFlightFailureAndSuccess(App);
  await testCreateModelAndAccessSingleFlight(App, State);
  await testNavigationAndMutationFlightsAreShared(App, State);
  await testInterruptIsSingleFlightAndRollsBackOnce(App, State);
  await testAcknowledgedInterruptCannotBeSentAgainWhileStopping(App, State);
  await testInterruptRequiresAnAuthoritativeTurnId(App, State);
  await testStoppingWithoutTurnIdIsAnIdempotentNoOp(App, State);
  await testTaskSettingsRejectWhileTurnStartIsPending(App);
  await testInterruptFailureRollsBackRunning(App, State);
  await testSlowHydrationAndNewTaskDraftMigration(App, State);
  await testRetryConnectionIsSingleFlightAndPreservesDraft(App);
  await testAuthenticatedEventAndReconnectStatusShareSnapshotRead(App);
  await testReconnectStatusAndLaterAuthenticatedEventShareSnapshotRead(App);
  testMessageDeltasAreCoalescedWithAnimationFrames(App);
  testUnavailableMoreActionStaysHiddenAndDisabled(App);
  await testSwitchingTaskLocksMutationsButPreservesDraft(App);
  await testSwitchingTaskDomDisablesSendAndEnterButKeepsComposerEditable(App);
  await testCreateAndSelectCanReturnSwitchingTasks(App);
  await testDomFiltersBackgroundFeedback(App);
  await testAttachmentMimeNormalizationIsAllowlistDriven(App);
  await testAttachmentUploadChunksSequentiallyAndSubmitsIds(App);
  await testUnsupportedAttachmentsKeepDraftAndReadyIdsForRetry(App);
  await testAttachmentChunkBoundaries(App);
  await testAttachmentFailureKeepsDraftAndOtherReadyChips(App);
  await testAttachmentRemovalCancelsFinishedUpload(App);
  await testAttachmentClientValidationAndBounds(App);
  await testAttachmentFailureRendersBeforeCleanupFinishes(App);
  await testAttachmentDisconnectInvalidatesFinishedIds(App);
  await testConnectedSocketRefreshesAuthoritativeStateAndPreservesLocalWork(App);
  await testUnsupportedTaskProtocolShowsCompatibilityMessage(App);
  await testDisconnectedSocketFencesItsPendingAuthoritativeSnapshot(App);
  await testReplacementSocketFencesItsPredecessorWithoutConnectionIds(App);
  await testConnectionInvalidationCannotReviveGatedUploads(App);
  await testRemovalCancelsUploadsAcrossProtocolGates(App);
  await testAttachmentUploadsAreSerialAndPickerRendersRemovableChips(App);
  await testSubmittedAttachmentsCannotBeCancelledInFlight(App);
  console.log("sidepanel tests passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
