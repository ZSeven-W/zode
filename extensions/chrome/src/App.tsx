import { useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  Brain,
  Check,
  CheckCircle2,
  CircleAlert,
  FileText,
  FolderGit2,
  Image as ImageIcon,
  LoaderCircle,
  Paperclip,
  Plus,
  RotateCcw,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  UserRound,
  Wrench,
  X,
} from "lucide-react";

import { Markdown } from "./components/markdown";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { ScrollArea } from "./components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./components/ui/select";
import { cn } from "./lib/utils";
import type {
  PanelAction,
  PanelApproval,
  PanelAttachment,
  PanelController,
  PanelMessage,
  PanelModel,
  PanelState,
  PanelTask,
  PanelTool,
} from "./types";

const ACCEPTED_ATTACHMENTS =
  "image/png,image/jpeg,image/gif,image/webp,text/*,.rs,.js,.ts,.tsx,.jsx,.json,.md,.toml,.yaml,.yml,.css,.html,.sh,.py,.go,.java,.kt";

function ignoreFailure(operation: () => Promise<unknown>) {
  try {
    void Promise.resolve(operation()).catch((error) =>
      console.debug("zode side panel action failed", error),
    );
  } catch (error) {
    console.debug("zode side panel action failed", error);
  }
}

function taskLabel(task: PanelTask) {
  return task.title || task.name || task.id || "Untitled task";
}

function workspaceText(workspace: PanelState["workspace"]) {
  if (!workspace) return "No workspace";
  if (typeof workspace === "string") return workspace;
  return workspace.name || workspace.path || workspace.cwd || "Workspace";
}

function workspaceTitle(workspace: PanelState["workspace"]) {
  if (!workspace || typeof workspace === "string") return workspaceText(workspace);
  return workspace.path || workspace.cwd || workspaceText(workspace);
}

function modelId(model: PanelModel) {
  return typeof model === "string" ? model : model.id || model.value || model.name || "";
}

function modelLabel(model: PanelModel) {
  return typeof model === "string" ? model : model.label || model.name || model.id || "Model";
}

function attachmentSize(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.ceil(size / 1024)} KiB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
}

function toolOutput(tool: PanelTool) {
  if (typeof tool.output === "string") return tool.output;
  try {
    return JSON.stringify(tool.output, null, 2);
  } catch {
    return String(tool.output);
  }
}

function MessageCard({ message }: { message: PanelMessage }) {
  const isUser = message.role === "user";
  const text = String(message.text || message.content || "");

  return (
    <article className={cn("group flex gap-2.5", isUser && "flex-row-reverse")}>
      <div
        className={cn(
          "mt-0.5 grid size-7 shrink-0 place-items-center rounded-md border",
          isUser
            ? "border-primary/15 bg-primary text-primary-foreground"
            : "border-border bg-card text-primary",
        )}
        aria-hidden="true"
      >
        {isUser ? <UserRound className="size-3.5" /> : <Bot className="size-3.5" />}
      </div>
      <div className={cn("min-w-0 max-w-[calc(100%-2.75rem)] flex-1", isUser && "flex flex-col items-end")}>
        <div className={cn("mb-1.5 flex items-center gap-2 px-1", isUser && "flex-row-reverse")}>
          <span className="text-[10px] font-bold tracking-[0.14em] text-muted-foreground uppercase">
            {isUser ? "You" : "Zode"}
          </span>
        </div>
        <div
          className={cn(
            "min-w-0 rounded-xl border text-[13px] leading-6",
            isUser
              ? "max-w-[92%] border-primary/20 bg-primary/8 px-4 py-3 text-foreground"
              : "w-full border-transparent bg-transparent px-1 py-0.5 text-card-foreground",
          )}
        >
          {isUser ? <p className="whitespace-pre-wrap">{text}</p> : <Markdown value={text} />}
        </div>
      </div>
    </article>
  );
}

