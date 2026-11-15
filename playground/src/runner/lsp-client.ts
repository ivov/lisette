import type { LspSession } from "./wasm-bridge.js";

export const DOCUMENT_URI = "file:///playground/playground.lis";

const CHANGE_DELAY_MS = 150;

export interface LspPosition {
  line: number;
  character: number;
}

export interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

export interface LspTextEdit {
  range: LspRange;
  newText: string;
}

export interface LspDiagnostic {
  range: LspRange;
  severity?: number;
  code?: string | number;
  message: string;
}

export interface LspCapabilities {
  completionProvider?: { triggerCharacters?: string[] };
  signatureHelpProvider?: { triggerCharacters?: string[] };
}

interface Message {
  id?: number | string;
  method?: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  params?: any;
  result?: unknown;
  error?: { code: number; message: string };
}

export interface Reply<T> {
  result: T | null;
  error: string | null;
}

export class LspClient {
  capabilities: LspCapabilities = {};
  private session: LspSession | null;
  private text: string;
  private readonly onDiagnostics: (diagnostics: LspDiagnostic[]) => void;
  private readonly loadFreshSession: (instance: number) => Promise<LspSession>;
  private nextId = 1;
  private version = 0;
  private pendingChange: ReturnType<typeof setTimeout> | null = null;
  private opening = false;
  private restarting = false;
  private instances = 0;

  // Node type stripping rejects parameter properties.
  constructor(
    session: LspSession,
    text: string,
    onDiagnostics: (diagnostics: LspDiagnostic[]) => void,
    loadFreshSession: (instance: number) => Promise<LspSession>,
  ) {
    this.session = session;
    this.text = text;
    this.onDiagnostics = onDiagnostics;
    this.loadFreshSession = loadFreshSession;
    this.open();
  }

  change(text: string): void {
    this.text = text;
    if (!this.session) {
      this.restart();
      return;
    }
    if (this.pendingChange !== null) clearTimeout(this.pendingChange);
    this.pendingChange = setTimeout(() => this.flush(), CHANGE_DELAY_MS);
  }

  request<T>(method: string, params: object): T | null {
    return this.requestWithError<T>(method, params).result;
  }

  requestWithError<T>(method: string, params: object): Reply<T> {
    this.flush();
    const id = this.nextId++;
    const reply = this.send({ id, method, params }).find(
      (message) => message.id === id && message.method === undefined,
    );
    if (!reply) return { result: null, error: null };
    if (reply.error) return { result: null, error: reply.error.message };
    return { result: (reply.result ?? null) as T | null, error: null };
  }

  private open(): void {
    this.opening = true;
    this.cancelPendingChange();
    const initialize = this.request<{ capabilities: LspCapabilities }>("initialize", {
      processId: null,
      rootUri: null,
      capabilities: {},
    });
    if (initialize) this.capabilities = initialize.capabilities;
    this.notify("initialized", {});
    this.version++;
    this.notify("textDocument/didOpen", {
      textDocument: {
        uri: DOCUMENT_URI,
        languageId: "lisette",
        version: this.version,
        text: this.text,
      },
    });
    this.opening = false;
  }

  private flush(): void {
    if (this.pendingChange === null) return;
    this.cancelPendingChange();
    this.version++;
    this.notify("textDocument/didChange", {
      textDocument: { uri: DOCUMENT_URI, version: this.version },
      contentChanges: [{ text: this.text }],
    });
  }

  private cancelPendingChange(): void {
    if (this.pendingChange === null) return;
    clearTimeout(this.pendingChange);
    this.pendingChange = null;
  }

  private notify(method: string, params: object): void {
    this.send({ method, params });
  }

  private send(message: Message): Message[] {
    if (!this.session) return [];
    let raw: string;
    try {
      raw = this.session.handle(JSON.stringify({ jsonrpc: "2.0", ...message }));
    } catch (err) {
      console.error("[lsp] The language server stopped:", err);
      this.session = null;
      if (!this.opening) this.restart();
      return [];
    }
    const replies = JSON.parse(raw) as Message[];
    for (const reply of replies) {
      if (
        reply.method === "textDocument/publishDiagnostics" &&
        reply.params?.uri === DOCUMENT_URI
      ) {
        this.onDiagnostics(reply.params.diagnostics as LspDiagnostic[]);
      }
    }
    return replies;
  }

  private restart(): void {
    if (this.restarting) return;
    this.restarting = true;
    this.loadFreshSession(++this.instances).then(
      (session) => {
        this.restarting = false;
        this.session = session;
        this.open();
      },
      (err) => {
        this.restarting = false;
        console.error("[lsp] The language server did not restart:", err);
      },
    );
  }
}
