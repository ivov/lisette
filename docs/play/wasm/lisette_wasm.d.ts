/* tslint:disable */
/* eslint-disable */

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
 * Semantic completion items at byte offset (JSON array).
 */
export function complete(code: string, offset: number): string;

/**
 * Format Lisette source. Returns the formatted source, or the original on failure.
 */
export function format(code: string): string;

/**
 * Go-to-definition at byte offset. Returns JSON `{ "line", "col", "end_line", "end_col" }` or empty.
 */
export function goto_definition(code: string, offset: number): string;

/**
 * Hover info at byte offset. Returns JSON `{ "markdown": "...", ... }` or empty string.
 */
export function hover(code: string, offset: number): string;

export function init(): void;

/**
 * Signature help for a function call at byte offset. Returns JSON or empty string.
 */
export function signature_help(code: string, offset: number): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly check: (a: number, b: number) => [number, number];
    readonly compile: (a: number, b: number) => [number, number];
    readonly complete: (a: number, b: number, c: number) => [number, number];
    readonly format: (a: number, b: number) => [number, number];
    readonly goto_definition: (a: number, b: number, c: number) => [number, number];
    readonly hover: (a: number, b: number, c: number) => [number, number];
    readonly init: () => void;
    readonly signature_help: (a: number, b: number, c: number) => [number, number];
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
