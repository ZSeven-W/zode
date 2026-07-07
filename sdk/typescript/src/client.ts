import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import type {
  InitializeParams,
  InitializeResponse,
  JsonRpcError,
  JsonRpcNotification,
  JsonRpcRequest,
  JsonRpcResponse,
  ProtocolMethod,
  RequestId,
} from "./protocol.js";

export interface ZodeClientOptions {
  binary?: string;
}

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
  private child?: ChildProcessWithoutNullStreams;
  private lines?: Interface;
  private nextId = 1;
  private pending = new Map<RequestId, {
    resolve: (value: unknown) => void;
    reject: (reason: unknown) => void;
  }>();

  constructor(options: ZodeClientOptions = {}) {
    this.binary = options.binary ?? "zode";
  }

  start(): void {
    if (this.child) return;
    this.child = spawn(this.binary, ["server"], { stdio: "pipe" });
    this.lines = createInterface({ input: this.child.stdout });
    this.lines.on("line", (line) => this.routeLine(line));
    this.child.on("exit", () => {
      for (const pending of this.pending.values()) {
        pending.reject(new Error("zode server exited"));
      }
      this.pending.clear();
    });
  }

  initialize(name = "zode-sdk-js", version = "0.0.0"): Promise<InitializeResponse> {
    const params: InitializeParams = { clientInfo: { name, version } };
    return this.request<InitializeParams, InitializeResponse>("initialize", params);
  }

  request<TParams, TResult>(method: string | ProtocolMethod, params: TParams): Promise<TResult> {
    this.ensureStarted();
    const id = this.nextId++;
    const request: JsonRpcRequest<TParams> = { id, method, params };
    return new Promise<TResult>((resolve, reject) => {
      this.pending.set(id, {
        resolve: (value) => resolve(value as TResult),
        reject,
      });
      this.writeLine(request);
    });
  }

  notify<TParams>(method: string | ProtocolMethod, params?: TParams): void {
    this.ensureStarted();
    const notification: JsonRpcNotification<TParams> = { method, params };
    this.writeLine(notification);
  }

  close(): void {
    this.lines?.close();
    this.child?.kill();
    this.lines = undefined;
    this.child = undefined;
  }

  private ensureStarted(): void {
    if (!this.child) {
      this.start();
    }
  }

  private writeLine(value: unknown): void {
    if (!this.child?.stdin.write(`${JSON.stringify(value)}\n`)) {
      throw new Error("failed to write to zode server");
    }
  }

  private routeLine(line: string): void {
    const message = JSON.parse(line) as JsonRpcResponse | JsonRpcError;
    if ("result" in message) {
      const pending = this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id);
        pending.resolve(message.result);
      }
      return;
    }
    if ("error" in message) {
      const pending = this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id);
        pending.reject(new RpcError(message.error));
      }
    }
  }
}