function ThinkingCard({ message }: { message: PanelMessage }) {
  const text = String(message.text || message.content || "");
  if (!text.trim()) return null;
  return (
    <div className="flex gap-2.5">
      <div
        className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-md border border-dashed border-border text-muted-foreground"
        aria-hidden="true"
      >
        <Brain className="size-3.5" />
      </div>
      <div className="min-w-0 max-w-[calc(100%-2.75rem)] flex-1">
        <div className="mb-1 px-1 text-[10px] font-bold tracking-[0.14em] text-muted-foreground uppercase">
          思考
        </div>
        <p className="px-1 text-xs leading-5 whitespace-pre-wrap text-muted-foreground italic">
          {text}
        </p>
      </div>
    </div>
  );
}

function toolStatusLabel(tool: PanelTool) {
  return tool.status || "completed";
}

function ToolRow({ tool }: { tool: PanelTool }) {
  const failed = tool.status === "failed" || tool.status === "error";
  const running = tool.status === "running";
  const icon = failed ? (
    <CircleAlert className="size-3.5 text-destructive" />
  ) : running ? (
    <LoaderCircle className="size-3.5 animate-spin text-warning" />
  ) : (
    <Check className="size-3.5 text-success" />
  );
  const row = (
    <>
      <span className="grid size-5 shrink-0 place-items-center" aria-hidden="true">
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate text-xs font-medium">
        {tool.summary || tool.name || tool.tool || "Tool activity"}
      </span>
      <span
        className={cn(
          "shrink-0 text-[10px]",
          failed ? "text-destructive" : running ? "text-warning" : "text-muted-foreground",
        )}
      >
        {toolStatusLabel(tool)}
      </span>
    </>
  );
  if (tool.output == null) {
    return <div className="flex items-center gap-2.5 px-3.5 py-2">{row}</div>;
  }
  return (
    <details className="group/tool">
      <summary className="flex cursor-pointer list-none items-center gap-2.5 px-3.5 py-2 outline-none hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring/50 [&::-webkit-details-marker]:hidden">
        {row}
      </summary>
      <div className="px-3.5 pb-2.5">
        <pre className="overflow-x-auto rounded-lg border border-border/70 bg-background/70 p-3 font-mono text-[11px] leading-5 whitespace-pre-wrap text-muted-foreground">
          {toolOutput(tool)}
        </pre>
      </div>
    </details>
  );
}

/// One compact card for a consecutive run of tool calls: one row per tool,
/// in timeline order. Rows expand only when the runtime forwarded output
/// (tool payloads are sanitized away by default).
function ToolGroupCard({ tools }: { tools: PanelTool[] }) {
  return (
    <div className="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
      {tools.map((tool) => (
        <ToolRow key={tool.id} tool={tool} />
      ))}
    </div>
  );
}

function ApprovalCard({
  approval,
  controller,
  connected,
}: {
  approval: PanelApproval;
  controller: PanelController;
  connected: boolean;
}) {
  const responding = controller.isRespondingApproval(approval.id);

  return (
    <article className="rounded-xl border border-warning/25 bg-warning/5 p-4">
      <div className="flex items-start gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-warning/10 text-warning">
          <ShieldCheck className="size-4.5" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-2">
            <strong className="text-sm">Approval required</strong>
            <Badge variant="warning">Waiting</Badge>
          </div>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">
            {approval.summary || "This tool is waiting for approval."}
          </p>
        </div>
      </div>
      <div className="mt-3 flex flex-wrap justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={responding || !connected}
          onClick={() => ignoreFailure(() => controller.respondApproval(approval.id, "deny"))}
        >
          Deny
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={responding || !connected}
          onClick={() =>
            ignoreFailure(() => controller.respondApproval(approval.id, "allowAlways"))
          }
        >
          Always allow
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={responding || !connected}
          onClick={() => ignoreFailure(() => controller.respondApproval(approval.id, "allow"))}
        >
          {responding && <LoaderCircle className="animate-spin" />}
          Allow once
        </Button>
      </div>
    </article>
  );
}

