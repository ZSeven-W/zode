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
  FsReadFile = "fs/readFile",
  FsWriteFile = "fs/writeFile",
  FsCreateDirectory = "fs/createDirectory",
  FsGetMetadata = "fs/getMetadata",
  FsReadDirectory = "fs/readDirectory",
  FsRemove = "fs/remove",
  FsCopy = "fs/copy",
  CommandExec = "command/exec",
  ModelList = "model/list",
  ConfigRead = "config/read",
  ConfigList = "config/list",
  SkillsList = "skills/list",
  SkillsRead = "skills/read",
  HooksList = "hooks/list",
  McpServerStatusList = "mcpServerStatus/list",
  PluginList = "plugin/list",
}

export interface JsonRpcRequest<TParams = unknown> {
  id: RequestId;
  method: string;
  params?: TParams;
}

export interface JsonRpcResponse<TResult = unknown> {
  id: RequestId;
  result: TResult;
}

export interface JsonRpcError {
  id: RequestId;
  error: {
    code: number;
    message: string;
    data?: unknown;
  };
}

export interface JsonRpcNotification<TParams = unknown> {
  method: string;
  params?: TParams;
}

export interface InitializeParams {
  clientInfo: {
    name: string;
    version: string;
  };
}

export interface InitializeResponse {
  serverInfo: {
    name: string;
    version: string;
  };
  zodeHome: string;
  platformFamily: string;
  platformOs: string;
  capabilities: string[];
}

export interface CommandExecParams {
  command: string[];
  cwd?: string;
}

export interface CommandExecResponse {
  processId: string;
  stdout: string;
  stderr: string;
  exitCode?: number;
}
