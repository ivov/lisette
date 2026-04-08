/**
 * Bridge between the TypeScript UI and the Lisette WASM module.
 *
 * The WASM module is built from the `wasm/` Rust crate (wasm-pack output lives
 * in `public/wasm/`).  We load it lazily so it doesn't block the initial render.
 */

// Matches the wasm-pack generated exports from public/wasm/lisette_wasm.js
interface LisetteWasmModule {
  default(input?: unknown): Promise<unknown>;
  format(code: string): string;
  check(code: string): string;
  compile(code: string): string;
  complete(code: string, offset: number): string;
  hover(code: string, offset: number): string;
  goto_definition(code: string, offset: number): string;
  signature_help(code: string, offset: number): string;
}

export interface Diagnostic {
  severity: "error" | "warning" | "info";
  message: string;
  /** 1-based */
  line: number;
  /** 1-based */
  col: number;
  endLine?: number;
  endCol?: number;
  code?: string;
}

export interface CompileResult {
  ok: boolean;
  goSource?: string;
  diagnostics: Diagnostic[];
}

export interface FormatResult {
  ok: boolean;
  formatted?: string;
  error?: string;
}

export interface CompletionItem {
  label: string;
  kind?: string;
  detail?: string;
  documentation?: string;
  insertText?: string;
}

export interface HoverResult {
  markdown: string;
  startLine?: number;
  startCol?: number;
  endLine?: number;
  endCol?: number;
}

export interface DefinitionResult {
  line: number;
  col: number;
  endLine: number;
  endCol: number;
}

export interface SignatureHelpResult {
  label: string;
  parameters: string[];
  activeParameter: number;
}

export interface LisetteBridge {
  format(code: string): Promise<FormatResult>;
  check(code: string): Promise<Diagnostic[]>;
  compile(code: string): Promise<CompileResult>;
  complete(code: string, offset: number): Promise<CompletionItem[]>;
  hover(code: string, offset: number): Promise<HoverResult | null>;
  gotoDefinition(code: string, offset: number): Promise<DefinitionResult | null>;
  signatureHelp(code: string, offset: number): Promise<SignatureHelpResult | null>;
}

class WasmBridge implements LisetteBridge {
  constructor(private readonly wasm: LisetteWasmModule) {}

  async format(code: string): Promise<FormatResult> {
    try {
      const formatted = this.wasm.format(code);
      return { ok: true, formatted };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  }

  async check(code: string): Promise<Diagnostic[]> {
    try {
      const raw = this.wasm.check(code);
      return JSON.parse(raw) as Diagnostic[];
    } catch {
      return [];
    }
  }

  async compile(code: string): Promise<CompileResult> {
    try {
      const raw = this.wasm.compile(code);
      const parsed = JSON.parse(raw) as {
        ok: boolean;
        go_source?: string;
        diagnostics: Diagnostic[];
      };
      return {
        ok: parsed.ok,
        goSource: parsed.go_source,
        diagnostics: parsed.diagnostics,
      };
    } catch (e) {
      return {
        ok: false,
        diagnostics: [
          { severity: "error", message: String(e), line: 1, col: 1 },
        ],
      };
    }
  }

  async complete(code: string, offset: number): Promise<CompletionItem[]> {
    try {
      const raw = this.wasm.complete(code, offset);
      return JSON.parse(raw) as CompletionItem[];
    } catch {
      return [];
    }
  }

  async hover(code: string, offset: number): Promise<HoverResult | null> {
    try {
      const raw = this.wasm.hover(code, offset);
      if (!raw) return null;
      const parsed = JSON.parse(raw) as {
        markdown: string;
        start_line?: number;
        start_col?: number;
        end_line?: number;
        end_col?: number;
      };
      return {
        markdown: parsed.markdown,
        startLine: parsed.start_line,
        startCol: parsed.start_col,
        endLine: parsed.end_line,
        endCol: parsed.end_col,
      };
    } catch {
      return null;
    }
  }

  async gotoDefinition(code: string, offset: number): Promise<DefinitionResult | null> {
    try {
      const raw = this.wasm.goto_definition(code, offset);
      if (!raw) return null;
      const parsed = JSON.parse(raw) as {
        line: number;
        col: number;
        end_line: number;
        end_col: number;
      };
      return {
        line: parsed.line,
        col: parsed.col,
        endLine: parsed.end_line,
        endCol: parsed.end_col,
      };
    } catch {
      return null;
    }
  }

  async signatureHelp(code: string, offset: number): Promise<SignatureHelpResult | null> {
    try {
      const raw = this.wasm.signature_help(code, offset);
      if (!raw) return null;
      const parsed = JSON.parse(raw) as {
        label: string;
        parameters: string[];
        active_parameter: number;
      };
      return {
        label: parsed.label,
        parameters: parsed.parameters,
        activeParameter: parsed.active_parameter,
      };
    } catch {
      return null;
    }
  }
}

let _bridge: LisetteBridge | null = null;

export async function loadWasmBridge(): Promise<LisetteBridge> {
  if (_bridge) return _bridge;

  // Load the wasm-pack generated module from public/wasm/.
  // We pass the URL as a parameter so Vite replaces import.meta.env.BASE_URL
  // at build time, while still hiding the dynamic import from tsc (TS2307).
  const wasmJsUrl = `${import.meta.env.BASE_URL}wasm/lisette_wasm.js`;
  const loadMod = new Function("url", "return import(url)");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mod = (await loadMod(wasmJsUrl)) as any;

  // wasm-pack generates a default export that initialises the WASM binary.
  await mod.default();

  _bridge = new WasmBridge(mod as LisetteWasmModule);
  return _bridge;
}