function AttachmentChip({
  attachment,
  disabled,
  onRemove,
}: {
  attachment: PanelAttachment;
  disabled: boolean;
  onRemove: () => void;
}) {
  const isImage = attachment.mime.startsWith("image/");
  const detail =
    attachment.status === "error"
      ? attachment.error || "Upload failed"
      : attachment.status === "ready"
        ? `${attachmentSize(attachment.size)} · Ready`
        : attachment.status === "uploading"
          ? "Uploading…"
          : "Queued";

  return (
    <article
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-lg border bg-muted/50 py-1.5 pr-1.5 pl-2",
        attachment.status === "error" ? "border-destructive/25" : "border-border",
      )}
    >
      {isImage ? (
        <ImageIcon className="size-4 shrink-0 text-primary" />
      ) : (
        <FileText className="size-4 shrink-0 text-primary" />
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[11px] font-semibold" title={attachment.name}>
          {attachment.name}
        </span>
        <span
          className={cn(
            "block truncate text-[9px] text-muted-foreground",
            attachment.status === "error" && "text-destructive",
          )}
        >
          {detail}
        </span>
      </span>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        className="size-6 rounded-md"
        aria-label={`Remove ${attachment.name}`}
        disabled={disabled}
        onClick={onRemove}
      >
        <X className="size-3.5" />
      </Button>
    </article>
  );
}

