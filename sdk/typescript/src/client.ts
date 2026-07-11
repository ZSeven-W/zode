import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import WebSocket from "ws";
import {
  JsonRpcVersion,
  classifyIncomingFrame,
  type ApprovalDecision,
  type ApprovalRequestParams,
  type InitializeOptions,
  type InitializeParams,
  type InitializeResponse,
  type JsonRpcError,
  type JsonRpcNotification,
  type JsonRpcRequest,
  type ProtocolMethod,
  type RequestId,
} from "./protocol.js";

export interface ZodeClientOptions { binary?: string; serverArgs?: string[]; env?: NodeJS.ProcessEnv }
export interface WebSocketOptions { url: string; token: string }
type NotificationHandler = (notification: JsonRpcNotification) => void;
type ApprovalHandler = (params: ApprovalRequestParams) => ApprovalDecision | Promise<ApprovalDecision>;

export class RpcError extends Error {
  readonly code: number;
  readonly data?: unknown;
  constructor(error: JsonRpcError["error"]) {
    super(error.message);
    this.code = error.code;
    this.data = error.data;
  }
}

export class ZodeClient {
  readonly binary: string;
  readonly serverArgs: readonly string[];
  private readonly env?: NodeJS.ProcessEnv;
  private child?: ChildProcessWithoutNullStreams;
  private lines?: Interface;
  private socket?: WebSocket;
  private nextId = 1;
  private notificationHandler?: NotificationHandler;
  private approvalHandler?: ApprovalHandler;
  private pending = new Map<RequestId, { resolve: (value: unknown) => void; reject: (reason: unknown) => void }>();

  constructor(options: ZodeClientOptions = {}) {
    this.binary = options.binary ?? "zode";
    this.serverArgs = options.serverArgs ?? ["server"];
    this.env = options.env;
  }

  static async connectWebSocket(options: WebSocketOptions): Promise<ZodeClient> {
    const client = new ZodeClient();
    const socket = new WebSocket(options.url, { headers: { Authorization: `Bearer ${options.token}` } });
    client.socket = socket;
    socket.on("message", (data, isBinary) => {
      if (!isBinary) client.routeText(data.toString());
    });
    socket.on("close", () => client.rejectPending(new Error("zode websocket closed")));
    socket.on("error", (error) => client.rejectPending(error));
    await new Promise<void>((resolve, reject) => {
      socket.once("open", resolve);
      socket.once("error", reject);
    });
    return client;
  }

  start(): void {
    if (this.child || this.socket) return;
    this.child = spawn(this.binary, this.serverArgs, { stdio: "pipe", env: this.env });
    this.lines = createInterface({ input: this.child.stdout });
    this.lines.on("line", (line) => this.routeText(line));
    this.child.on("exit", () => this.rejectPending(new Error("zode server exited")));
  }

  initialize(name = "zode-sdk-js", version = "0.0.0", options: InitializeOptions = {}): Promise<InitializeResponse> {
    const params: InitializeParams = { clientInfo: { name, version } };
    if (options.approvalPolicy !== undefined) params.approvalPolicy = options.approvalPolicy;
    return this.request("initialize", params);
  }

  onNotification(handler: NotificationHandler): () => void {
    this.notificationHandler = handler;
    return () => { if (this.notificationHandler === handler) this.notificationHandler = undefined; };
  }

  onApprovalRequest(handler: ApprovalHandler): () => void {
    this.approvalHandler = handler;
    return () => { if (this.approvalHandler === handler) this.approvalHandler = undefined; };
  }

  request<TParams, TResult>(method: string | ProtocolMethod, params: TParams): Promise<TResult> {
    this.ensureStarted();
    const id = this.nextId++;
    const frame: JsonRpcRequest<TParams> = { jsonrpc: JsonRpcVersion, id, method, params };
    return new Promise<TResult>((resolve, reject) => {
      this.pending.set(id, { resolve: (value) => resolve(value as TResult), reject });
      try { this.write(frame); } catch (error) { this.pending.delete(id); reject(error); }
    });
  }

  notify<TParams>(method: string | ProtocolMethod, params?: TParams): void {
    this.ensureStarted();
    this.write({ jsonrpc: JsonRpcVersion, method, ...(params === undefined ? {} : { params }) });
  }

  close(): void {
    this.lines?.close();
    this.child?.kill();
    this.socket?.close();
    this.lines = undefined;
    this.child = undefined;
    this.socket = undefined;
    this.rejectPending(new Error("zode client closed"));
  }

  private ensureStarted(): void { if (!this.child && !this.socket) this.start(); }

  private write(value: unknown): void {
    const text = JSON.stringify(value);
    if (this.socket) {
      if (this.socket.readyState !== WebSocket.OPEN) throw new Error("zode websocket is not open");
      this.socket.send(text);
    } else if (!this.child?.stdin.write(`${text}\n`)) {
      throw new Error("failed to write to zode server");
    }
  }

  private routeText(text: string): void {
    let classified;
    try { classified = classifyIncomingFrame(JSON.parse(text)); } catch { return; }
    const { kind, frame } = classified;
    if (kind === "response" || kind === "error") {
      const pending = this.pending.get(frame.id);
      if (!pending) return;
      this.pending.delete(frame.id);
      if (kind === "response") pending.resolve(frame.result);
      else pending.reject(new RpcError(frame.error));
    } else if (kind === "notification") {
      this.notificationHandler?.(frame);
    } else {
      queueMicrotask(() => void this.answerServerRequest(frame));
    }
  }

  private async answerServerRequest(request: JsonRpcRequest): Promise<void> {
    if (request.method !== "approval/request") {
      this.write({ jsonrpc: JsonRpcVersion, id: request.id, error: { code: -32601, message: "method not found" } });
      return;
    }
    let decision: ApprovalDecision = "deny";
    try {
      if (this.approvalHandler) decision = await this.approvalHandler(request.params as ApprovalRequestParams);
    } catch { decision = "deny"; }
    this.write({ jsonrpc: JsonRpcVersion, id: request.id, result: { decision } });
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
  }
}
