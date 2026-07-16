(() => {
  "use strict";

  const NEW_TASK_DRAFT = "__new__";
  const APPROVAL_DECISIONS = new Set(["allow", "allowAlways", "deny"]);

  function cloneValue(value) {
    if (Array.isArray(value)) {
      return value.map(cloneValue);
    }
    if (value && typeof value === "object") {
      const copy = {};
      for (const [key, child] of Object.entries(value)) {
        copy[key] = cloneValue(child);
      }
      return copy;
    }
    return value;
  }

  function initialState() {
    return {
      connection: "checking",
      connectionMessage: "正在连接…",
      loaded: false,
      workspace: null,
      tasks: [],
      currentTaskId: null,
      models: [],
      messages: [],
      tools: [],
      approvals: [],
      terminal: null,
      error: null,
      terminalsByTask: {},
      errorsByTask: {},
      drafts: {},
      collapsedToolIds: [],
    };
  }

  function paramsOf(action) {
    return action && action.params && typeof action.params === "object" ? action.params : {};
  }

  function taskIdOf(params) {
    return params.taskId || (params.task && params.task.id) || null;
  }

  function itemOrder(item) {
    const order = Number(item && item.order);
    return Number.isSafeInteger(order) && order >= 0 ? order : null;
  }

  function nextTimelineOrder(state) {
    let highest = -1;
    for (const items of [state.messages, state.tools, state.approvals]) {
      for (const item of items) {
        const order = itemOrder(item);
        if (order != null) {
          highest = Math.max(highest, order);
        }
      }
    }
    return highest + 1;
  }

  function normalizeTimeline(messages, tools, approvals) {
    const collections = [messages, tools, approvals];
    let next = 0;
    for (const items of collections) {
      for (const item of items) {
        const order = itemOrder(item);
        if (order != null) {
          next = Math.max(next, order + 1);
        }
      }
    }
    return collections.map((items) =>
      items.map((item) => {
        const copy = cloneValue(item);
        if (itemOrder(copy) == null) {
          copy.order = next;
          next += 1;
        }
        return copy;
      }),
    );
  }

  function upsertById(items, id, patch, create = true) {
    const index = items.findIndex((item) => item.id === id);
    if (index < 0) {
      return create ? [...items, cloneValue({ id, ...patch })] : items.map(cloneValue);
    }
    return items.map((item, itemIndex) =>
      itemIndex === index ? cloneValue({ ...item, ...patch, id }) : cloneValue(item),
    );
  }

  function updateTask(state, taskId, patch, create = true) {
    if (!taskId) {
      return state.tasks.map(cloneValue);
    }
    return upsertById(state.tasks, taskId, patch, create);
  }

  function boundedTurnIds(...sources) {
    const ids = [];
    for (const source of sources) {
      for (const value of Array.isArray(source) ? source : []) {
        const turnId = String(value || "");
        if (!turnId) {
          continue;
        }
        const existing = ids.indexOf(turnId);
        if (existing >= 0) {
          ids.splice(existing, 1);
        }
        ids.push(turnId);
      }
    }
    return ids.slice(-32);
  }

  function terminatedTurnIds(task, terminalTurnId) {
    return boundedTurnIds(
      task && task.terminatedTurnIds,
      terminalTurnId ? [terminalTurnId] : [],
    );
  }

  function hasTerminatedTurn(task, turnId) {
    return Boolean(
      task &&
      (task.lastTerminalTurnId === turnId ||
        (Array.isArray(task.terminatedTurnIds) && task.terminatedTurnIds.includes(turnId))),
    );
  }

  function resolveTurnEvent(state, params) {
    const taskId = params.taskId;
    const turnId = params.turnId;
    if (!taskId || !turnId) {
      return { accepted: false, taskId: null, turnId: null, bind: false };
    }
    const task = state.tasks.find((candidate) => candidate.id === taskId);
    if (!task || hasTerminatedTurn(task, turnId)) {
      return { accepted: false, taskId, turnId, bind: false };
    }
    if (task.activeTurnId) {
      return {
        accepted: task.activeTurnId === turnId,
        taskId,
        turnId,
        bind: false,
      };
    }
    return {
      accepted: task.status === "running" || task.status === "stopping",
      taskId,
      turnId,
      bind: task.status === "running" || task.status === "stopping",
    };
  }

  function bindTurnEvent(state, scope) {
    if (!scope.bind) {
      return state;
    }
    return {
      ...state,
      tasks: updateTask(state, scope.taskId, { activeTurnId: scope.turnId }, false),
    };
  }

  function resolvePendingApprovals(approvals, taskId, turnId) {
    return approvals.map((approval) =>
      approval.status === "pending" &&
      approval.taskId === taskId &&
      approval.turnId === turnId
        ? cloneValue({ ...approval, status: "resolved", decision: "cancelled" })
        : cloneValue(approval),
    );
  }

  function errorValue(params, fallback) {
    return {
      taskId: params.taskId || null,
      turnId: params.turnId || null,
      code: params.code || null,
      message: String(params.message || params.error || fallback),
    };
  }

  function setTaskFeedback(feedbackByTask, taskId, feedback) {
    return {
      ...cloneValue(feedbackByTask || {}),
      [taskId]: cloneValue(feedback),
    };
  }

  function clearTaskFeedback(feedbackByTask, taskId) {
    const next = cloneValue(feedbackByTask || {});
    delete next[taskId];
    return next;
  }

  function clearLegacyFeedback(feedback, taskId) {
    return feedback && feedback.taskId === taskId ? null : cloneValue(feedback);
  }

  function reduceSnapshot(state, snapshot) {
    const value = snapshot && typeof snapshot === "object" ? snapshot : {};
    const tasks = (Array.isArray(value.tasks) ? value.tasks : []).map((incoming) => {
      const task = cloneValue(incoming);
      const previous = state.tasks.find((candidate) => candidate.id === task.id);
      const history = boundedTurnIds(
        previous && previous.terminatedTurnIds,
        task.terminatedTurnIds,
        previous && previous.lastTerminalTurnId ? [previous.lastTerminalTurnId] : [],
        task.lastTerminalTurnId ? [task.lastTerminalTurnId] : [],
      );
      if (history.length) {
        task.terminatedTurnIds = history;
      }
      if (!task.lastTerminalTurnId && previous && previous.lastTerminalTurnId) {
        task.lastTerminalTurnId = previous.lastTerminalTurnId;
      }
      return task;
    });
    const [messages, tools, approvals] = normalizeTimeline(
      Array.isArray(value.messages) ? value.messages : [],
      Array.isArray(value.tools)
        ? value.tools
        : Array.isArray(value.toolRuns)
          ? value.toolRuns
          : [],
      Array.isArray(value.approvals) ? value.approvals : [],
    );
    return {
      ...state,
      loaded: true,
      workspace: cloneValue(value.workspace == null ? null : value.workspace),
      tasks,
      currentTaskId: value.currentTaskId == null ? null : value.currentTaskId,
      models: cloneValue(Array.isArray(value.models) ? value.models : []),
      messages,
      tools,
      approvals,
      terminal: null,
      error: null,
      terminalsByTask: {},
      errorsByTask: {},
      drafts: cloneValue(state.drafts),
      collapsedToolIds: cloneValue(state.collapsedToolIds),
    };
  }

  function reduce(state, action) {
    const current = state || initialState();
    const event = action || {};
    const params = paramsOf(event);

    if (event.type === "snapshot") {
      return reduceSnapshot(current, event.snapshot);
    }

    if (event.type === "persistence/hydrated") {
      const persisted = event.persisted && typeof event.persisted === "object" ? event.persisted : {};
      const persistedDrafts =
        persisted.drafts && typeof persisted.drafts === "object" ? persisted.drafts : {};
      const restoredTaskId =
        current.currentTaskId == null && persisted.currentTaskId != null
          ? String(persisted.currentTaskId)
          : null;
      const drafts = cloneValue({ ...persistedDrafts, ...current.drafts });
      if (
        restoredTaskId &&
        Object.prototype.hasOwnProperty.call(current.drafts, NEW_TASK_DRAFT)
      ) {
        drafts[restoredTaskId] = String(current.drafts[NEW_TASK_DRAFT] || "");
        delete drafts[NEW_TASK_DRAFT];
      }
      return {
        ...current,
        currentTaskId:
          current.currentTaskId != null || persisted.currentTaskId == null
            ? current.currentTaskId
            : String(persisted.currentTaskId),
        drafts,
        collapsedToolIds: [
          ...new Set([
            ...(Array.isArray(persisted.collapsedToolIds)
              ? persisted.collapsedToolIds.map(String)
              : []),
            ...current.collapsedToolIds.map(String),
          ]),
        ],
      };
    }

    if (event.type === "connection/checking") {
      return {
        ...current,
        connection: "checking",
        connectionMessage: String(event.message || "正在连接…"),
      };
    }

    if (event.type === "connected") {
      return {
        ...current,
        connection: "connected",
        connectionMessage: String(event.message || "已连接到 zode"),
      };
    }

    if (event.type === "disconnected") {
      return {
        ...current,
        connection: "disconnected",
        connectionMessage: String(
          event.message || "启动 zode 并运行 /browser pair 以恢复连接。",
        ),
      };
    }

    if (event.type === "connection/error") {
      return {
        ...current,
        error: errorValue({ ...params, taskId: null, turnId: null }, "连接失败"),
      };
    }

    if (event.type === "error/set") {
      const error = errorValue(event, "操作失败");
      return {
        ...current,
        error,
        errorsByTask: error.taskId
          ? setTaskFeedback(current.errorsByTask, error.taskId, error)
          : cloneValue(current.errorsByTask || {}),
      };
    }

    if (event.type === "error/clear") {
      const taskId = event.taskId || params.taskId || null;
      return {
        ...current,
        error: taskId ? clearLegacyFeedback(current.error, taskId) : null,
        errorsByTask: taskId
          ? clearTaskFeedback(current.errorsByTask, taskId)
          : cloneValue(current.errorsByTask || {}),
      };
    }

    if (event.type === "draft/set") {
      const draftKey = event.taskId || NEW_TASK_DRAFT;
      return {
        ...current,
        drafts: {
          ...cloneValue(current.drafts),
          [draftKey]: String(event.value == null ? "" : event.value),
        },
      };
    }

    if (event.type === "draft/migrate") {
      const from = event.from || NEW_TASK_DRAFT;
      const to = event.to || event.taskId;
      const drafts = cloneValue(current.drafts);
      if (!to || !Object.prototype.hasOwnProperty.call(drafts, from)) {
        return { ...current, drafts };
      }
      drafts[to] = drafts[from];
      delete drafts[from];
      return { ...current, drafts };
    }

    if (event.type === "selection/set" || event.type === "task/selected") {
      const taskId = event.taskId || params.taskId || null;
      return {
        ...current,
        currentTaskId: taskId,
      };
    }

    if (event.type === "tool/toggle") {
      const toolId = String(event.toolId || "");
      const collapsed = new Set(current.collapsedToolIds);
      if (collapsed.has(toolId)) {
        collapsed.delete(toolId);
      } else if (toolId) {
        collapsed.add(toolId);
      }
      return {
        ...current,
        collapsedToolIds: [...collapsed],
      };
    }

    if (event.type === "task/created") {
      const incoming = cloneValue(params.task || params);
      const taskId = incoming.id || params.taskId;
      if (!taskId) {
        return { ...current };
      }
      incoming.id = taskId;
      return {
        ...current,
        tasks: upsertById(current.tasks, taskId, incoming),
        currentTaskId:
          params.current === true || current.currentTaskId == null
            ? taskId
            : current.currentTaskId,
        error: clearLegacyFeedback(current.error, taskId),
        errorsByTask: clearTaskFeedback(current.errorsByTask, taskId),
      };
    }

    if (event.type === "task/updated") {
      const incoming = cloneValue(params.task || params);
      const taskId = incoming.id || params.taskId;
      if (!taskId) {
        return { ...current };
      }
      delete incoming.taskId;
      return {
        ...current,
        tasks: updateTask(current, taskId, incoming),
      };
    }

    if (event.type === "turn/started") {
      const taskId = taskIdOf(params);
      const turnId = params.turnId;
      const task = current.tasks.find((candidate) => candidate.id === taskId);
      if (
        !taskId ||
        !turnId ||
        (task && hasTerminatedTurn(task, turnId)) ||
        (task && task.activeTurnId && task.activeTurnId !== turnId)
      ) {
        return { ...current };
      }
      return {
        ...current,
        tasks: updateTask(current, taskId, {
          status: task && task.status === "stopping" ? "stopping" : "running",
          activeTurnId: turnId,
        }),
        terminal: clearLegacyFeedback(current.terminal, taskId),
        error: clearLegacyFeedback(current.error, taskId),
        terminalsByTask: clearTaskFeedback(current.terminalsByTask, taskId),
        errorsByTask: clearTaskFeedback(current.errorsByTask, taskId),
      };
    }

    if (event.type === "turn/stopping") {
      const taskId = taskIdOf(params) || event.taskId;
      return {
        ...current,
        tasks: updateTask(current, taskId, { status: "stopping" }),
      };
    }

    if (event.type === "message/added") {
      const taskId = taskIdOf(params);
      const turnId = params.turnId || null;
      const role = params.role || "user";
      const text = String(params.text == null ? params.content || "" : params.text);
      const messageId =
        params.messageId || `${taskId || "task"}:${turnId || "turn"}:${role}`;
      const exists = current.messages.some(
        (message) => message.id === messageId && (!taskId || message.taskId === taskId),
      );
      if (!taskId || !text || exists) {
        return { ...current, messages: current.messages.map(cloneValue) };
      }
      return {
        ...current,
        messages: [
          ...current.messages.map(cloneValue),
          {
            id: messageId,
            taskId,
            turnId,
            role,
            text,
            order: nextTimelineOrder(current),
          },
        ],
      };
    }

    if (event.type === "message/delta") {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current, messages: current.messages.map(cloneValue) };
      }
      const scoped = bindTurnEvent(current, scope);
      const taskId = scope.taskId;
      const messageId =
        params.messageId || `${taskId || "task"}:${params.turnId || "turn"}:assistant`;
      const index = scoped.messages.findIndex(
        (message) => message.id === messageId && (!taskId || message.taskId === taskId),
      );
      const delta = String(params.delta == null ? "" : params.delta);
      let messages;
      if (index < 0) {
        messages = [
          ...scoped.messages.map(cloneValue),
          {
            id: messageId,
            taskId,
            turnId: params.turnId || null,
            role: params.role || "assistant",
            text: delta,
            order: nextTimelineOrder(scoped),
          },
        ];
      } else {
        messages = scoped.messages.map((message, messageIndex) =>
          messageIndex === index
            ? {
                ...cloneValue(message),
                text: String(message.text || message.content || "") + delta,
              }
            : cloneValue(message),
        );
      }
      return { ...scoped, messages };
    }

    if (event.type === "tool/started") {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current, tools: current.tools.map(cloneValue) };
      }
      const scoped = bindTurnEvent(current, scope);
      const toolId = params.toolId || params.id;
      if (!toolId) {
        return { ...current };
      }
      const tool = {
        taskId: scope.taskId,
        turnId: params.turnId || null,
        name: params.tool || params.name || "tool",
        summary: params.summary || params.tool || params.name || "Tool",
        status: "running",
        order:
          itemOrder(scoped.tools.find((candidate) => candidate.id === toolId)) ??
          nextTimelineOrder(scoped),
      };
      return {
        ...scoped,
        tools: upsertById(scoped.tools, toolId, tool),
      };
    }

    if (event.type === "tool/completed") {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current, tools: current.tools.map(cloneValue) };
      }
      const scoped = bindTurnEvent(current, scope);
      const toolId = params.toolId || params.id;
      if (!toolId) {
        return { ...current };
      }
      const existing = scoped.tools.find((candidate) => candidate.id === toolId);
      return {
        ...scoped,
        tools: upsertById(scoped.tools, toolId, {
          taskId: scope.taskId,
          turnId: params.turnId || null,
          status: params.failed ? "failed" : "completed",
          failed: Boolean(params.failed),
          output: params.output == null ? null : cloneValue(params.output),
          order: itemOrder(existing) ?? nextTimelineOrder(scoped),
        }),
      };
    }

    if (event.type === "approval/requested") {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current, approvals: current.approvals.map(cloneValue) };
      }
      const scoped = bindTurnEvent(current, scope);
      const approvalId = params.approvalId || params.id;
      if (!approvalId) {
        return { ...current };
      }
      return {
        ...scoped,
        approvals: upsertById(scoped.approvals, approvalId, {
          ...cloneValue(params),
          id: approvalId,
          taskId: scope.taskId,
          status: "pending",
          order:
            itemOrder(scoped.approvals.find((candidate) => candidate.id === approvalId)) ??
            nextTimelineOrder(scoped),
        }),
      };
    }

    if (event.type === "approval/resolved") {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current, approvals: current.approvals.map(cloneValue) };
      }
      const scoped = bindTurnEvent(current, scope);
      const approvalId = params.approvalId || params.id;
      const approval = scoped.approvals.find((item) => item.id === approvalId);
      if (
        !approvalId ||
        !APPROVAL_DECISIONS.has(params.decision) ||
        !approval ||
        approval.taskId !== scope.taskId ||
        approval.turnId !== scope.turnId ||
        (approval.status !== "pending" &&
          !(approval.status === "resolved" && approval.decision === params.decision))
      ) {
        return { ...current };
      }
      return {
        ...scoped,
        approvals: upsertById(scoped.approvals, approvalId, {
          ...cloneValue(params),
          id: approvalId,
          status: "resolved",
        }, false),
      };
    }

    if (
      event.type === "turn/completed" ||
      event.type === "turn/interrupted" ||
      event.type === "turn/stopped" ||
      event.type === "task/terminal" ||
      event.type === "terminal"
    ) {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current };
      }
      const scoped = bindTurnEvent(current, scope);
      const taskId = scope.taskId;
      const terminalStatus =
        params.status ||
        (event.type === "turn/interrupted" || event.type === "turn/stopped"
          ? "interrupted"
          : "completed");
      const terminal = cloneValue({ ...params, taskId, status: terminalStatus });
      return {
        ...scoped,
        tasks: updateTask(scoped, taskId, {
          status: "idle",
          activeTurnId: null,
          lastTerminalTurnId: scope.turnId,
          terminatedTurnIds: terminatedTurnIds(
            scoped.tasks.find((task) => task.id === taskId),
            scope.turnId,
          ),
        }),
        approvals: resolvePendingApprovals(scoped.approvals, taskId, scope.turnId),
        terminal,
        error: clearLegacyFeedback(scoped.error, taskId),
        terminalsByTask: setTaskFeedback(scoped.terminalsByTask, taskId, terminal),
        errorsByTask: clearTaskFeedback(scoped.errorsByTask, taskId),
      };
    }

    if (
      event.type === "turn/error" ||
      event.type === "turn/failed" ||
      event.type === "task/error" ||
      event.type === "error"
    ) {
      const scope = resolveTurnEvent(current, params);
      if (!scope.accepted) {
        return { ...current };
      }
      const scoped = bindTurnEvent(current, scope);
      const taskId = scope.taskId;
      const error = errorValue({ ...params, taskId }, "任务执行失败");
      const terminal = cloneValue({ ...params, taskId, status: "failed" });
      return {
        ...scoped,
        tasks: updateTask(scoped, taskId, {
          status: "error",
          activeTurnId: null,
          lastTerminalTurnId: scope.turnId,
          terminatedTurnIds: terminatedTurnIds(
            scoped.tasks.find((task) => task.id === taskId),
            scope.turnId,
          ),
        }),
        approvals: resolvePendingApprovals(scoped.approvals, taskId, scope.turnId),
        terminal,
        error,
        terminalsByTask: setTaskFeedback(scoped.terminalsByTask, taskId, terminal),
        errorsByTask: setTaskFeedback(scoped.errorsByTask, taskId, error),
      };
    }

    return { ...current };
  }

  function taskById(state, taskId) {
    return state.tasks.find((task) => task.id === taskId) || null;
  }

  function primaryAction(state, taskId) {
    const task = taskById(state, taskId);
    if (task && task.status === "switching") {
      return "loading";
    }
    return task && (task.status === "running" || task.status === "stopping")
      ? "stop"
      : "send";
  }

  function currentDraft(state) {
    const key = state.currentTaskId || NEW_TASK_DRAFT;
    return String(state.drafts[key] || "");
  }

  function feedbackForCurrent(state, feedback) {
    if (!feedback) {
      return null;
    }
    return !feedback.taskId || feedback.taskId === state.currentTaskId ? feedback : null;
  }

  function errorForCurrent(state) {
    if (state.error && !state.error.taskId) {
      return state.error;
    }
    const taskError =
      state.currentTaskId && state.errorsByTask && state.errorsByTask[state.currentTaskId];
    if (taskError) {
      return taskError;
    }
    return feedbackForCurrent(state, state.error);
  }

  function terminalForCurrent(state) {
    const taskTerminal =
      state.currentTaskId && state.terminalsByTask && state.terminalsByTask[state.currentTaskId];
    if (taskTerminal) {
      return taskTerminal;
    }
    return feedbackForCurrent(state, state.terminal);
  }

  function safeUrl(value) {
    if (typeof value !== "string") {
      return null;
    }
    const url = value.trim();
    if (!url || /[\u0000-\u001f\u007f]/.test(url)) {
      return null;
    }
    return /^(?:https?:|mailto:)/i.test(url) ? url : null;
  }

  function inlineParts(text) {
    const source = String(text == null ? "" : text);
    const parts = [];
    const tokenPattern = /(`[^`\n]+`|\[[^\]\n]+\]\([^\s)]+\))/g;
    let cursor = 0;
    for (const match of source.matchAll(tokenPattern)) {
      if (match.index > cursor) {
        parts.push({ kind: "text", text: source.slice(cursor, match.index) });
      }
      const token = match[0];
      if (token.startsWith("`")) {
        parts.push({ kind: "code", text: token.slice(1, -1) });
      } else {
        const link = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
        const href = link && safeUrl(link[2]);
        if (link && href) {
          parts.push({ kind: "link", text: link[1], href });
        } else if (link) {
          parts.push({ kind: "text", text: link[1] });
        } else {
          parts.push({ kind: "text", text: token });
        }
      }
      cursor = match.index + token.length;
    }
    if (cursor < source.length) {
      parts.push({ kind: "text", text: source.slice(cursor) });
    }
    return parts.length ? parts : [{ kind: "text", text: source }];
  }

  function tableCells(line) {
    let source = String(line == null ? "" : line).trim();
    if (source.startsWith("|")) {
      source = source.slice(1);
    }
    if (source.endsWith("|")) {
      source = source.slice(0, -1);
    }
    return source.split("|").map((cell) => cell.trim());
  }

  function isTableDelimiter(line) {
    if (!String(line || "").includes("|")) {
      return false;
    }
    const cells = tableCells(line);
    return cells.length > 0 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
  }

  function isTableStart(line, nextLine) {
    return String(line || "").includes("|") && isTableDelimiter(nextLine);
  }

  function tableCell(cell) {
    const text = String(cell || "").trim();
    return { text, inlines: inlineParts(text) };
  }

  function isBlockStart(line, nextLine) {
    return (
      /^```/.test(line) ||
      /^#{1,6}\s+/.test(line) ||
      /^\s*[-*+]\s+/.test(line) ||
      isTableStart(line, nextLine)
    );
  }

  function markdownBlocks(markdown) {
    const lines = String(markdown == null ? "" : markdown)
      .replace(/\r\n?/g, "\n")
      .split("\n");
    const blocks = [];
    let index = 0;

    while (index < lines.length) {
      const line = lines[index];
      if (!line.trim()) {
        index += 1;
        continue;
      }

      const fence = line.match(/^```\s*([^\s`]*)\s*$/);
      if (fence) {
        const code = [];
        index += 1;
        while (index < lines.length && !/^```\s*$/.test(lines[index])) {
          code.push(lines[index]);
          index += 1;
        }
        if (index < lines.length) {
          index += 1;
        }
        blocks.push({ kind: "code", language: fence[1] || "", text: code.join("\n") });
        continue;
      }

      const heading = line.match(/^(#{1,6})\s+(.+)$/);
      if (heading) {
        const text = heading[2].trim();
        blocks.push({
          kind: "heading",
          level: heading[1].length,
          text,
          inlines: inlineParts(text),
        });
        index += 1;
        continue;
      }

      if (isTableStart(line, lines[index + 1])) {
        const headers = tableCells(line).map(tableCell);
        const rows = [];
        index += 2;
        while (index < lines.length && lines[index].trim() && lines[index].includes("|")) {
          const cells = tableCells(lines[index]);
          const row = headers.map((_, cellIndex) => tableCell(cells[cellIndex] || ""));
          rows.push(row);
          index += 1;
        }
        blocks.push({ kind: "table", headers, rows });
        continue;
      }

      if (/^\s*[-*+]\s+/.test(line)) {
        const items = [];
        while (index < lines.length) {
          const item = lines[index].match(/^\s*[-*+]\s+(.+)$/);
          if (!item) {
            break;
          }
          const text = item[1].trim();
          items.push({ text, inlines: inlineParts(text) });
          index += 1;
        }
        blocks.push({ kind: "list", items });
        continue;
      }

      const paragraph = [line.trim()];
      index += 1;
      while (
        index < lines.length &&
        lines[index].trim() &&
        !isBlockStart(lines[index], lines[index + 1])
      ) {
        paragraph.push(lines[index].trim());
        index += 1;
      }
      const text = paragraph.join(" ");
      blocks.push({ kind: "paragraph", text, inlines: inlineParts(text) });
    }

    return blocks;
  }

  globalThis.ZodePanelState = Object.freeze({
    NEW_TASK_DRAFT,
    initialState,
    reduce,
    taskById,
    primaryAction,
    currentDraft,
    errorForCurrent,
    terminalForCurrent,
    safeUrl,
    markdownBlocks,
  });
})();
