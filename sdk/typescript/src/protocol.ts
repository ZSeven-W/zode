export const JsonRpcVersion = "2.0" as const;
export type RequestId = number | string;

export enum ProtocolMethod {
  Initialize = "initialize",
  ThreadStart = "thread/start",
  ThreadResume = "thread/resume",
  ThreadList = "thread/list",
  ThreadRead = "thread/read",
  ThreadDelete = "thread/delete",
  ThreadNameSet = "thread/name/set",
  TurnStart = "turn/start",
  TurnInterrupt = "turn/interrupt",
  FsReadFile = "fs/readFile",
  FsWriteFile = "fs/writeFile",
  FsCreateDirectory = "fs/createDirectory",
  FsGetMetadata = "fs/getMetadata",
  FsReadDirectory = "fs/readDirectory",
  FsRemove = "fs/remove",
  FsCopy = "fs/copy",
  CommandExec = "command/exec",
  ModelList = "model/list",
  ModelSet = "model/set",
  ConfigRead = "config/read",
  ConfigList = "config/list",
  ConfigWrite = "config/write",
  SkillsList = "skills/list",
  SkillsRead = "skills/read",
  HooksList = "hooks/list",
  McpServerStatusList = "mcpServerStatus/list",
  PluginList = "plugin/list",
}

interface JsonRpcEnvelope { jsonrpc: typeof JsonRpcVersion }
export interface JsonRpcRequest<TParams = unknown> extends JsonRpcEnvelope { id: RequestId; method: string; params?: TParams }
export interface JsonRpcResponse<TResult = unknown> extends JsonRpcEnvelope { id: RequestId; result: TResult }
export interface JsonRpcError extends JsonRpcEnvelope { id: RequestId; error: { code: number; message: string; data?: unknown } }
export interface JsonRpcNotification<TParams = unknown> extends JsonRpcEnvelope { method: string; params?: TParams }

export type IncomingFrame = JsonRpcResponse | JsonRpcError | JsonRpcNotification | JsonRpcRequest;
export type ClassifiedIncomingFrame =
  | { kind: "response"; frame: JsonRpcResponse }
  | { kind: "error"; frame: JsonRpcError }
  | { kind: "notification"; frame: JsonRpcNotification }
  | { kind: "serverRequest"; frame: JsonRpcRequest };

export function classifyIncomingFrame(value: unknown): ClassifiedIncomingFrame {
  if (!value || typeof value !== "object" || (value as { jsonrpc?: unknown }).jsonrpc !== JsonRpcVersion) {
    throw new Error("invalid JSON-RPC 2.0 frame");
  }
  const frame = value as Record<string, unknown>;
  if ("method" in frame) {
    if (typeof frame.method !== "string") throw new Error("invalid JSON-RPC method");
    return "id" in frame
      ? { kind: "serverRequest", frame: frame as unknown as JsonRpcRequest }
      : { kind: "notification", frame: frame as unknown as JsonRpcNotification };
  }
  if (!("id" in frame)) throw new Error("invalid JSON-RPC response");
  if ("error" in frame) return { kind: "error", frame: frame as unknown as JsonRpcError };
  if ("result" in frame) return { kind: "response", frame: frame as unknown as JsonRpcResponse };
  throw new Error("invalid JSON-RPC response");
}

export type ApprovalPolicy = "auto" | "readOnly" | "prompt";
export type ApprovalDecision = "allow" | "allowAlways" | "deny";
export interface ApprovalRequestParams {
  approvalId: string;
  kind: string;
  summary: string;
  threadId?: string;
  turnId?: string;
  tool?: string;
  input?: unknown;
}

export interface InitializeParams {
  clientInfo: { name: string; version: string };
  approvalPolicy?: ApprovalPolicy;
}
export interface InitializeOptions { approvalPolicy?: ApprovalPolicy }
export interface InitializeResponse {
  serverInfo: { name: string; version: string };
  zodeHome: string;
  platformFamily: string;
  platformOs: string;
  capabilities: string[];
  approvalPolicy?: ApprovalPolicy;
}
export interface CommandExecParams { command: string[]; cwd?: string }
export interface CommandExecResponse { processId: string; stdout: string; stderr: string; exitCode?: number }
