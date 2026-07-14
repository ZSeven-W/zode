(() => {
  "use strict";

  const State = globalThis.ZodePanelState;
  const STORAGE_DRAFTS = "zodePanelDrafts";
  const STORAGE_CURRENT_TASK = "zodePanelCurrentTask";
  const STORAGE_COLLAPSED_TOOLS = "zodePanelCollapsedTools";
  const STORAGE_KEYS = [STORAGE_DRAFTS, STORAGE_CURRENT_TASK, STORAGE_COLLAPSED_TOOLS];
  const DISCONNECTED_GUIDANCE = "启动 zode 并运行 /browser pair 以连接任务客户端。";
  const UNSUPPORTED_TASK_CLIENT_MESSAGE =
    "Current zode version does not support the task client";
  const ATTACHMENT_CHUNK_BYTES = 256 * 1024;
  const ATTACHMENT_IMAGE_LIMIT = 5 * 1024 * 1024;
  const ATTACHMENT_TEXT_LIMIT = 1024 * 1024;
  const ATTACHMENT_TOTAL_LIMIT = 20 * 1024 * 1024;
  const ATTACHMENT_MAX_FILES = 8;
  const PENDING_TURN_DELTA_LIMIT = 64;
  const APPROVAL_DECISIONS = new Set(["allow", "allowAlways", "deny"]);
  const IMAGE_MIMES = new Set([
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
  ]);
  const IMAGE_MIME_BY_EXTENSION = new Map([
    [".png", "image/png"],
    [".jpg", "image/jpeg"],
    [".jpeg", "image/jpeg"],
    [".gif", "image/gif"],
    [".webp", "image/webp"],
  ]);
  const TEXT_EXTENSIONS = new Set([
    ".rs",
    ".js",
    ".ts",
    ".tsx",
    ".jsx",
    ".json",
    ".md",
    ".toml",
    ".yaml",
    ".yml",
    ".css",
    ".html",
    ".sh",
    ".py",
    ".go",
    ".java",
    ".kt",
  ]);
  const FORBIDDEN_EXTENSIONS = new Set([
    ".pdf",
    ".doc",
    ".docx",
    ".docm",
    ".xls",
    ".xlsx",
    ".xlsm",
    ".ppt",
    ".pptx",
    ".pptm",
    ".odt",
    ".ods",
    ".odp",
    ".rtf",
    ".zip",
    ".7z",
    ".rar",
    ".tar",
    ".gz",
    ".tgz",
    ".bz2",
    ".xz",
    ".exe",
    ".msi",
    ".dll",
    ".com",
    ".scr",
    ".bat",
    ".cmd",
    ".ps1",
    ".jar",
  ]);
  const STRICT_MIME = /^[a-z0-9][a-z0-9!#$&^_.+-]*\/[a-z0-9][a-z0-9!#$&^_.+-]*$/;

  function invoke(operation) {
    return Promise.resolve().then(operation);
  }

  function messageOf(error) {
    return String((error && error.message) || error || "unknown error");
  }

  function actionableError(prefix, error) {
    return `${prefix}: ${messageOf(error)}。请启动 zode 并运行 /browser pair，然后重试。`;
  }

  function turnStartError(error) {
    if (error && error.code === "attachments_not_supported") {
      return "无法发送任务：当前 zode 版本不支持文件附件。请升级 zode，或移除附件后仅发送文本任务。";
    }
    return actionableError("无法发送任务", error);
  }

  function isUnsupportedTaskClient(error) {
    return Boolean(
      error &&
        (error.code === "unsupported_task_client" ||
          messageOf(error) === UNSUPPORTED_TASK_CLIENT_MESSAGE),
    );
  }

  function disconnectedMessage(error) {
    return isUnsupportedTaskClient(error)
      ? UNSUPPORTED_TASK_CLIENT_MESSAGE
      : `${DISCONNECTED_GUIDANCE} (${messageOf(error)})`;
  }

  function attachmentExtension(name) {
    const normalized = String(name || "").toLowerCase();
    const dot = normalized.lastIndexOf(".");
    return dot >= 0 ? normalized.slice(dot) : "";
  }

  function normalizeAttachment(file) {
    const extension = attachmentExtension(file && file.name);
    if (FORBIDDEN_EXTENSIONS.has(extension)) {
      return null;
    }
    if (IMAGE_MIME_BY_EXTENSION.has(extension)) {
      return { kind: "image", mime: IMAGE_MIME_BY_EXTENSION.get(extension) };
    }
    if (TEXT_EXTENSIONS.has(extension)) {
      return { kind: "text", mime: "text/plain" };
    }

    const rawMime = String((file && file.type) || "");
    if (rawMime !== rawMime.trim()) {
      return null;
    }
    const mime = rawMime.toLowerCase();
    if (!STRICT_MIME.test(mime)) {
      return null;
    }
    if (IMAGE_MIMES.has(mime)) {
      return { kind: "image", mime };
    }
    if (mime.startsWith("text/") && mime !== "text/rtf") {
      return { kind: "text", mime: "text/plain" };
    }
    return null;
  }

  function encodeBase64(bytes) {
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
    }
    if (typeof globalThis.btoa !== "function") {
      throw new Error("base64 encoding is unavailable");
    }
    return globalThis.btoa(binary);
  }

  function appendInlineNodes(document, parent, parts) {
    for (const part of parts || []) {
      if (part.kind === "code") {
        const code = document.createElement("code");
        code.textContent = part.text;
        parent.appendChild(code);
        continue;
      }
      if (part.kind === "link" && State.safeUrl(part.href)) {
        const link = document.createElement("a");
        link.textContent = part.text;
        link.setAttribute("href", State.safeUrl(part.href));
        link.setAttribute("target", "_blank");
        link.setAttribute("rel", "noopener noreferrer");
        parent.appendChild(link);
        continue;
      }
      if (typeof document.createTextNode === "function") {
        parent.appendChild(document.createTextNode(part.text || ""));
      } else {
        const span = document.createElement("span");
        span.textContent = part.text || "";
        parent.appendChild(span);
      }
    }
  }

  function renderMarkdown(document, parent, markdown) {
    if (!parent || !State || typeof State.markdownBlocks !== "function") {
      return;
    }
    parent.replaceChildren();
    for (const block of State.markdownBlocks(markdown)) {
      if (block.kind === "code") {
        const pre = document.createElement("pre");
        const code = document.createElement("code");
        if (block.language) {
          code.setAttribute("data-language", block.language);
        }
        code.textContent = block.text;
        pre.appendChild(code);
        parent.appendChild(pre);
        continue;
      }
      if (block.kind === "list") {
        const list = document.createElement("ul");
        for (const item of block.items) {
          const listItem = document.createElement("li");
          appendInlineNodes(document, listItem, item.inlines);
          list.appendChild(listItem);
        }
        parent.appendChild(list);
        continue;
      }
      const tagName = block.kind === "heading" ? `h${block.level}` : "p";
      const element = document.createElement(tagName);
      appendInlineNodes(document, element, block.inlines);
      parent.appendChild(element);
    }
  }

  function createController(options = {}) {
    if (!State || typeof State.initialState !== "function") {
      throw new Error("zode panel state API is unavailable");
    }

    const runtime = options.runtime || null;
    const storage = options.storage || null;
    const render =
      typeof options.render === "function"
        ? options.render
        : options.view && typeof options.view.render === "function"
          ? (state, controller, action) => options.view.render(state, controller, action)
          : () => {};
    let state = State.initialState();
    let startPromise = null;
    let listenerRegistered = false;
    let persistenceQueue = Promise.resolve();
    let persistenceHydrated = false;
    const preHydrationWrites = new Set();
    let navigationFlight = null;
    const mutationByTask = new Map();
    const approvalResponseById = new Map();
    let retryPromise = null;
    let snapshotRefreshFlight = null;
    let taskConnectionId = null;
    let taskConnectionEpoch = 0;
    let taskAuthorityGeneration = 0;
    const attachmentsByTask = new Map();
    const attachmentNotices = new Map();
    let attachmentSequence = 0;
    let uploadQueue = Promise.resolve();
    let controller = null;

    function notify(action = { type: "controller/render" }) {
      try {
        render(state, controller, action);
      } catch (error) {
        console.debug("zode side panel render failed", error);
      }
    }

    function dispatch(action) {
      if (action && (action.type === "snapshot" || action.type === "disconnected")) {
        invalidatePendingTurnStarts(action.type);
        invalidateApprovalResponses(action.type);
      }
      if (action && isTerminalTurnAction(action.type)) {
        const params = action.params || {};
        invalidateApprovalResponsesForTurn(params.taskId, params.turnId, action.type);
      }
      if (action && action.type === "approval/resolved") {
        resolveApprovalResponseFromEvent(action.params || {});
      }
      if (action && action.type === "disconnected") {
        invalidateAttachmentEntries();
      }
      state = State.reduce(state, action);
      notify(action);
      return state;
    }

    function getState() {
      return state;
    }

    function currentDraft() {
      return State.currentDraft(state);
    }

    function primaryAction() {
      return State.primaryAction(state, state.currentTaskId);
    }

    function mutationKind(taskId = state.currentTaskId) {
      const flight = taskId && mutationByTask.get(taskId);
      return flight ? flight.kind : null;
    }

    function isMutating(taskId) {
      if (arguments.length === 0) {
        return mutationByTask.size > 0;
      }
      return Boolean(taskId && mutationByTask.has(taskId));
    }

    function isSubmitting(taskId = state.currentTaskId) {
      return mutationKind(taskId) === "turn/start";
    }

    function isInterrupting(taskId = state.currentTaskId) {
      return mutationKind(taskId) === "turn/interrupt";
    }

    function isNavigating() {
      return navigationFlight !== null;
    }

    function isSelecting() {
      return Boolean(navigationFlight && navigationFlight.kind === "select");
    }

    function isCreating() {
      return Boolean(navigationFlight && navigationFlight.kind === "create");
    }

    function isSettingModel(taskId = state.currentTaskId) {
      return mutationKind(taskId) === "model/set";
    }

    function isSettingAccess(taskId = state.currentTaskId) {
      return mutationKind(taskId) === "permission/set";
    }

    function isRetrying() {
      return retryPromise !== null;
    }

    function isRespondingApproval(approvalId) {
      return approvalResponseById.has(String(approvalId || ""));
    }

    function assertConnected() {
      if (state.connection !== "connected") {
        throw new Error(DISCONNECTED_GUIDANCE);
      }
    }

    function runtimeMessage(message) {
      if (!runtime || typeof runtime.sendMessage !== "function") {
        return Promise.reject(new Error("Chrome runtime messaging is unavailable"));
      }
      return invoke(() => runtime.sendMessage(message));
    }

    async function taskRequest(method, params = {}, allowChecking = false) {
      if (allowChecking) {
        if (state.connection !== "checking" && state.connection !== "connected") {
          throw new Error(DISCONNECTED_GUIDANCE);
        }
      } else {
        assertConnected();
      }
      const response = await runtimeMessage({
        type: "zode-task-request",
        method,
        params,
      });
      if (!response || !response.ok) {
        const error = new Error((response && response.error) || `${method} failed`);
        if (response && response.code != null) {
          error.code = response.code;
        }
        throw error;
      }
      return response.result;
    }

    function request(method, params = {}) {
      return taskRequest(method, params, false);
    }

    function attachmentKey(taskId = state.currentTaskId) {
      return taskId || State.NEW_TASK_DRAFT || "__new__";
    }

    function attachmentEntries(taskId = state.currentTaskId, create = false) {
      const key = attachmentKey(taskId);
      if (create && !attachmentsByTask.has(key)) {
        attachmentsByTask.set(key, []);
      }
      return attachmentsByTask.get(key) || [];
    }

    function publicAttachment(entry) {
      return {
        localId: entry.localId,
        name: entry.name,
        mime: entry.mime,
        size: entry.size,
        status: entry.status,
        uploadId: entry.uploadId || null,
        attachmentId: entry.attachmentId || null,
        error: entry.error || null,
      };
    }

    function getAttachments(taskId = state.currentTaskId) {
      return attachmentEntries(taskId).map(publicAttachment);
    }

    function getAttachmentNotice(taskId = state.currentTaskId) {
      return attachmentNotices.get(attachmentKey(taskId)) || "";
    }

    function notifyAttachments(taskId, type = "attachment/updated") {
      notify({ type, taskId: taskId || null });
    }

    function setAttachmentNotice(taskId, message) {
      const key = attachmentKey(taskId);
      if (message) {
        attachmentNotices.set(key, message);
      } else {
        attachmentNotices.delete(key);
      }
      notifyAttachments(taskId, "attachment/notice");
    }

    function isUploadingAttachments(taskId = state.currentTaskId) {
      return attachmentEntries(taskId).some(
        (entry) => entry.status === "queued" || entry.status === "uploading",
      );
    }

    function readyAttachmentEntries(taskId = state.currentTaskId) {
      return attachmentEntries(taskId).filter(
        (entry) => entry.status === "ready" && entry.attachmentId,
      );
    }

    function attachmentBytes(taskId = state.currentTaskId) {
      return attachmentEntries(taskId).reduce(
        (total, entry) =>
          entry.status === "error" || entry.removed ? total : total + entry.size,
        0,
      );
    }

    function attachmentValidation(file, taskId) {
      const name = String((file && file.name) || "attachment");
      const size = Number(file && file.size);
      if (
        !Number.isSafeInteger(size) ||
        size < 0 ||
        !file ||
        typeof file.arrayBuffer !== "function"
      ) {
        return { error: "Unsupported file object.", kind: null, mime: "" };
      }
      const normalized = normalizeAttachment(file);
      if (!normalized) {
        return { error: `Unsupported file type: ${name}`, kind: null, mime: "" };
      }
      const { kind, mime } = normalized;
      if (kind === "image" && size > ATTACHMENT_IMAGE_LIMIT) {
        return { error: "Image attachments must be 5 MiB or smaller.", kind, mime };
      }
      if (kind === "text" && size > ATTACHMENT_TEXT_LIMIT) {
        return { error: "Text attachments must be 1 MiB or smaller.", kind, mime };
      }
      if (attachmentBytes(taskId) + size > ATTACHMENT_TOTAL_LIMIT) {
        return { error: "Attachments may total at most 20 MiB per turn.", kind, mime };
      }
      return { error: null, kind, mime };
    }

    function invalidateAttachmentEntries() {
      for (const entries of attachmentsByTask.values()) {
        for (const entry of entries) {
          if (
            entry.status === "queued" ||
            entry.status === "uploading" ||
            entry.status === "ready"
          ) {
            entry.invalidated = true;
            entry.file = null;
            entry.status = "error";
            entry.attachmentId = null;
            entry.error = "Connection changed; re-attach this file before sending.";
          }
        }
      }
    }

    function hasAttachmentEntries() {
      return [...attachmentsByTask.values()].some((entries) => entries.length > 0);
    }

    function normalizeTaskConnectionId(connectionId) {
      return typeof connectionId === "string" && connectionId.length > 0 ? connectionId : null;
    }

    function adoptTaskConnection(connectionId, invalidateChanged, authenticatedEvent = false) {
      const next = normalizeTaskConnectionId(connectionId);
      const identityChanged = next !== taskConnectionId;
      if (invalidateChanged && identityChanged && hasAttachmentEntries()) {
        invalidateAttachmentEntries();
      }
      if (identityChanged || (authenticatedEvent && next == null)) {
        invalidatePendingTurnStarts("connection changed");
        invalidateApprovalResponses("connection changed");
        taskConnectionEpoch += 1;
      }
      taskConnectionId = next;
      return { connectionId: next, connectionEpoch: taskConnectionEpoch };
    }

    function invalidateTaskConnection() {
      invalidatePendingTurnStarts("connection invalidated");
      invalidateApprovalResponses("connection invalidated");
      taskConnectionEpoch += 1;
      taskConnectionId = null;
    }

    function taskConnectionIsCurrent(connectionId, connectionEpoch) {
      return (
        taskConnectionEpoch === connectionEpoch &&
        taskConnectionId === normalizeTaskConnectionId(connectionId)
      );
    }

    async function cancelAttachmentUpload(entry) {
      if (!entry.uploadId || entry.cancelSent) {
        return;
      }
      entry.cancelSent = true;
      try {
        await request("attachment/cancel", { uploadId: entry.uploadId });
      } catch (error) {
        console.debug("zode attachment cleanup failed", error);
      }
    }

    async function uploadAttachment(entry) {
      if (entry.removed || entry.invalidated) {
        return null;
      }
      entry.status = "uploading";
      notifyAttachments(entry.taskId);
      try {
        const buffer = await invoke(() => entry.file.arrayBuffer());
        const bytes = new Uint8Array(buffer);
        if (bytes.byteLength !== entry.size) {
          throw new Error("Selected file size changed before upload");
        }
        if (entry.removed || entry.invalidated) {
          return null;
        }

        const begun = await request("attachment/begin", {
          taskId: entry.taskId,
          name: entry.name,
          mediaType: entry.mime,
          size: entry.size,
        });
        entry.uploadId = typeof begun === "string" ? begun : begun && begun.uploadId;
        if (!entry.uploadId) {
          throw new Error("attachment/begin did not return an upload ID");
        }
        if (entry.removed || entry.invalidated) {
          await cancelAttachmentUpload(entry);
          return null;
        }

        let sequence = 0;
        for (let offset = 0; offset < bytes.length; offset += ATTACHMENT_CHUNK_BYTES) {
          const chunk = bytes.subarray(
            offset,
            Math.min(offset + ATTACHMENT_CHUNK_BYTES, bytes.length),
          );
          await request("attachment/chunk", {
            uploadId: entry.uploadId,
            sequence,
            data: encodeBase64(chunk),
          });
          sequence += 1;
          if (entry.removed || entry.invalidated) {
            await cancelAttachmentUpload(entry);
            return null;
          }
        }

        const finished = await request("attachment/finish", { uploadId: entry.uploadId });
        const attachmentId =
          typeof finished === "string" ? finished : finished && finished.attachmentId;
        if (!attachmentId) {
          throw new Error("attachment/finish did not return an attachment ID");
        }
        if (entry.removed || entry.invalidated) {
          await cancelAttachmentUpload(entry);
          return null;
        }
        entry.attachmentId = attachmentId;
        entry.file = null;
        entry.status = "ready";
        entry.error = null;
        notifyAttachments(entry.taskId);
        return entry.attachmentId;
      } catch (error) {
        if (!entry.removed) {
          entry.file = null;
          entry.status = "error";
          entry.error = messageOf(error);
          notifyAttachments(entry.taskId);
        }
        await cancelAttachmentUpload(entry);
        return null;
      }
    }

    function enqueueAttachment(entry) {
      const operation = uploadQueue.catch(() => {}).then(() => uploadAttachment(entry));
      uploadQueue = operation.catch(() => {});
      entry.uploadPromise = operation;
      return operation;
    }

    function addFiles(files) {
      try {
        assertConnected();
      } catch (error) {
        return Promise.reject(error);
      }
      const taskId = state.currentTaskId;
      if (!taskId) {
        return Promise.reject(new Error("select a task before attaching files"));
      }
      if (mutationByTask.has(taskId) || State.primaryAction(state, taskId) !== "send") {
        return Promise.reject(new Error("task is busy; wait before attaching files"));
      }
      const entries = attachmentEntries(taskId, true);
      attachmentNotices.delete(attachmentKey(taskId));
      const uploads = [];
      for (const file of Array.from(files || [])) {
        if (entries.length >= ATTACHMENT_MAX_FILES) {
          attachmentNotices.set(
            attachmentKey(taskId),
            `You can attach at most ${ATTACHMENT_MAX_FILES} files per turn.`,
          );
          break;
        }
        const validation = attachmentValidation(file, taskId);
        const entry = {
          localId: `file-${++attachmentSequence}`,
          taskId,
          name: String((file && file.name) || "attachment"),
          mime: validation.mime,
          size: Number(file && file.size) || 0,
          status: validation.error ? "error" : "queued",
          uploadId: null,
          attachmentId: null,
          error: validation.error,
          file: validation.error ? null : file,
          removed: false,
          invalidated: false,
          cancelSent: false,
          uploadPromise: null,
        };
        entries.push(entry);
        if (!validation.error) {
          uploads.push(enqueueAttachment(entry));
        }
      }
      notifyAttachments(taskId);
      return Promise.all(uploads).then(() => getAttachments(taskId));
    }

    async function removeAttachment(localId, taskId = state.currentTaskId) {
      if (taskId && mutationByTask.has(taskId)) {
        throw new Error("task is busy submitting attachments");
      }
      const entries = attachmentEntries(taskId);
      const index = entries.findIndex((entry) => entry.localId === localId);
      if (index < 0) {
        return false;
      }
      const entry = entries[index];
      entry.removed = true;
      entries.splice(index, 1);
      if (entries.length < ATTACHMENT_MAX_FILES) {
        attachmentNotices.delete(attachmentKey(taskId));
      }
      notifyAttachments(taskId);
      if (entry.uploadId) {
        await cancelAttachmentUpload(entry);
      } else if (entry.uploadPromise) {
        await entry.uploadPromise;
      }
      return true;
    }

    function consumeAttachments(taskId, attachmentIds) {
      if (!attachmentIds.length) {
        return;
      }
      const consumed = new Set(attachmentIds);
      const remaining = attachmentEntries(taskId).filter(
        (entry) => !consumed.has(entry.attachmentId),
      );
      attachmentsByTask.set(attachmentKey(taskId), remaining);
      attachmentNotices.delete(attachmentKey(taskId));
      notifyAttachments(taskId);
    }

    function writePersistence(values) {
      if (!storage || typeof storage.set !== "function") {
        return Promise.resolve();
      }
      persistenceQueue = persistenceQueue
        .catch(() => {})
        .then(() => invoke(() => storage.set(values)))
        .catch((error) => {
          console.debug("zode side panel persistence failed", error);
        });
      return persistenceQueue;
    }

    function queuePersistence(values) {
      if (!persistenceHydrated) {
        for (const key of Object.keys(values)) {
          preHydrationWrites.add(key);
        }
        return Promise.resolve();
      }
      return writePersistence(values);
    }

    async function hydratePersistence() {
      persistenceHydrated = false;
      if (!storage || typeof storage.get !== "function") {
        dispatch({ type: "persistence/hydrated", persisted: {} });
        const shouldWriteBack = preHydrationWrites.size > 0;
        persistenceHydrated = true;
        preHydrationWrites.clear();
        if (shouldWriteBack) {
          await writePersistence({
            [STORAGE_DRAFTS]: state.drafts,
            [STORAGE_CURRENT_TASK]: state.currentTaskId,
            [STORAGE_COLLAPSED_TOOLS]: state.collapsedToolIds,
          });
        }
        return;
      }
      let stored = {};
      try {
        stored = (await invoke(() => storage.get(STORAGE_KEYS))) || {};
      } catch (error) {
        console.debug("zode side panel persistence unavailable", error);
      }
      dispatch({
        type: "persistence/hydrated",
        persisted: {
          drafts:
            stored[STORAGE_DRAFTS] && typeof stored[STORAGE_DRAFTS] === "object"
              ? stored[STORAGE_DRAFTS]
              : {},
          currentTaskId: stored[STORAGE_CURRENT_TASK] || null,
          collapsedToolIds: Array.isArray(stored[STORAGE_COLLAPSED_TOOLS])
            ? stored[STORAGE_COLLAPSED_TOOLS]
            : [],
        },
      });
      const shouldWriteBack = preHydrationWrites.size > 0;
      preHydrationWrites.clear();
      persistenceHydrated = true;
      if (shouldWriteBack) {
        await writePersistence({
          [STORAGE_DRAFTS]: state.drafts,
          [STORAGE_CURRENT_TASK]: state.currentTaskId,
          [STORAGE_COLLAPSED_TOOLS]: state.collapsedToolIds,
        });
      }
    }

    function applySnapshot(snapshot) {
      const previousTaskId = state.currentTaskId;
      dispatch({ type: "snapshot", snapshot: snapshot || {} });
      if (state.currentTaskId === previousTaskId) {
        return Promise.resolve();
      }
      return queuePersistence({
        [STORAGE_CURRENT_TASK]: state.currentTaskId,
        [STORAGE_DRAFTS]: state.drafts,
        [STORAGE_COLLAPSED_TOOLS]: state.collapsedToolIds,
      });
    }

    function applyTaskResult(result, fallbackAction) {
      if (result && result.snapshot && typeof result.snapshot === "object") {
        void applySnapshot(result.snapshot);
        return;
      }
      if (result && Array.isArray(result.tasks)) {
        void applySnapshot(result);
        return;
      }
      if (fallbackAction) {
        dispatch(fallbackAction(result));
      }
    }

    async function refreshSnapshot(taskId, options = {}) {
      const connectionId = Object.prototype.hasOwnProperty.call(options, "connectionId")
        ? normalizeTaskConnectionId(options.connectionId)
        : taskConnectionId;
      const connectionEpoch =
        typeof options.connectionEpoch === "number"
          ? options.connectionEpoch
          : taskConnectionEpoch;
      try {
        const result = await taskRequest(
          "snapshot/read",
          taskId ? { taskId } : {},
          options.allowChecking === true,
        );
        if (!taskConnectionIsCurrent(connectionId, connectionEpoch)) {
          return result;
        }
        await applySnapshot(result || {});
        if (taskConnectionIsCurrent(connectionId, connectionEpoch)) {
          dispatch({ type: "connected" });
        }
        return result;
      } catch (error) {
        if (
          taskConnectionIsCurrent(connectionId, connectionEpoch) &&
          !isUnsupportedTaskClient(error)
        ) {
          dispatch({
            type: "error/set",
            message: actionableError("无法加载任务快照", error),
          });
        }
        throw error;
      }
    }

    function refreshAuthoritativeSnapshot(taskId, options = {}) {
      const connectionId = Object.prototype.hasOwnProperty.call(options, "connectionId")
        ? normalizeTaskConnectionId(options.connectionId)
        : taskConnectionId;
      const connectionEpoch =
        typeof options.connectionEpoch === "number"
          ? options.connectionEpoch
          : taskConnectionEpoch;
      if (
        snapshotRefreshFlight &&
        snapshotRefreshFlight.connectionId === connectionId &&
        snapshotRefreshFlight.connectionEpoch === connectionEpoch
      ) {
        return snapshotRefreshFlight.promise;
      }
      let tracked;
      tracked = refreshSnapshot(taskId, {
        ...options,
        connectionId,
        connectionEpoch,
      }).finally(() => {
        if (snapshotRefreshFlight && snapshotRefreshFlight.promise === tracked) {
          snapshotRefreshFlight = null;
        }
      });
      snapshotRefreshFlight = { connectionId, connectionEpoch, promise: tracked };
      return tracked;
    }

    function queuePendingTurnDelta(action) {
      const params = action.params || {};
      const taskId = params.taskId;
      const turnId = params.turnId;
      const flight = taskId && mutationByTask.get(taskId);
      const task = taskId && State.taskById(state, taskId);
      if (
        action.type !== "message/delta" ||
        !taskId ||
        !turnId ||
        !flight ||
        flight.kind !== "turn/start" ||
        flight.invalidated ||
        (task && (task.activeTurnId || task.status === "running" || task.status === "stopping"))
      ) {
        return false;
      }
      if (flight.pendingTurnDeltas.length < PENDING_TURN_DELTA_LIMIT) {
        flight.pendingTurnDeltas.push(action);
        return true;
      }
      const buffered = flight.pendingTurnDeltas.findLast(
        (candidate) =>
          candidate.params.taskId === taskId &&
          candidate.params.turnId === turnId &&
          candidate.params.messageId === params.messageId,
      );
      if (buffered) {
        buffered.params = {
          ...buffered.params,
          delta: String(buffered.params.delta || "") + String(params.delta || ""),
        };
      }
      return true;
    }

    function flushPendingTurnDeltas(taskId, turnId) {
      const flight = taskId && mutationByTask.get(taskId);
      if (!flight || flight.kind !== "turn/start" || flight.invalidated || !turnId) {
        return;
      }
      const pending = flight.pendingTurnDeltas;
      flight.pendingTurnDeltas = [];
      for (const action of pending) {
        if (action.params && action.params.turnId === turnId) {
          dispatch(action);
        }
      }
    }

    function handleRuntimeMessage(message) {
      if (!message || typeof message !== "object") {
        return false;
      }
      if (message.type === "zode-task-event" && typeof message.event === "string") {
        if (message.event === "snapshot") {
          void applySnapshot(message.params || {});
        } else {
          const action = { type: message.event, params: message.params || {} };
          if (!queuePendingTurnDelta(action)) {
            dispatch(action);
            if (action.type === "turn/started") {
              flushPendingTurnDeltas(action.params.taskId, action.params.turnId);
            }
          }
        }
        return false;
      }
      if (message.type === "zode-task-disconnected") {
        invalidateTaskConnection();
        dispatch({ type: "disconnected", message: DISCONNECTED_GUIDANCE });
        return false;
      }
      if (message.type === "zode-task-unsupported") {
        invalidateTaskConnection();
        dispatch({ type: "disconnected", message: UNSUPPORTED_TASK_CLIENT_MESSAGE });
        return false;
      }
      if (message.type === "zode-task-connected") {
        if (message.protocolVersion !== 1) {
          invalidateTaskConnection();
          dispatch({ type: "disconnected", message: UNSUPPORTED_TASK_CLIENT_MESSAGE });
          return false;
        }
        const connection = adoptTaskConnection(message.connectionId, true, true);
        dispatch({ type: "connection/checking", message: "正在同步任务…" });
        void refreshAuthoritativeSnapshot(state.currentTaskId, {
          allowChecking: true,
          ...connection,
        }).catch((error) => {
          if (taskConnectionIsCurrent(connection.connectionId, connection.connectionEpoch)) {
            dispatch({ type: "disconnected", message: disconnectedMessage(error) });
          }
        });
        return false;
      }
      return false;
    }

    function registerRuntimeListener() {
      if (listenerRegistered) {
        return;
      }
      if (!runtime || !runtime.onMessage || typeof runtime.onMessage.addListener !== "function") {
        return;
      }
      try {
        runtime.onMessage.addListener(handleRuntimeMessage);
        listenerRegistered = true;
      } catch (error) {
        console.debug("zode side panel event listener unavailable", error);
      }
    }

    async function startFlow() {
      registerRuntimeListener();
      dispatch({ type: "connection/checking" });
      await hydratePersistence();

      try {
        const statusResponse = await runtimeMessage({ type: "zode-status" });
        if (!statusResponse || !statusResponse.ok) {
          throw new Error((statusResponse && statusResponse.error) || "status unavailable");
        }
        let connectionStatus = statusResponse.status || {};
        let connected = Boolean(connectionStatus.connected);
        if (!connected && connectionStatus.canReconnect) {
          const reconnect = await runtimeMessage({ type: "zode-reconnect" });
          if (!reconnect || !reconnect.ok) {
            throw new Error((reconnect && reconnect.error) || "reconnect failed");
          }
          connectionStatus = reconnect.status || {};
          connected = Boolean(connectionStatus.connected);
        }

        if (!connected) {
          invalidateTaskConnection();
          dispatch({ type: "disconnected", message: DISCONNECTED_GUIDANCE });
          return;
        }

        if (connectionStatus.taskClientSupported === false) {
          invalidateTaskConnection();
          dispatch({ type: "disconnected", message: UNSUPPORTED_TASK_CLIENT_MESSAGE });
          return;
        }
        adoptTaskConnection(connectionStatus.taskConnectionId, true);

        await refreshAuthoritativeSnapshot(state.currentTaskId, { allowChecking: true });
      } catch (error) {
        invalidateTaskConnection();
        dispatch({
          type: "disconnected",
          message: disconnectedMessage(error),
        });
      }
    }

    function start() {
      if (!startPromise) {
        startPromise = startFlow().catch((error) => {
          invalidateTaskConnection();
          dispatch({
            type: "disconnected",
            message: disconnectedMessage(error),
          });
        });
      }
      return startPromise;
    }

    function retryConnection() {
      if (retryPromise) {
        return retryPromise;
      }
      const operation = invoke(async () => {
        try {
          const statusResponse = await runtimeMessage({ type: "zode-status" });
          if (!statusResponse || !statusResponse.ok) {
            throw new Error((statusResponse && statusResponse.error) || "status unavailable");
          }
          let connectionStatus = statusResponse.status || {};
          let connected = Boolean(connectionStatus.connected);
          if (!connected && connectionStatus.canReconnect) {
            const reconnect = await runtimeMessage({ type: "zode-reconnect" });
            if (!reconnect || !reconnect.ok) {
              throw new Error((reconnect && reconnect.error) || "reconnect failed");
            }
            connectionStatus = reconnect.status || {};
            connected = Boolean(connectionStatus.connected);
          }
          if (!connected) {
            invalidateTaskConnection();
            dispatch({ type: "disconnected", message: DISCONNECTED_GUIDANCE });
            return false;
          }
          if (connectionStatus.taskClientSupported === false) {
            invalidateTaskConnection();
            dispatch({ type: "disconnected", message: UNSUPPORTED_TASK_CLIENT_MESSAGE });
            return false;
          }
          adoptTaskConnection(connectionStatus.taskConnectionId, true);
          await refreshAuthoritativeSnapshot(state.currentTaskId, { allowChecking: true });
          return true;
        } catch (error) {
          invalidateTaskConnection();
          dispatch({
            type: "connection/error",
            params: { message: actionableError("重新连接失败", error) },
          });
          dispatch({
            type: "disconnected",
            message: disconnectedMessage(error),
          });
          return false;
        }
      });
      let tracked;
      tracked = operation.finally(() => {
        if (retryPromise === tracked) {
          retryPromise = null;
          notify({ type: "controller/retrying" });
        }
      });
      retryPromise = tracked;
      dispatch({ type: "connection/checking", message: "正在重新连接…" });
      notify({ type: "controller/retrying" });
      return tracked;
    }

    function setDraftForTask(taskId, value) {
      dispatch({ type: "draft/set", taskId, value });
      return queuePersistence({ [STORAGE_DRAFTS]: state.drafts });
    }

    function setDraft(value) {
      return setDraftForTask(state.currentTaskId, value);
    }

    function toggleTool(toolId) {
      dispatch({ type: "tool/toggle", toolId });
      return queuePersistence({ [STORAGE_COLLAPSED_TOOLS]: state.collapsedToolIds });
    }

    function beginNavigation(kind, operation) {
      if (navigationFlight) {
        return navigationFlight.promise;
      }
      const flight = { kind, promise: null };
      navigationFlight = flight;
      flight.promise = invoke(operation).finally(() => {
        if (navigationFlight === flight) {
          navigationFlight = null;
          notify({ type: "controller/navigation" });
        }
      });
      notify({ type: "controller/navigation" });
      return flight.promise;
    }

    function beginTaskMutation(taskId, kind, operation) {
      const active = mutationByTask.get(taskId);
      if (active) {
        if (active.kind === kind) {
          return active.promise;
        }
        return Promise.reject(new Error("task is busy"));
      }
      let resolveInvalidation;
      const invalidation = new Promise((resolve) => {
        resolveInvalidation = resolve;
      });
      const flight = {
        taskId,
        kind,
        promise: null,
        pendingTurnDeltas: [],
        authorityGeneration: taskAuthorityGeneration,
        invalidated: false,
        invalidation,
        resolveInvalidation,
      };
      mutationByTask.set(taskId, flight);
      flight.promise = invoke(() => operation(flight)).finally(() => {
        if (mutationByTask.get(taskId) === flight) {
          mutationByTask.delete(taskId);
          notify({ type: "controller/mutation", taskId, kind });
        }
      });
      notify({ type: "controller/mutation", taskId, kind });
      return flight.promise;
    }

    function invalidatePendingTurnStarts(reason) {
      taskAuthorityGeneration += 1;
      for (const flight of mutationByTask.values()) {
        if (flight.kind !== "turn/start" || flight.invalidated) {
          continue;
        }
        flight.invalidated = true;
        flight.pendingTurnDeltas = [];
        flight.resolveInvalidation(reason);
      }
    }

    function staleTurnStartError(reason) {
      const error = new Error(`turn/start became stale after ${reason || "state changed"}`);
      error.code = "stale_turn_start";
      return error;
    }

    function turnStartFlightIsCurrent(flight) {
      return Boolean(
        flight &&
          !flight.invalidated &&
          flight.authorityGeneration === taskAuthorityGeneration &&
          mutationByTask.get(flight.taskId) === flight,
      );
    }

    async function requestTurnStart(flight, params) {
      if (!turnStartFlightIsCurrent(flight)) {
        throw staleTurnStartError("state changed");
      }
      const result = await Promise.race([
        request("turn/start", params),
        flight.invalidation.then((reason) => {
          throw staleTurnStartError(reason);
        }),
      ]);
      if (!turnStartFlightIsCurrent(flight)) {
        throw staleTurnStartError("state changed");
      }
      return result;
    }

    function isTerminalTurnAction(type) {
      return (
        type === "turn/completed" ||
        type === "turn/interrupted" ||
        type === "turn/stopped" ||
        type === "task/terminal" ||
        type === "terminal" ||
        type === "turn/error" ||
        type === "turn/failed" ||
        type === "task/error" ||
        type === "error"
      );
    }

    function invalidateApprovalFlight(flight, reason) {
      if (!flight || flight.invalidated) {
        return;
      }
      flight.invalidated = true;
      flight.resolveInvalidation(reason);
    }

    function invalidateApprovalResponses(reason) {
      for (const flight of approvalResponseById.values()) {
        invalidateApprovalFlight(flight, reason);
      }
    }

    function invalidateApprovalResponsesForTurn(taskId, turnId, reason) {
      if (!taskId || !turnId) {
        return;
      }
      for (const flight of approvalResponseById.values()) {
        if (flight.taskId === taskId && flight.turnId === turnId) {
          invalidateApprovalFlight(flight, reason);
        }
      }
    }

    function resolveApprovalResponseFromEvent(params) {
      const approvalId = String(params.approvalId || params.id || "");
      const flight = approvalResponseById.get(approvalId);
      if (
        !flight ||
        flight.invalidated ||
        flight.eventResolved ||
        params.taskId !== flight.taskId ||
        params.turnId !== flight.turnId ||
        params.decision !== flight.decision
      ) {
        return;
      }
      flight.eventResolved = true;
      flight.resolveEvent({});
    }

    function staleApprovalResponseError(reason) {
      const error = new Error(`approval response became stale after ${reason || "state changed"}`);
      error.code = "stale_approval_response";
      return error;
    }

    function approvalResponseIsCurrent(flight, allowResolved = false) {
      if (!flight) {
        return false;
      }
      const approval = state.approvals.find((item) => item.id === flight.approvalId);
      const task = State.taskById(state, flight.taskId);
      const approvalStatusMatches =
        approval &&
        (approval.status === "pending" ||
          (allowResolved &&
            approval.status === "resolved" &&
            approval.decision === flight.decision));
      return Boolean(
        !flight.invalidated &&
          flight.authorityGeneration === taskAuthorityGeneration &&
          approvalResponseById.get(flight.approvalId) === flight &&
          state.connection === "connected" &&
          task &&
          (task.status === "running" || task.status === "stopping") &&
          task.activeTurnId === flight.turnId &&
          approval &&
          approval.taskId === flight.taskId &&
          approval.turnId === flight.turnId &&
          approvalStatusMatches,
      );
    }

    async function requestApprovalResponse(flight) {
      if (flight.eventResolved && approvalResponseIsCurrent(flight, true)) {
        return {};
      }
      if (!approvalResponseIsCurrent(flight)) {
        throw staleApprovalResponseError("state changed");
      }
      const result = await Promise.race([
        request("approval/respond", {
          taskId: flight.taskId,
          turnId: flight.turnId,
          approvalId: flight.approvalId,
          decision: flight.decision,
        }),
        flight.invalidation.then((reason) => {
          throw staleApprovalResponseError(reason);
        }),
        flight.resolution,
      ]);
      if (!approvalResponseIsCurrent(flight, true)) {
        throw staleApprovalResponseError("state changed");
      }
      return result;
    }

    function respondApproval(approvalId, decision) {
      const id = String(approvalId || "");
      try {
        if (!APPROVAL_DECISIONS.has(decision)) {
          const error = new Error("invalid approval decision");
          error.code = "invalid_approval_decision";
          throw error;
        }
        assertConnected();
        const active = approvalResponseById.get(id);
        if (active) {
          return active.promise;
        }
        const approval = state.approvals.find((item) => item.id === id);
        const task = approval && State.taskById(state, approval.taskId);
        if (
          !approval ||
          approval.status !== "pending" ||
          !approval.taskId ||
          !approval.turnId ||
          !task ||
          (task.status !== "running" && task.status !== "stopping") ||
          task.activeTurnId !== approval.turnId
        ) {
          const error = new Error("approval is no longer pending for the active turn");
          error.code = "stale_approval_response";
          throw error;
        }

        let resolveInvalidation;
        const invalidation = new Promise((resolve) => {
          resolveInvalidation = resolve;
        });
        let resolveEvent;
        const resolution = new Promise((resolve) => {
          resolveEvent = resolve;
        });
        const flight = {
          approvalId: id,
          taskId: approval.taskId,
          turnId: approval.turnId,
          decision,
          promise: null,
          authorityGeneration: taskAuthorityGeneration,
          invalidated: false,
          invalidation,
          resolveInvalidation,
          eventResolved: false,
          resolution,
          resolveEvent,
        };
        approvalResponseById.set(id, flight);
        flight.promise = invoke(async () => {
          try {
            const result = await requestApprovalResponse(flight);
            const current = state.approvals.find((item) => item.id === id);
            if (current && current.status === "pending") {
              dispatch({
                type: "approval/resolved",
                params: {
                  taskId: flight.taskId,
                  turnId: flight.turnId,
                  approvalId: flight.approvalId,
                  decision: flight.decision,
                },
              });
            }
            return result;
          } catch (error) {
            if (error && error.code === "stale_approval") {
              error.code = "stale_approval_response";
            }
            if (error && error.code === "stale_approval_response") {
              throw error;
            }
            dispatch({
              type: "error/set",
              taskId: flight.taskId,
              message: actionableError("无法响应审批", error),
            });
            throw error;
          }
        }).finally(() => {
          if (approvalResponseById.get(id) === flight) {
            approvalResponseById.delete(id);
            notify({ type: "controller/approval", approvalId: id });
          }
        });
        notify({ type: "controller/approval", approvalId: id });
        return flight.promise;
      } catch (error) {
        return Promise.reject(error);
      }
    }

    function selectTask(taskId) {
      const errorTaskId = state.currentTaskId;
      try {
        assertConnected();
      } catch (error) {
        return Promise.reject(error);
      }
      return beginNavigation("select", async () => {
        try {
          const result = await request("task/select", { taskId });
          applyTaskResult(result, () => ({ type: "selection/set", taskId }));
          if (state.currentTaskId !== taskId) {
            dispatch({ type: "selection/set", taskId });
          }
          await queuePersistence({ [STORAGE_CURRENT_TASK]: taskId });
          return result;
        } catch (error) {
          dispatch({
            type: "error/set",
            taskId: errorTaskId,
            message: actionableError("无法切换任务", error),
          });
          throw error;
        }
      });
    }

    function createTask() {
      const errorTaskId = state.currentTaskId;
      try {
        assertConnected();
      } catch (error) {
        return Promise.reject(error);
      }
      const migrateNewTaskDraft = state.currentTaskId == null;
      return beginNavigation("create", async () => {
        try {
          const result = await request("task/create", {});
          applyTaskResult(result, (value) => ({
            type: "task/created",
            params: {
              task: value && value.task ? value.task : value,
              current: true,
            },
          }));
          const createdId =
            (result && result.currentTaskId) ||
            (result && result.task && result.task.id) ||
            (result && result.id) ||
            state.currentTaskId;
          if (createdId) {
            dispatch({ type: "selection/set", taskId: createdId });
            if (migrateNewTaskDraft) {
              dispatch({ type: "draft/migrate", from: State.NEW_TASK_DRAFT, to: createdId });
            }
            await queuePersistence({
              [STORAGE_CURRENT_TASK]: createdId,
              [STORAGE_DRAFTS]: state.drafts,
            });
          }
          return result;
        } catch (error) {
          dispatch({
            type: "error/set",
            taskId: errorTaskId,
            message: actionableError("无法创建任务", error),
          });
          throw error;
        }
      });
    }

    function assertTaskEditable(taskId = state.currentTaskId) {
      if (!taskId) {
        throw new Error("select a task first");
      }
      const action = State.primaryAction(state, taskId);
      if (action === "loading") {
        throw new Error("task is switching");
      }
      if (action === "stop") {
        throw new Error("task is busy");
      }
    }

    function setModel(model) {
      const taskId = state.currentTaskId;
      try {
        assertConnected();
        const active = taskId && mutationByTask.get(taskId);
        if (active) {
          if (active.kind === "model/set") {
            return active.promise;
          }
          throw new Error("task is busy");
        }
        assertTaskEditable(taskId);
      } catch (error) {
        dispatch({
          type: "error/set",
          taskId,
          message: actionableError("无法切换模型", error),
        });
        return Promise.reject(error);
      }

      return beginTaskMutation(taskId, "model/set", async () => {
        try {
          const result = await request("model/set", { taskId, model });
          applyTaskResult(result, () => ({
            type: "task/updated",
            params: { task: { id: taskId, model } },
          }));
          return result;
        } catch (error) {
          dispatch({
            type: "error/set",
            taskId,
            message: actionableError("无法切换模型", error),
          });
          throw error;
        }
      });
    }

    function setAccess(access) {
      const taskId = state.currentTaskId;
      try {
        assertConnected();
        const active = taskId && mutationByTask.get(taskId);
        if (active) {
          if (active.kind === "permission/set") {
            return active.promise;
          }
          throw new Error("task is busy");
        }
        assertTaskEditable(taskId);
      } catch (error) {
        dispatch({
          type: "error/set",
          taskId,
          message: actionableError("无法更新权限", error),
        });
        return Promise.reject(error);
      }

      return beginTaskMutation(taskId, "permission/set", async () => {
        try {
          const result = await request("permission/set", { taskId, mode: access });
          applyTaskResult(result, () => ({
            type: "task/updated",
            params: { task: { id: taskId, access } },
          }));
          return result;
        } catch (error) {
          dispatch({
            type: "error/set",
            taskId,
            message: actionableError("无法更新权限", error),
          });
          throw error;
        }
      });
    }

    function submit() {
      try {
        assertConnected();
      } catch (error) {
        return Promise.reject(error);
      }
      const taskId = state.currentTaskId;
      if (!taskId) {
        return Promise.resolve(null);
      }
      const active = mutationByTask.get(taskId);
      if (active) {
        if (active.kind === "turn/start" || active.kind === "turn/interrupt") {
          return active.promise;
        }
        return Promise.reject(new Error("task is busy"));
      }
      const selectedTask = State.taskById(state, taskId);
      if (primaryAction() === "loading") {
        const error = new Error("task is switching");
        dispatch({
          type: "error/set",
          taskId,
          message: "任务正在恢复会话，请等待加载完成后再发送。",
        });
        return Promise.reject(error);
      }
      if (selectedTask && selectedTask.status === "stopping") {
        return Promise.resolve(null);
      }
      if (
        selectedTask &&
        selectedTask.status === "running" &&
        !selectedTask.activeTurnId
      ) {
        const error = new Error("active turn ID is missing");
        dispatch({
          type: "error/set",
          taskId,
          message: "无法停止任务：缺少活动 turn ID。请刷新任务快照后重试。",
        });
        return Promise.reject(error);
      }
      if (primaryAction() === "stop") {
        const turnId = selectedTask && selectedTask.activeTurnId;
        return beginTaskMutation(taskId, "turn/interrupt", async () => {
          dispatch({ type: "turn/stopping", params: { taskId, turnId } });
          try {
            return await request("turn/interrupt", {
              taskId,
              turnId,
            });
          } catch (error) {
            const interruptedTask = State.taskById(state, taskId);
            if (
              interruptedTask &&
              interruptedTask.status === "stopping" &&
              (interruptedTask.activeTurnId || null) === (turnId || null)
            ) {
              dispatch({
                type: "task/updated",
                params: { task: { id: taskId, status: "running", activeTurnId: turnId || null } },
              });
            }
            dispatch({
              type: "error/set",
              taskId,
              message: actionableError("无法停止任务", error),
            });
            throw error;
          }
        });
      }

      const originalDraft = currentDraft();
      const input = originalDraft.trim();
      if (isUploadingAttachments(taskId)) {
        const error = new Error("attachments are still uploading");
        setAttachmentNotice(taskId, "Wait for attachments to finish uploading before sending.");
        return Promise.reject(error);
      }
      const submittedAttachments = readyAttachmentEntries(taskId);
      const attachmentIds = submittedAttachments.map((entry) => entry.attachmentId);
      if (!input && attachmentIds.length === 0) {
        return Promise.resolve(null);
      }

      return beginTaskMutation(taskId, "turn/start", async (flight) => {
        try {
          const params = { taskId, input };
          if (attachmentIds.length) {
            params.attachmentIds = attachmentIds;
          }
          const result = await requestTurnStart(flight, params);
          if (result && result.turnId) {
            dispatch({
              type: "turn/started",
              params: { taskId, turnId: result.turnId },
            });
            flushPendingTurnDeltas(taskId, result.turnId);
          }
          if (state.drafts[taskId] === originalDraft) {
            await setDraftForTask(taskId, "");
          }
          consumeAttachments(taskId, attachmentIds);
          return result;
        } catch (error) {
          if (error && error.code === "stale_turn_start") {
            throw error;
          }
          dispatch({
            type: "error/set",
            taskId,
            message: turnStartError(error),
          });
          throw error;
        }
      });
    }

    controller = {
      start,
      getState,
      dispatch,
      request,
      refreshSnapshot,
      currentDraft,
      primaryAction,
      isNavigating,
      isMutating,
      isSubmitting,
      isInterrupting,
      isSelecting,
      isCreating,
      isSettingModel,
      isSettingAccess,
      isRetrying,
      isRespondingApproval,
      isUploadingAttachments,
      retryConnection,
      setDraft,
      toggleTool,
      selectTask,
      createTask,
      setModel,
      setAccess,
      respondApproval,
      getAttachments,
      getAttachmentNotice,
      addFiles,
      removeAttachment,
      submit,
    };
    notify();
    return controller;
  }

  function createDomView(document) {
    const byId = (id) => document.getElementById(id);
    const elements = {
      connectionStatus: byId("connection-status"),
      connectionBanner: byId("connection-banner"),
      connectionBannerMessage: byId("connection-banner-message"),
      retry: byId("retry-button"),
      workspace: byId("workspace-label"),
      taskMenu: byId("task-menu"),
      newTask: byId("new-task-button"),
      more: byId("more-button"),
      messageStream: byId("message-stream"),
      messageList: byId("message-list"),
      emptyState: byId("empty-state"),
      toolRegion: byId("tool-region"),
      approvalRegion: byId("approval-region"),
      errorRegion: byId("error-region"),
      composerForm: byId("composer-form"),
      composer: byId("composer"),
      attach: byId("attach-button"),
      attachmentInput: byId("attachment-input"),
      attachmentList: byId("attachment-list"),
      attachmentStatus: byId("attachment-status"),
      access: byId("access-select"),
      model: byId("model-select"),
      send: byId("send-button"),
    };
    let bound = false;
    const messageNodes = new Map();
    const requestFrame =
      document.defaultView && typeof document.defaultView.requestAnimationFrame === "function"
        ? document.defaultView.requestAnimationFrame.bind(document.defaultView)
        : null;
    const pendingDeltaActions = new Map();
    let deltaFramePending = false;
    let pendingDeltaState = null;

    function workspaceText(workspace) {
      if (!workspace) {
        return "No workspace";
      }
      if (typeof workspace === "string") {
        return workspace;
      }
      return workspace.name || workspace.path || workspace.cwd || "Workspace";
    }

    function taskLabel(task) {
      return task.title || task.name || task.id || "Untitled task";
    }

    function modelId(model) {
      return typeof model === "string" ? model : model.id || model.value || model.name || "";
    }

    function modelLabel(model) {
      return typeof model === "string" ? model : model.label || model.name || model.id || "Model";
    }

    function renderTaskMenu(state, controller) {
      if (!elements.taskMenu) {
        return;
      }
      elements.taskMenu.replaceChildren();
      if (!state.tasks.length) {
        const option = document.createElement("option");
        option.value = "";
        option.textContent = "No tasks";
        elements.taskMenu.appendChild(option);
      } else {
        for (const task of state.tasks) {
          const option = document.createElement("option");
          option.value = task.id;
          option.textContent = taskLabel(task);
          option.selected = task.id === state.currentTaskId;
          elements.taskMenu.appendChild(option);
        }
      }
      elements.taskMenu.value = state.currentTaskId || "";
      elements.taskMenu.disabled =
        state.connection !== "connected" ||
        !state.tasks.length ||
        (controller && (controller.isNavigating() || controller.isMutating()));
    }

    function messagesForCurrentTask(state) {
      return state.messages.filter(
        (message) => !message.taskId || message.taskId === state.currentTaskId,
      );
    }

    function toolsForCurrentTask(state) {
      return state.tools.filter(
        (tool) => !tool.taskId || tool.taskId === state.currentTaskId,
      );
    }

    function updateEmptyState(state, messages = messagesForCurrentTask(state)) {
      if (elements.emptyState) {
        elements.emptyState.hidden =
          messages.length > 0 || toolsForCurrentTask(state).length > 0;
      }
    }

    function messageKey(message) {
      return `${message.taskId || ""}\u0000${message.id || ""}`;
    }

    function messageText(message) {
      return String(message.text || message.content || "");
    }

    function renderMessageBody(body, message) {
      if (message.role === "assistant") {
        renderMarkdown(document, body, messageText(message));
      } else {
        body.textContent = messageText(message);
      }
    }

    function createMessageNode(message) {
      const article = document.createElement("article");
      article.className = `message message-${message.role || "assistant"}`;
      const role = document.createElement("p");
      role.className = "message-role";
      role.textContent = message.role === "user" ? "You" : "zode";
      const body = document.createElement("div");
      body.className = "message-body markdown-body";
      renderMessageBody(body, message);
      article.append(role, body);
      return {
        article,
        body,
        role: message.role || "assistant",
        text: messageText(message),
      };
    }

    function renderMessages(state) {
      if (!elements.messageList) {
        return;
      }
      elements.messageList.replaceChildren();
      messageNodes.clear();
      const messages = messagesForCurrentTask(state);
      updateEmptyState(state, messages);
      for (const message of messages) {
        const rendered = createMessageNode(message);
        messageNodes.set(messageKey(message), rendered);
        elements.messageList.appendChild(rendered.article);
      }
    }

    function renderMessageDelta(state, action) {
      if (!elements.messageList) {
        return true;
      }
      const params = action.params || {};
      if (!params.taskId || params.taskId !== state.currentTaskId) {
        return true;
      }
      const messageId =
        params.messageId || `${params.taskId}:${params.turnId || "turn"}:assistant`;
      const message = state.messages.find(
        (candidate) =>
          candidate.id === messageId && candidate.taskId === params.taskId,
      );
      if (!message) {
        return true;
      }
      const key = messageKey(message);
      const rendered = messageNodes.get(key);
      const nextText = messageText(message);
      const nextRole = message.role || "assistant";
      if (rendered) {
        if (rendered.text === nextText && rendered.role === nextRole) {
          return true;
        }
        rendered.article.className = `message message-${nextRole}`;
        renderMessageBody(rendered.body, message);
        rendered.text = nextText;
        rendered.role = nextRole;
        return true;
      }

      const messages = messagesForCurrentTask(state);
      if (
        messages.at(-1) !== message ||
        messageNodes.size !== messages.length - 1 ||
        elements.messageList.children.length !== messages.length - 1
      ) {
        return false;
      }
      const created = createMessageNode(message);
      messageNodes.set(key, created);
      elements.messageList.appendChild(created.article);
      updateEmptyState(state, messages);
      return true;
    }

    function deltaActionKey(action) {
      const params = action.params || {};
      const messageId =
        params.messageId || `${params.taskId || ""}:${params.turnId || "turn"}:assistant`;
      return `${params.taskId || ""}\u0000${messageId}`;
    }

    function scheduleMessageDelta(state, action) {
      if (!requestFrame) {
        return false;
      }
      pendingDeltaState = state;
      pendingDeltaActions.set(deltaActionKey(action), action);
      if (deltaFramePending) {
        return true;
      }
      deltaFramePending = true;
      try {
        requestFrame(() => {
          deltaFramePending = false;
          const nextState = pendingDeltaState;
          const actions = [...pendingDeltaActions.values()];
          pendingDeltaState = null;
          pendingDeltaActions.clear();
          if (!nextState || actions.length === 0) {
            return;
          }
          if (actions.some((pendingAction) => !renderMessageDelta(nextState, pendingAction))) {
            renderMessages(nextState);
          }
        });
      } catch (error) {
        deltaFramePending = false;
        pendingDeltaState = null;
        pendingDeltaActions.clear();
        console.debug("zode side panel animation frame unavailable", error);
        return false;
      }
      return true;
    }

    function renderTools(state, controller) {
      if (!elements.toolRegion) {
        return;
      }
      elements.toolRegion.replaceChildren();
      const tools = toolsForCurrentTask(state);
      elements.toolRegion.hidden = tools.length === 0;
      for (const tool of tools) {
        const details = document.createElement("details");
        details.className = `tool-card tool-${tool.status || "completed"}`;
        details.open = !state.collapsedToolIds.includes(tool.id);
        const summary = document.createElement("summary");
        const title = document.createElement("span");
        title.textContent = tool.summary || tool.name || "Tool";
        const status = document.createElement("span");
        status.className = "tool-status";
        status.textContent = tool.status || "completed";
        summary.append(title, status);
        const body = document.createElement("pre");
        body.textContent =
          typeof tool.output === "string"
            ? tool.output
            : tool.output == null
              ? "No detailed output"
              : JSON.stringify(tool.output, null, 2);
        details.append(summary, body);
        details.addEventListener("toggle", () => {
          const collapsed = controller.getState().collapsedToolIds.includes(tool.id);
          if (collapsed === details.open) {
            controller.toggleTool(tool.id);
          }
        });
        elements.toolRegion.appendChild(details);
      }
    }

    function renderApprovals(state, controller) {
      if (!elements.approvalRegion) {
        return;
      }
      elements.approvalRegion.replaceChildren();
      const approvals = state.approvals.filter(
        (approval) =>
          (!approval.taskId || approval.taskId === state.currentTaskId) &&
          approval.status === "pending",
      );
      elements.approvalRegion.hidden = approvals.length === 0;
      for (const approval of approvals) {
        const card = document.createElement("article");
        card.className = "approval-card";
        const title = document.createElement("strong");
        title.textContent = "Approval required";
        const description = document.createElement("p");
        description.textContent = approval.summary || "This tool is waiting for approval.";
        const actions = document.createElement("div");
        actions.className = "approval-actions";
        const responding = controller.isRespondingApproval(approval.id);
        for (const [decision, label] of [
          ["allow", "Allow once"],
          ["allowAlways", "Always allow"],
          ["deny", "Deny"],
        ]) {
          const button = document.createElement("button");
          button.type = "button";
          button.className =
            decision === "deny"
              ? "approval-button approval-deny"
              : "approval-button approval-allow";
          button.dataset.decision = decision;
          button.textContent = label;
          button.disabled = responding || state.connection !== "connected";
          button.addEventListener("click", () => {
            if (!button.disabled) {
              ignoreFailure(() => controller.respondApproval(approval.id, decision));
            }
          });
          actions.appendChild(button);
        }
        card.append(title, description, actions);
        elements.approvalRegion.appendChild(card);
      }
    }

    function renderError(state) {
      if (!elements.errorRegion) {
        return;
      }
      elements.errorRegion.replaceChildren();
      const error = State.errorForCurrent(state);
      const currentTerminal = State.terminalForCurrent(state);
      elements.errorRegion.hidden = !error && !currentTerminal;
      if (error) {
        const card = document.createElement("article");
        card.className = "error-card";
        const title = document.createElement("strong");
        title.textContent = "Task error";
        const text = document.createElement("p");
        text.textContent = error.message || String(error);
        card.append(title, text);
        elements.errorRegion.appendChild(card);
      } else if (currentTerminal) {
        const terminalCard = document.createElement("p");
        terminalCard.className = "terminal-card";
        terminalCard.textContent = `Turn ${currentTerminal.status || "completed"}`;
        elements.errorRegion.appendChild(terminalCard);
      }
    }

    function renderSelectors(state, controller) {
      const task = State.taskById(state, state.currentTaskId);
      const taskId = task && task.id;
      const busy =
        State.primaryAction(state, state.currentTaskId) !== "send" ||
        controller.isMutating(taskId);
      const controlsDisabled =
        state.connection !== "connected" || !task || busy || controller.isNavigating();
      if (elements.model) {
        elements.model.replaceChildren();
        for (const model of state.models) {
          const option = document.createElement("option");
          option.value = modelId(model);
          option.textContent = modelLabel(model);
          option.selected = option.value === (task && task.model);
          elements.model.appendChild(option);
        }
        elements.model.value = (task && task.model) || modelId(state.models[0] || "");
        elements.model.disabled =
          controlsDisabled ||
          state.models.length === 0 ||
          controller.isSettingModel(taskId);
      }
      if (elements.access) {
        elements.access.value = (task && task.access) || "prompt";
        elements.access.disabled = controlsDisabled || controller.isSettingAccess(taskId);
      }
    }

    function attachmentSize(size) {
      if (size < 1024) {
        return `${size} B`;
      }
      if (size < 1024 * 1024) {
        return `${Math.ceil(size / 1024)} KiB`;
      }
      return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
    }

    function renderAttachments(state, controller) {
      if (elements.attachmentList) {
        elements.attachmentList.replaceChildren();
        const attachments = controller.getAttachments(state.currentTaskId);
        elements.attachmentList.hidden = attachments.length === 0;
        for (const attachment of attachments) {
          const chip = document.createElement("article");
          chip.className = "attachment-chip";
          chip.dataset.status = attachment.status;

          const name = document.createElement("span");
          name.className = "attachment-name";
          name.textContent = attachment.name;
          name.setAttribute("title", attachment.name);

          const status = document.createElement("span");
          status.className = "attachment-state";
          status.textContent =
            attachment.status === "error"
              ? attachment.error || "Upload failed"
              : attachment.status === "ready"
                ? `Ready · ${attachmentSize(attachment.size)}`
                : attachment.status === "uploading"
                  ? "Uploading…"
                  : "Queued";

          const remove = document.createElement("button");
          remove.className = "attachment-remove";
          remove.type = "button";
          remove.textContent = "×";
          remove.setAttribute("aria-label", `Remove ${attachment.name}`);
          remove.disabled =
            controller.isNavigating() || controller.isMutating(state.currentTaskId);
          remove.addEventListener("click", () => {
            ignoreFailure(() => controller.removeAttachment(attachment.localId));
          });

          chip.append(name, status, remove);
          elements.attachmentList.appendChild(chip);
        }
      }
      if (elements.attachmentStatus) {
        const notice = controller.getAttachmentNotice(state.currentTaskId);
        elements.attachmentStatus.textContent = notice;
        elements.attachmentStatus.hidden = !notice;
      }
    }

    function renderComposer(state, controller) {
      const action = controller.primaryAction();
      const loading = action === "loading";
      const connected = state.connection === "connected";
      const draft = controller.currentDraft();
      const submitting = controller.isSubmitting(state.currentTaskId);
      const interrupting = controller.isInterrupting(state.currentTaskId);
      const task = State.taskById(state, state.currentTaskId);
      const awaitingStop = interrupting || Boolean(task && task.status === "stopping");
      const attachmentsUploading = controller.isUploadingAttachments(state.currentTaskId);
      const hasReadyAttachment = controller
        .getAttachments(state.currentTaskId)
        .some((attachment) => attachment.status === "ready");
      const interactionLocked =
        controller.isNavigating() || controller.isMutating(state.currentTaskId);
      if (elements.composer && elements.composer.value !== draft) {
        elements.composer.value = draft;
      }
      if (elements.composer) {
        elements.composer.placeholder = connected
          ? "Ask zode to work on this task…"
          : "Draft preserved — connect zode to send";
      }
      if (elements.send) {
        const stopping = action === "stop";
        elements.send.textContent = loading
          ? "Loading"
          : awaitingStop
          ? "Stopping"
          : submitting
            ? "Sending"
            : stopping
              ? "Stop"
              : "Send";
        elements.send.setAttribute(
          "aria-label",
          loading
            ? "Task is loading"
            : awaitingStop
            ? "Stopping task"
            : submitting
              ? "Sending task"
              : stopping
                ? "Stop task"
                : "Send task",
        );
        elements.send.dataset.action = loading
          ? "loading"
          : awaitingStop
          ? "stopping"
          : submitting
            ? "sending"
            : action;
        elements.send.disabled =
          interactionLocked ||
          loading ||
          awaitingStop ||
          !connected ||
          !state.currentTaskId ||
          attachmentsUploading ||
          (!stopping && draft.trim().length === 0 && !hasReadyAttachment);
      }
      if (elements.attach) {
        elements.attach.disabled =
          !connected ||
          !state.currentTaskId ||
          action !== "send" ||
          controller.isNavigating() ||
          controller.isMutating(state.currentTaskId);
      }
    }

    function render(state, controller, action = {}) {
      if (action.type === "message/delta") {
        if (scheduleMessageDelta(state, action) || renderMessageDelta(state, action)) {
          return;
        }
      }
      if (action.type === "draft/set") {
        renderComposer(state, controller);
        return;
      }
      if (action.type === "attachment/updated" || action.type === "attachment/notice") {
        renderAttachments(state, controller);
        renderComposer(state, controller);
        return;
      }
      if (action.type === "controller/approval") {
        renderApprovals(state, controller);
        return;
      }
      if (
        action.type === "controller/navigation" ||
        action.type === "controller/mutation"
      ) {
        if (elements.newTask) {
          elements.newTask.disabled =
            state.connection !== "connected" ||
            controller.isNavigating() ||
            controller.isMutating();
        }
        renderTaskMenu(state, controller);
        renderAttachments(state, controller);
        renderComposer(state, controller);
        renderSelectors(state, controller);
        return;
      }
      if (elements.connectionStatus) {
        elements.connectionStatus.dataset.state = state.connection;
        elements.connectionStatus.textContent = state.connectionMessage;
      }
      if (elements.connectionBanner) {
        elements.connectionBanner.hidden = state.connection === "connected";
      }
      if (elements.connectionBannerMessage) {
        elements.connectionBannerMessage.textContent = state.connectionMessage;
      }
      if (elements.retry) {
        elements.retry.hidden = state.connection !== "disconnected";
        elements.retry.disabled = state.connection !== "disconnected" || controller.isRetrying();
      }
      if (elements.workspace) {
        elements.workspace.textContent = workspaceText(state.workspace);
        elements.workspace.setAttribute(
          "title",
          typeof state.workspace === "object" && state.workspace
            ? state.workspace.path || state.workspace.cwd || workspaceText(state.workspace)
            : workspaceText(state.workspace),
        );
      }
      if (elements.newTask) {
        elements.newTask.disabled =
          state.connection !== "connected" ||
          controller.isNavigating() ||
          controller.isMutating();
      }
      if (elements.more) {
        elements.more.hidden = true;
        elements.more.disabled = true;
      }
      renderTaskMenu(state, controller);
      pendingDeltaState = null;
      pendingDeltaActions.clear();
      renderMessages(state);
      renderTools(state, controller);
      renderApprovals(state, controller);
      renderError(state);
      renderSelectors(state, controller);
      renderAttachments(state, controller);
      renderComposer(state, controller);
    }

    function ignoreFailure(operation) {
      invoke(operation).catch((error) => console.debug("zode side panel action failed", error));
    }

    function bind(controller) {
      if (bound) {
        return;
      }
      bound = true;
      if (elements.taskMenu) {
        elements.taskMenu.addEventListener("change", () => {
          if (elements.taskMenu.value) {
            ignoreFailure(() => controller.selectTask(elements.taskMenu.value));
          }
        });
      }
      if (elements.newTask) {
        elements.newTask.addEventListener("click", () => {
          ignoreFailure(async () => {
            await controller.createTask();
            if (elements.composer) {
              elements.composer.focus();
            }
          });
        });
      }
      if (elements.retry) {
        elements.retry.addEventListener("click", () => {
          ignoreFailure(() => controller.retryConnection());
        });
      }
      if (elements.attach && elements.attachmentInput) {
        elements.attach.addEventListener("click", () => {
          elements.attachmentInput.value = "";
          if (typeof elements.attachmentInput.click === "function") {
            elements.attachmentInput.click();
          }
        });
        elements.attachmentInput.addEventListener("change", () => {
          const files = Array.from(elements.attachmentInput.files || []);
          elements.attachmentInput.value = "";
          if (files.length) {
            ignoreFailure(() => controller.addFiles(files));
          }
        });
      }
      if (elements.composer) {
        elements.composer.addEventListener("input", () => {
          ignoreFailure(() => controller.setDraft(elements.composer.value));
        });
        elements.composer.addEventListener("keydown", (event) => {
          if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
            event.preventDefault();
            if (controller.primaryAction() !== "loading") {
              ignoreFailure(() => controller.submit());
            }
          }
        });
      }
      if (elements.composerForm) {
        elements.composerForm.addEventListener("submit", (event) => {
          event.preventDefault();
          if (controller.primaryAction() !== "loading") {
            ignoreFailure(() => controller.submit());
          }
        });
      }
      if (elements.model) {
        elements.model.addEventListener("change", () => {
          ignoreFailure(() => controller.setModel(elements.model.value));
        });
      }
      if (elements.access) {
        elements.access.addEventListener("change", () => {
          ignoreFailure(() => controller.setAccess(elements.access.value));
        });
      }
    }

    return { elements, render, bind };
  }

  function startLegacyStatus(document, chromeApi) {
    const status = document.querySelector("#connection-status");
    if (!status || !State || typeof State.setConnection !== "function") {
      return;
    }
    const render = () => {
      status.dataset.state = State.connection;
      status.textContent = State.message;
    };
    render();
    Promise.resolve()
      .then(() => chromeApi.runtime.sendMessage({ type: "zode-status" }))
      .then((response) => {
        State.setConnection(
          response && response.ok && response.status && response.status.connected
            ? "connected"
            : "disconnected",
          response && response.ok && response.status && response.status.connected
            ? "已连接到 zode"
            : "尚未连接到 zode",
        );
        render();
      })
      .catch(() => {
        State.setConnection("disconnected", "无法读取连接状态");
        render();
      });
  }

  function autoStart() {
    if (
      typeof document === "undefined" ||
      typeof chrome === "undefined" ||
      !chrome.runtime
    ) {
      return null;
    }
    if (!State || typeof State.initialState !== "function") {
      startLegacyStatus(document, chrome);
      return null;
    }
    if (globalThis.__zodePanelController) {
      return globalThis.__zodePanelController;
    }
    const view = createDomView(document);
    const controller = createController({
      runtime: chrome.runtime,
      storage: chrome.storage && chrome.storage.local,
      view,
    });
    view.bind(controller);
    globalThis.__zodePanelController = controller;
    controller.start();
    return controller;
  }

  globalThis.ZodePanelApp = Object.freeze({
    createController,
    createDomView,
    renderMarkdown,
    autoStart,
    storageKeys: Object.freeze({
      drafts: STORAGE_DRAFTS,
      currentTask: STORAGE_CURRENT_TASK,
      collapsedTools: STORAGE_COLLAPSED_TOOLS,
    }),
  });

  autoStart();
})();
