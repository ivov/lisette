/**
 * Bridge between the TypeScript UI and the Lisette WASM module.
 *
 * The WASM module is built from the `wasm/` Rust crate (wasm-pack output lives
 * in `public/wasm/`).  We load it lazily so it doesn't block the initial render.
 */

export interface LspSession {
  handle(message: string): string;
  free(): void;
}

// Matches the wasm-pack generated exports from public/wasm/lisette_wasm.js
interface LisetteWasmModule {
  default(input?: unknown): Promise<unknown>;
  version(): string;
  format(code: string): string;
  check(code: string): string;
  compile(code: string): string;
  LspSession: new () => LspSession;
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
  crashed?: boolean;
  goSource?: string;
  diagnostics: Diagnostic[];
}

export interface FormatResult {
  ok: boolean;
  formatted?: string;
  error?: string;
}

export interface LisetteBridge {
  readonly version: string;
  format(code: string): Promise<FormatResult>;
  check(code: string): Promise<Diagnostic[]>;
  compile(code: string): Promise<CompileResult>;
}

class WasmBridge implements LisetteBridge {
  readonly version: string;
  constructor(private readonly wasm: LisetteWasmModule) {
    this.version = this.wasm.version();
  }

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
        crashed: true,
        diagnostics: [
          { severity: "error", message: String(e), line: 1, col: 1 },
        ],
      };
    }
  }
}

let _bridge: LisetteBridge | null = null;

export async function loadWasmBridge(): Promise<LisetteBridge> {
  if (_bridge) return _bridge;
  _bridge = new WasmBridge(await importWasmModule());
  return _bridge;
}

export async function loadFreshLspSession(instance: number): Promise<LspSession> {
  const wasm = await importWasmModule(`?instance=${instance}`);
  return new wasm.LspSession();
}

async function importWasmModule(query = ""): Promise<LisetteWasmModule> {
  // Load the wasm-pack generated module from public/wasm/.
  // We pass the URL as a parameter so Vite replaces import.meta.env.BASE_URL
  // at build time, while still hiding the dynamic import from tsc (TS2307).
  // A different query string gives a separate module instance.
  const wasmJsUrl = `${import.meta.env.BASE_URL}wasm/lisette_wasm.js${query}`;
  const loadMod = new Function("url", "return import(url)");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const mod = (await loadMod(wasmJsUrl)) as any;

  // wasm-pack generates a default export that initialises the WASM binary.
  await mod.default();
  return mod as LisetteWasmModule;
}
