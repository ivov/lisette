import type * as Monaco from "monaco-editor";
import { LANG_ID, registerLanguage, registerCompletionProvider, registerHoverProvider, registerFormatProvider, registerDefinitionProvider, registerSignatureHelpProvider } from "./language.js";
import { registerTheme, THEME } from "./theme.js";
import { wireTextMateGrammar } from "./textmate.js";
import type { LisetteBridge } from "../runner/wasm-bridge.js";

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
  const monaco = await import("monaco-editor");

  registerTheme(monaco);
  registerLanguage(monaco);

  let bridge: LisetteBridge | null = null;

  registerCompletionProvider(monaco, async (model, position) => {
    if (!bridge) return [];
    const code = model.getValue();
    const offset = model.getOffsetAt(position);
    const completions = await bridge.complete(code, offset);
    const word = model.getWordUntilPosition(position);
    const range: Monaco.IRange = {
      startLineNumber: position.lineNumber,
      endLineNumber: position.lineNumber,
      startColumn: word.startColumn,
      endColumn: word.endColumn,
    };
    return completions.map((c) => ({
      label: c.label,
      kind: mapCompletionKind(monaco, c.kind),
      detail: c.detail,
      documentation: c.documentation,
      insertText: c.insertText ?? c.label,
      range,
    }));
  });

  registerFormatProvider(monaco, async (code) => {
    if (!bridge) return null;
    const result = await bridge.format(code);
    return result.ok ? (result.formatted ?? null) : null;
  });

  registerHoverProvider(monaco, async (model, position) => {
    if (!bridge) return null;
    const code = model.getValue();
    const offset = model.getOffsetAt(position);
    const hover = await bridge.hover(code, offset);
    if (!hover) return null;
    const result: Monaco.languages.Hover = {
      contents: [{ value: hover.markdown }],
    };
    if (hover.startLine && hover.startCol && hover.endLine && hover.endCol) {
      result.range = {
        startLineNumber: hover.startLine,
        startColumn: hover.startCol,
        endLineNumber: hover.endLine,
        endColumn: hover.endCol,
      };
    }
    return result;
  });

  registerDefinitionProvider(monaco, async (model, position) => {
    if (!bridge) return null;
    const code = model.getValue();
    const offset = model.getOffsetAt(position);
    const def = await bridge.gotoDefinition(code, offset);
    if (!def) return null;
    return {
      uri: model.uri,
      range: {
        startLineNumber: def.line,
        startColumn: def.col,
        endLineNumber: def.endLine,
        endColumn: def.endCol,
      },
    };
  });

  registerSignatureHelpProvider(monaco, async (model, position) => {
    if (!bridge) return null;
    const code = model.getValue();
    const offset = model.getOffsetAt(position);
    const sig = await bridge.signatureHelp(code, offset);
    if (!sig) return null;
    return {
      value: {
        signatures: [{
          label: sig.label,
          parameters: sig.parameters.map((p) => ({ label: p })),
        }],
        activeSignature: 0,
        activeParameter: sig.activeParameter,
      },
      dispose: () => {},
    };
  });

  // The grammar loads in the background and re-tokenizes open models when ready.
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
    stickyScroll: { enabled: false },
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
    stickyScroll: { enabled: false },
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

function mapCompletionKind(
  monaco: typeof Monaco,
  kind: string | undefined
): Monaco.languages.CompletionItemKind {
  switch (kind) {
    case "function":  return monaco.languages.CompletionItemKind.Function;
    case "variable":  return monaco.languages.CompletionItemKind.Variable;
    case "type":      return monaco.languages.CompletionItemKind.Class;
    case "keyword":   return monaco.languages.CompletionItemKind.Keyword;
    case "module":    return monaco.languages.CompletionItemKind.Module;
    case "field":     return monaco.languages.CompletionItemKind.Field;
    case "method":    return monaco.languages.CompletionItemKind.Method;
    case "enum":      return monaco.languages.CompletionItemKind.Enum;
    case "constant":  return monaco.languages.CompletionItemKind.Constant;
    case "snippet":   return monaco.languages.CompletionItemKind.Snippet;
    default:          return monaco.languages.CompletionItemKind.Text;
  }
}
