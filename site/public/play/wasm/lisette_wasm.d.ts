/* tslint:disable */
/* eslint-disable */

export class LspSession {
    free(): void;
    [Symbol.dispose](): void;
    handle(message: string): string;
    constructor();
}

/**
 * Type-check source and return a JSON array of diagnostics.
 */
export function check(code: string): string;

/**
 * Compile Lisette → Go. Returns a JSON object:
 * `{ "ok": bool, "go_source": "...", "diagnostics": [...] }`
 */
export function compile(code: string): string;

/**
 * Format Lisette source. Returns the formatted source, or the original on failure.
 */
export function format(code: string): string;

export function init(): void;

/**
 * The Lisette compiler version this WASM was built against.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_lspsession_free: (a: number, b: number) => void;
    readonly check: (a: number, b: number) => [number, number];
    readonly compile: (a: number, b: number) => [number, number];
    readonly format: (a: number, b: number) => [number, number];
    readonly init: () => void;
    readonly lspsession_handle: (a: number, b: number, c: number) => [number, number];
    readonly lspsession_new: () => number;
    readonly version: () => [number, number];
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
