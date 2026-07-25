export type PanelModel =
  | string
  | {
      id?: string;
      value?: string;
      name?: string;
      label?: string;
    };

export interface PanelTask {
  id: string;
  title?: string;
  name?: string;
  status?: string;
  model?: string;
  access?: string;
  activeTurnId?: string | null;
}

export interface PanelMessage {
  id?: string;
  taskId?: string;
  turnId?: string | null;
  order?: number;
  role?: string;
  text?: string;
  content?: string;
}

export interface PanelTool {
  id: string;
  taskId?: string;
  turnId?: string | null;
  order?: number;
  name?: string;
  tool?: string;
  summary?: string;
  status?: string;
  output?: unknown;
}

export interface PanelApproval {
  id: string;
  taskId?: string;
  turnId?: string;
  order?: number;
  status?: string;
  summary?: string;
}

export interface PanelAttachment {
  localId: string;
  name: string;
  mime: string;
  size: number;
  status: string;
  uploadId?: string | null;
  attachmentId?: string | null;
  error?: string | null;
  previewUrl?: string | null;
}

export interface PanelTurnImage {
  name: string;
  mime: string;
  url: string;
}

export interface PanelSelectedElement {
  selector: string;
  label: string;
  tag: string;
  text: string;
  value: string;
  html: string;
  url: string;
  title: string;
  inFrame: boolean;
  rect: { x: number; y: number; width: number; height: number } | null;
  attributes: Array<{ name: string; value: string }>;
}

export interface PanelFeedback {
  taskId?: string | null;
  turnId?: string | null;
  status?: string;
  message?: string;
  code?: string | null;
}

export interface PanelState {
  connection: "checking" | "connected" | "disconnected" | string;
  connectionMessage: string;
  loaded: boolean;
  workspace: string | { name?: string; path?: string; cwd?: string } | null;
  tasks: PanelTask[];
  currentTaskId: string | null;
  models: PanelModel[];
  messages: PanelMessage[];
  tools: PanelTool[];
  approvals: PanelApproval[];
  terminal: PanelFeedback | null;
  error: PanelFeedback | null;
  terminalsByTask: Record<string, PanelFeedback>;
  errorsByTask: Record<string, PanelFeedback>;
  drafts: Record<string, string>;
  collapsedToolIds: string[];
}

export type MarkdownInline = {
  kind: "text" | "code" | "link";
  text: string;
  href?: string;
};

export type MarkdownBlock =
  | { kind: "heading"; level: number; text: string; inlines: MarkdownInline[] }
  | { kind: "paragraph"; text: string; inlines: MarkdownInline[] }
  | { kind: "list"; items: Array<{ text: string; inlines: MarkdownInline[] }> }
  | { kind: "code"; language: string; text: string }
  | {
      kind: "table";
      headers: Array<{ text: string; inlines: MarkdownInline[] }>;
      rows: Array<Array<{ text: string; inlines: MarkdownInline[] }>>;
    };

export interface PanelStateApi {
  initialState(): PanelState;
  taskById(state: PanelState, taskId: string | null): PanelTask | null;
  primaryAction(state: PanelState, taskId: string | null): string;
  currentDraft(state: PanelState): string;
  errorForCurrent(state: PanelState): PanelFeedback | null;
  terminalForCurrent(state: PanelState): PanelFeedback | null;
  safeUrl(value: string): string | null;
  markdownBlocks(markdown: string): MarkdownBlock[];
}

export interface PanelController {
  start(): Promise<unknown>;
  getState(): PanelState;
  currentDraft(): string;
  primaryAction(): string;
  isNavigating(): boolean;
  isMutating(taskId?: string | null): boolean;
  isSubmitting(taskId?: string | null): boolean;
  isInterrupting(taskId?: string | null): boolean;
  isSelecting(): boolean;
  isCreating(): boolean;
  isSettingModel(taskId?: string | null): boolean;
  isSettingAccess(taskId?: string | null): boolean;
  isRetrying(): boolean;
  isRespondingApproval(approvalId: string): boolean;
  isUploadingAttachments(taskId?: string | null): boolean;
  isPickingElement(taskId?: string | null): boolean;
  getTurnImages(taskId?: string | null, turnId?: string | null): PanelTurnImage[];
  getSelectedElement(taskId?: string | null): PanelSelectedElement | null;
  pickElement(): Promise<unknown>;
  cancelElementPick(): Promise<unknown>;
  clearSelectedElement(taskId?: string | null): Promise<unknown>;
  retryConnection(): Promise<unknown>;
  setDraft(value: string): Promise<unknown>;
  toggleTool(toolId: string): Promise<unknown>;
  selectTask(taskId: string): Promise<unknown>;
  createTask(): Promise<unknown>;
  setModel(model: string): Promise<unknown>;
  setAccess(access: string): Promise<unknown>;
  respondApproval(approvalId: string, decision: string): Promise<unknown>;
  getAttachments(taskId?: string | null): PanelAttachment[];
  getAttachmentNotice(taskId?: string | null): string;
  addFiles(files: File[]): Promise<unknown>;
  removeAttachment(localId: string): Promise<unknown>;
  submit(): Promise<unknown>;
}

export interface PanelAction {
  type?: string;
  [key: string]: unknown;
}

declare global {
  var ZodePanelState: PanelStateApi;
  var ZodePanelApp: {
    createController(options: {
      runtime: typeof chrome.runtime;
      storage?: chrome.storage.StorageArea;
      view: {
        render(
          state: PanelState,
          controller: PanelController,
          action?: PanelAction,
        ): void;
      };
    }): PanelController;
  };
  var __zodePanelController: PanelController | undefined;
}

export {};
