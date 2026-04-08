/**
 * TextMate-based syntax highlighting for Lisette.
 *
 * Uses monaco-textmate (which is what monaco-editor-textmate is built on) with
 * onigasm providing the Oniguruma WASM regex engine. This combination uses the
 * findNextMatchSync interface and avoids the compileAG mismatch that occurs
 * when pairing vscode-textmate v9 with onigasm v2.
 *
 * Loads lazily and falls back to the Monarch tokenizer silently on any error.
 */

import type * as Monaco from "monaco-editor";
import { loadWASM } from "onigasm";
import { wireTmGrammars } from "monaco-editor-textmate";

import { LANG_ID } from "./language.js";

const ONIGASM_WASM_URL = `${import.meta.env.BASE_URL}onigasm.wasm`;
const TM_GRAMMAR_URL   = `${import.meta.env.BASE_URL}lisette.tmLanguage.json`;
const TM_SCOPE         = "source.lisette";

let _wirePromise: Promise<void> | null = null;
let _onigasmLoaded = false;

async function ensureOnigasm(): Promise<void> {
  if (_onigasmLoaded) return;
  await loadWASM(ONIGASM_WASM_URL);
  _onigasmLoaded = true;
}

export async function wireTextMateGrammar(monaco: typeof Monaco): Promise<void> {
  if (_wirePromise) return _wirePromise;

  _wirePromise = (async () => {
    await ensureOnigasm();

    // Dynamic import so tsc doesn't complain about the CJS-only monaco-textmate package.
    const { Registry } = await import("monaco-textmate");

    const registry = new Registry({
      getGrammarDefinition: async (scopeName: string) => {
        if (scopeName !== TM_SCOPE) {
          return { format: "json" as const, content: "{}" };
        }
        const resp = await fetch(TM_GRAMMAR_URL);
        if (!resp.ok) throw new Error(`Grammar fetch failed: ${resp.status}`);
        const content = await resp.json() as object;
        return { format: "json" as const, content };
      },
    });

    const grammarMap = new Map([[LANG_ID, TM_SCOPE]]);
    await wireTmGrammars(monaco, registry, grammarMap);
  })();

  return _wirePromise;
}
