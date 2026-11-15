import type * as Monaco from "monaco-editor";
import { LANG_ID, registerLanguage, registerFormatProvider, registerSnippetProvider } from "./language.js";
import { registerLspProviders } from "./lsp-providers.js";
import { registerTheme, THEME } from "./theme.js";
import { wireTextMateGrammar } from "./textmate.js";
import type { LisetteBridge } from "../runner/wasm-bridge.js";
import type { LspClient } from "../runner/lsp-client.js";

export const GO_PLACEHOLDER = "// The emitted Go appears here once the code compiles.";

const FONT_MONO =
  'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';

// Monaco's configuration accepts only this flat key, not the nested option.
const BRACKET_COLOURS_OFF = { "bracketPairColorization.enabled": false };

export interface EditorSetupResult {
  mainEditor: Monaco.editor.IStandaloneCodeEditor;
  goSourceEditor: Monaco.editor.IStandaloneCodeEditor;
  getCode: () => string;
  setGoSource: (source: string) => void;
  setBridge: (bridge: LisetteBridge) => void;
  setLsp: (lsp: LspClient) => void;
  addMarkers: (diagnostics: DiagnosticItem[]) => void;
}

export interface DiagnosticItem {
  severity: "error" | "warning" | "info";
  message: string;
  line: number;
  col: number;
  endLine?: number;
  endCol?: number;
}

export async function setupEditors(
  editorContainer: HTMLElement,
  goSourceContainer: HTMLElement,
  initialCode: string,
): Promise<EditorSetupResult> {
  const monaco = await import("./monaco.js");

  registerTheme(monaco);
  registerLanguage(monaco);
  monaco.editor.addKeybindingRule({ keybinding: monaco.KeyCode.F12, command: null });

  let bridge: LisetteBridge | null = null;

  registerSnippetProvider(monaco);

  registerFormatProvider(monaco, async (code) => {
    if (!bridge) return null;
    const result = await bridge.format(code);
    return result.ok ? (result.formatted ?? null) : null;
  });

  wireTextMateGrammar(monaco).catch((err) => {
    console.warn("[textmate] Failed to load TM grammar, using Monarch fallback:", err);
  });

  const mainEditor = monaco.editor.create(editorContainer, {
    value: initialCode,
    language: LANG_ID,
    theme: THEME,
    fontSize: 14,
    lineHeight: 22,
    fontFamily: FONT_MONO,
    minimap: { enabled: false },
    overviewRulerLanes: 0,
    overviewRulerBorder: false,
    hideCursorInOverviewRuler: true,
    fixedOverflowWidgets: true,
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 2,
    insertSpaces: true,
    wordWrap: "on",
    suggest: {
      showKeywords: true,
      showSnippets: true,
    },
    renderLineHighlight: "line",
    inlayHints: { enabled: "offUnlessPressed" },
    ...BRACKET_COLOURS_OFF,
    guides: { bracketPairs: false, indentation: true },
    smoothScrolling: true,
    cursorBlinking: "solid",
    cursorSmoothCaretAnimation: "on",
    padding: { top: 12, bottom: 12 },
  });

  const goSourceEditor = monaco.editor.create(goSourceContainer, {
    value: GO_PLACEHOLDER,
    language: "go",
    theme: THEME,
    fontSize: 14,
    lineHeight: 22,
    fontFamily: FONT_MONO,
    minimap: { enabled: false },
    overviewRulerLanes: 0,
    overviewRulerBorder: false,
    hideCursorInOverviewRuler: true,
    fixedOverflowWidgets: true,
    scrollBeyondLastLine: false,
    automaticLayout: true,
    readOnly: true,
    renderLineHighlight: "none",
    padding: { top: 12, bottom: 12 },
  });

  return {
    mainEditor,
    goSourceEditor,
    getCode: () => mainEditor.getValue(),
    setGoSource: (src: string) => {
      goSourceEditor.setValue(src);
    },
    setBridge: (b: LisetteBridge) => {
      bridge = b;
    },
    setLsp: (lsp: LspClient) => {
      registerLspProviders(monaco, lsp);
    },
    addMarkers: (diagnostics: DiagnosticItem[]) => {
      const model = mainEditor.getModel();
      if (!model) return;
      const markers: Monaco.editor.IMarkerData[] = diagnostics.map((d) => ({
        severity:
          d.severity === "error"
            ? monaco.MarkerSeverity.Error
            : d.severity === "warning"
            ? monaco.MarkerSeverity.Warning
            : monaco.MarkerSeverity.Info,
        message: d.message,
        startLineNumber: d.line,
        startColumn: d.col,
        endLineNumber: d.endLine ?? d.line,
        endColumn: d.endCol ?? d.col + 1,
      }));
      monaco.editor.setModelMarkers(model, "lisette", markers);
    },
  };
}