function Composer({ state, controller }: { state: PanelState; controller: PanelController }) {
  const fileInput = useRef<HTMLInputElement>(null);
  const task = globalThis.ZodePanelState.taskById(state, state.currentTaskId);
  const taskId = task?.id || null;
  const draft = controller.currentDraft();
  const action = controller.primaryAction();
  const loading = action === "loading";
  const stopping = action === "stop";
  const submitting = controller.isSubmitting(taskId);
  const interrupting = controller.isInterrupting(taskId);
  const awaitingStop = interrupting || task?.status === "stopping";
  const connected = state.connection === "connected";
  const attachments = controller.getAttachments(taskId);
  const notice = controller.getAttachmentNotice(taskId);
  const uploading = controller.isUploadingAttachments(taskId);
  const readyAttachment = attachments.some((attachment) => attachment.status === "ready");
  const interactionLocked = controller.isNavigating() || controller.isMutating(taskId);
  const controlDisabled =
    !connected || !task || action !== "send" || interactionLocked;
  const sendDisabled =
    interactionLocked ||
    loading ||
    awaitingStop ||
    !connected ||
    !taskId ||
    uploading ||
    (!stopping && draft.trim().length === 0 && !readyAttachment);
  const validModels = state.models.filter((model) => modelId(model));
  const modelValue = task?.model || modelId(validModels[0] || "");
  const sendLabel = loading
    ? "Loading"
    : awaitingStop
      ? "Stopping"
      : submitting
        ? "Sending"
        : stopping
          ? "Stop"
          : "Send";

  return (
    <footer className="composer-shell">
      <form
        className="composer-card"
        onSubmit={(event) => {
          event.preventDefault();
          if (!sendDisabled) ignoreFailure(() => controller.submit());
        }}
      >
        <label className="sr-only" htmlFor="composer-react">
          Task message
        </label>
        <textarea
          id="composer-react"
          rows={2}
          value={draft}
          aria-label="Task message"
          placeholder={
            connected ? "Ask zode to work on this task…" : "写下任务，连接后即可发送…"
          }
          onChange={(event) => {
            const value = event.currentTarget.value;
            ignoreFailure(() => controller.setDraft(value));
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
              event.preventDefault();
              if (!sendDisabled) ignoreFailure(() => controller.submit());
            }
          }}
          className="min-h-14 w-full resize-none bg-transparent px-2 py-1 text-[13px] leading-5 outline-none placeholder:text-muted-foreground/65"
        />

        {attachments.length > 0 && (
          <div className="grid grid-cols-2 gap-2 max-[390px]:grid-cols-1">
            {attachments.map((attachment) => (
              <AttachmentChip
                key={attachment.localId}
                attachment={attachment}
                disabled={interactionLocked}
                onRemove={() =>
                  ignoreFailure(() => controller.removeAttachment(attachment.localId))
                }
              />
            ))}
          </div>
        )}

        {notice && (
          <p role="status" aria-live="polite" className="text-[10px] text-warning">
            {notice}
          </p>
        )}

        <div className="flex min-w-0 items-center gap-1.5">
          <input
            ref={fileInput}
            type="file"
            multiple
            hidden
            accept={ACCEPTED_ATTACHMENTS}
            aria-label="Choose files to attach"
            onChange={(event) => {
              const files = Array.from(event.target.files || []);
              event.target.value = "";
              if (files.length) ignoreFailure(() => controller.addFiles(files));
            }}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Attach files"
            title="Attach files"
            disabled={controlDisabled}
            onClick={() => fileInput.current?.click()}
          >
            <Paperclip />
          </Button>

          {task ? (
            <>
              <Select
                value={task.access || "prompt"}
                disabled={controlDisabled || controller.isSettingAccess(taskId)}
                onValueChange={(value) => ignoreFailure(() => controller.setAccess(value))}
              >
                <SelectTrigger
                  size="sm"
                  aria-label="Task access"
                  className="w-auto min-w-0 flex-none gap-1.5 rounded-md border-transparent bg-transparent px-2 shadow-none hover:bg-muted disabled:bg-transparent"
                >
                  <ShieldCheck className="size-3.5" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="readOnly">Read only</SelectItem>
                  <SelectItem value="prompt">Prompt</SelectItem>
                  <SelectItem value="auto">Auto</SelectItem>
                </SelectContent>
              </Select>

              <Select
                value={modelValue || undefined}
                disabled={
                  controlDisabled ||
                  validModels.length === 0 ||
                  controller.isSettingModel(taskId)
                }
                onValueChange={(value) => ignoreFailure(() => controller.setModel(value))}
              >
                <SelectTrigger
                  size="sm"
                  aria-label="Task model"
                  className="w-auto min-w-0 max-w-48 flex-1 gap-1.5 rounded-md border-transparent bg-transparent px-2 shadow-none hover:bg-muted disabled:bg-transparent"
                >
                  <Bot className="size-3.5" />
                  <SelectValue placeholder="Default" />
                </SelectTrigger>
                <SelectContent>
                  {validModels.map((model) => (
                    <SelectItem key={modelId(model)} value={modelId(model)}>
                      {modelLabel(model)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </>
          ) : (
            <span className="min-w-0 flex-1 truncate px-1 text-[11px] text-muted-foreground/75">
              {connected ? "新建任务后可发送" : "连接后可发送"}
            </span>
          )}

          <Button
            type="submit"
            size="icon-sm"
            variant="ghost"
            aria-label={stopping ? "Stop task" : "Send task"}
            title={sendLabel}
            disabled={sendDisabled}
            className={cn(
              "ml-auto size-8 rounded-lg disabled:bg-muted disabled:text-muted-foreground disabled:opacity-100 disabled:shadow-none",
              stopping
                ? "bg-destructive/10 text-destructive shadow-none hover:bg-destructive/15 hover:text-destructive"
                : "bg-primary/10 text-primary shadow-none hover:bg-primary/15 hover:text-primary",
            )}
          >
            {loading || awaitingStop || submitting ? (
              <LoaderCircle className="animate-spin" />
            ) : stopping ? (
              <Square className="size-2.5 fill-current stroke-[1.5]" />
            ) : (
              <Send className="size-3.5 stroke-[1.75]" />
            )}
          </Button>
        </div>
      </form>
    </footer>
  );
}

function EmptyState({
  state,
  controller,
  hasTask,
}: {
  state: PanelState;
  controller: PanelController;
  hasTask: boolean;
}) {
  const connected = state.connection === "connected";
  const checking = state.connection === "checking";
  const retrying = controller.isRetrying();
  const title = connected
    ? hasTask
      ? "今天想完成什么？"
      : "从这里开始一个任务"
    : checking
      ? "正在连接 Zode"
      : "还差一步即可开始";
  const description = connected
    ? hasTask
      ? "写下目标，Zode 会在浏览器与终端之间持续完成工作。"
      : "创建任务，把当前页面上的工作直接交给 Zode。"
    : checking
      ? "正在唤醒本机 bridge，通常只需几秒。"
      : state.connectionMessage;

  return (
    <section className="empty-state">
      <div className="empty-state-content">
        <div
          className={cn(
            "empty-state-icon",
            connected ? "text-primary" : checking ? "text-muted-foreground" : "text-warning",
          )}
        >
          {checking ? (
            <LoaderCircle className="size-5 animate-spin" />
          ) : connected ? (
            <Sparkles className="size-5" />
          ) : (
            <CircleAlert className="size-5" />
          )}
          {connected && (
            <span className="absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full border-2 border-background bg-success" />
          )}
        </div>
        <h2 className="mt-4 text-[17px] font-semibold tracking-tight">{title}</h2>
        <p className="mt-1.5 text-xs leading-5 text-muted-foreground">{description}</p>

        {!connected && !checking && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="mt-5 bg-card"
            disabled={retrying}
            onClick={() => ignoreFailure(() => controller.retryConnection())}
          >
            <RotateCcw className={cn(retrying && "animate-spin")} />
            重新连接
          </Button>
        )}

        {connected && !hasTask && (
          <Button
            type="button"
            size="sm"
            className="mt-5 px-4"
            disabled={controller.isNavigating() || controller.isMutating()}
            onClick={() => ignoreFailure(() => controller.createTask())}
          >
            {controller.isCreating() ? <LoaderCircle className="animate-spin" /> : <Plus />}
            新建任务
          </Button>
        )}
      </div>
    </section>
  );
}

function Panel({
  state,
  controller,
  action,
}: {
  state: PanelState;
  controller: PanelController;
  action: PanelAction;
}) {
  const scrollContainer = useRef<HTMLDivElement>(null);
  const currentTask = globalThis.ZodePanelState.taskById(state, state.currentTaskId);
  const connected = state.connection === "connected";
  const messages = state.messages.filter(
    (message) => !message.taskId || message.taskId === state.currentTaskId,
  );
  const tools = state.tools.filter((tool) => !tool.taskId || tool.taskId === state.currentTaskId);
  const approvals = state.approvals.filter(
    (approval) =>
      (!approval.taskId || approval.taskId === state.currentTaskId) &&
      approval.status === "pending",
  );
  const conversationItems = [
    ...messages.map((value, index) => ({
      kind: "message" as const,
      value,
      order: value.order,
      fallback: index,
    })),
    ...tools.map((value, index) => ({
      kind: "tool" as const,
      value,
      order: value.order,
      fallback: messages.length + index,
    })),
    ...approvals.map((value, index) => ({
      kind: "approval" as const,
      value,
      order: value.order,
      fallback: messages.length + tools.length + index,
    })),
  ].sort((left, right) => {
    const leftOrder = Number.isSafeInteger(left.order) ? Number(left.order) : left.fallback;
    const rightOrder = Number.isSafeInteger(right.order) ? Number(right.order) : right.fallback;
    return leftOrder - rightOrder || left.fallback - right.fallback;
  });
  // Merge consecutive tool calls into one compact activity group. Grouping
  // happens AFTER the timeline sort so a message/approval between two tools
  // still breaks the run — order is preserved exactly.
  type TimelineItem = (typeof conversationItems)[number];
  const renderItems: Array<
    | { kind: "single"; item: TimelineItem }
    | { kind: "toolGroup"; key: string; tools: PanelTool[] }
  > = [];
  for (const item of conversationItems) {
    if (item.kind === "tool") {
      const last = renderItems[renderItems.length - 1];
      if (last && last.kind === "toolGroup") {
        last.tools.push(item.value);
      } else {
        renderItems.push({ kind: "toolGroup", key: `toolgroup:${item.value.id}`, tools: [item.value] });
      }
    } else {
      renderItems.push({ kind: "single", item });
    }
  }
  const error = globalThis.ZodePanelState.errorForCurrent(state);
  const terminal = globalThis.ZodePanelState.terminalForCurrent(state);
  const hasActivity =
    messages.length > 0 || tools.length > 0 || approvals.length > 0 || error || terminal;
  const taskBusy = currentTask?.status === "running" || currentTask?.status === "stopping";
  const statusLabel = connected
    ? "已连接"
    : state.connection === "checking"
      ? "连接中"
      : "未连接";

  useEffect(() => {
    if (
      action.type !== "message/added" &&
      action.type !== "message/delta" &&
      action.type !== "turn/started" &&
      action.type !== "tool/started" &&
      action.type !== "tool/completed" &&
      action.type !== "approval/requested"
    ) return;
    const viewport = scrollContainer.current?.querySelector<HTMLElement>(
      '[data-slot="scroll-area-viewport"]',
    );
    if (viewport) viewport.scrollTop = viewport.scrollHeight;
  }, [action, conversationItems.length, messages.length]);

  return (
    <div className="panel-app">
      <header className="panel-header">
        <div className="flex min-w-0 items-center gap-2.5">
          <img
            src="icons/zode-128.png"
            alt=""
            className="size-8 shrink-0 rounded-lg"
          />
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <h1 id="panel-title-react" className="truncate text-sm font-semibold tracking-tight">
              zode
            </h1>
            <span
              role="status"
              aria-live="polite"
              title={state.connectionMessage}
              className={cn(
                "inline-flex shrink-0 items-center gap-1.5 text-[10px] font-medium",
                connected
                  ? "text-success"
                  : state.connection === "checking"
                    ? "text-muted-foreground"
                    : "text-warning",
              )}
            >
              <span
                className={cn(
                  "size-1.5 shrink-0 rounded-full",
                  connected
                    ? "bg-success"
                    : state.connection === "checking"
                      ? "animate-pulse bg-muted-foreground"
                      : "bg-warning",
                )}
              />
              {statusLabel}
            </span>
            {taskBusy && (
              <Badge variant="warning" className="max-[360px]:hidden">
                <LoaderCircle className="size-2.5 animate-spin" />
                {currentTask?.status}
              </Badge>
            )}
          </div>
          {state.workspace && (
            <Badge
              variant="outline"
              className="max-w-32 gap-1.5 truncate border-transparent bg-muted/70 py-1 max-[390px]:hidden"
              title={workspaceTitle(state.workspace)}
            >
              <FolderGit2 className="size-3" />
              <span className="truncate">{workspaceText(state.workspace)}</span>
            </Badge>
          )}
          {!connected && state.connection === "disconnected" && hasActivity && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Retry connection"
              title={state.connectionMessage}
              disabled={controller.isRetrying()}
              onClick={() => ignoreFailure(() => controller.retryConnection())}
            >
              <RotateCcw className={cn(controller.isRetrying() && "animate-spin")} />
            </Button>
          )}
        </div>

        {state.tasks.length > 0 && (
          <div className="mt-2.5 flex min-w-0 gap-2">
            <Select
              value={state.currentTaskId || undefined}
              disabled={
                !connected ||
                state.tasks.length === 0 ||
                controller.isNavigating() ||
                controller.isMutating()
              }
              onValueChange={(value) => ignoreFailure(() => controller.selectTask(value))}
            >
              <SelectTrigger className="w-0 min-w-0 flex-1 overflow-hidden rounded-lg border-transparent bg-muted/55 font-medium shadow-none hover:bg-muted">
                <SelectValue placeholder="No tasks" />
              </SelectTrigger>
              <SelectContent>
                {state.tasks.map((task) => (
                  <SelectItem key={task.id} value={task.id}>
                    {taskLabel(task)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label="New task"
              title="New task"
              disabled={!connected || controller.isNavigating() || controller.isMutating()}
              onClick={() => ignoreFailure(() => controller.createTask())}
            >
              {controller.isCreating() ? (
                <LoaderCircle className="animate-spin" />
              ) : (
                <Plus />
              )}
            </Button>
          </div>
        )}
      </header>

      <main className="min-h-0" aria-labelledby="panel-title-react">
        <ScrollArea ref={scrollContainer} className="h-full">
          {!hasActivity ? (
            <EmptyState state={state} controller={controller} hasTask={Boolean(currentTask)} />
          ) : (
            <div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-3.5 py-4">
              {renderItems.map((entry) => {
                if (entry.kind === "toolGroup") {
                  return (
                    <section key={entry.key} aria-label="Tool activity">
                      <ToolGroupCard tools={entry.tools} />
                    </section>
                  );
                }
                const item = entry.item;
                if (item.kind === "message") {
                  const key = `message:${item.value.taskId || ""}:${item.value.id || item.fallback}`;
                  if (item.value.role === "thinking") {
                    return <ThinkingCard key={key} message={item.value} />;
                  }
                  return <MessageCard key={key} message={item.value} />;
                }
                if (item.kind === "approval") {
                  return (
                    <section key={`approval:${item.value.id}`} aria-label="Approval request">
                      <ApprovalCard
                        approval={item.value}
                        controller={controller}
                        connected={connected}
                      />
                    </section>
                  );
                }
                return null;
              })}

              {(error || terminal) && (
                <section aria-label="Task status and errors" aria-live="polite">
                  {error ? (
                    <article className="flex gap-3 rounded-xl border border-destructive/25 bg-destructive/5 p-3.5">
                      <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-destructive/10 text-destructive">
                        <CircleAlert className="size-4" />
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-2">
                          <strong className="text-xs">Task error</strong>
                          <Badge variant="destructive">Failed</Badge>
                        </div>
                        <p className="mt-1.5 text-[11px] leading-5 text-muted-foreground">
                          {error.message || String(error)}
                        </p>
                      </div>
                    </article>
                  ) : (
                    <div className="flex items-center gap-2 rounded-lg border border-success/20 bg-success/5 px-3 py-2.5 text-xs text-success">
                      <CheckCircle2 className="size-4" />
                      Turn {terminal?.status || "completed"}
                    </div>
                  )}
                </section>
              )}
            </div>
          )}
        </ScrollArea>
      </main>

      <Composer state={state} controller={controller} />
    </div>
  );
}

export function App() {
  const initialState = useMemo(() => globalThis.ZodePanelState.initialState(), []);
  const [rendered, setRendered] = useState<{
    state: PanelState;
    controller: PanelController | null;
    action: PanelAction;
    failure?: string;
  }>({ state: initialState, controller: null, action: { type: "react/boot" } });

  useEffect(() => {
    if (!globalThis.ZodePanelApp || !globalThis.ZodePanelState || !globalThis.chrome?.runtime) {
      setRendered((current) => ({
        ...current,
        failure: "The zode browser bridge runtime could not be loaded.",
      }));
      return;
    }

    const view = {
      render(state: PanelState, controller: PanelController, action: PanelAction = {}) {
        setRendered({ state, controller, action });
      },
    };
    const controller = globalThis.ZodePanelApp.createController({
      runtime: chrome.runtime,
      storage: chrome.storage?.local,
      view,
    });
    globalThis.__zodePanelController = controller;
    setRendered({ state: controller.getState(), controller, action: { type: "react/ready" } });
    ignoreFailure(() => controller.start());
  }, []);

  if (rendered.failure) {
    return (
      <main className="grid h-dvh place-items-center bg-background p-6 text-center">
        <div className="max-w-xs rounded-xl border border-destructive/25 bg-card p-5 shadow-sm">
          <CircleAlert className="mx-auto size-6 text-destructive" />
          <h1 className="mt-3 text-sm font-semibold">Unable to start zode</h1>
          <p className="mt-1.5 text-xs leading-5 text-muted-foreground">{rendered.failure}</p>
        </div>
      </main>
    );
  }

  if (!rendered.controller) {
    return (
      <main className="grid h-dvh place-items-center bg-background text-muted-foreground">
        <div className="flex items-center gap-2 text-xs">
          <LoaderCircle className="size-4 animate-spin text-primary" />
          Loading zode…
        </div>
      </main>
    );
  }

  return <Panel state={rendered.state} controller={rendered.controller} action={rendered.action} />;
}
