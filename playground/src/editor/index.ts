import type * as Monaco from "monaco-editor";
import { LANG_ID, registerLanguage, registerCompletionProvider, registerHoverProvider, registerFormatProvider, registerDefinitionProvider, registerSignatureHelpProvider } from "./language.js";
import { registerThemes, preferredTheme } from "./theme.js";
import { wireTextMateGrammar } from "./textmate.js";
import type { LisetteBridge } from "../runner/wasm-bridge.js";

const INITIAL_CODE = `// Welcome to the Lisette Playground!
// Lisette is a little language with Rust syntax that compiles to Go.
// Press Ctrl+Enter to run, or use the toolbar buttons above.

import "go:fmt"

fn fibonacci(n: int) -> int {
  if n <= 1 {
    n
  } else {
    fibonacci(n - 1) + fibonacci(n - 2)
  }
}

fn main() {
  for i in 0..10 {
    fmt.Printf("fib(%d) = %d\\n", i, fibonacci(i))
  }
}
`;

export interface EditorSetupResult {
  mainEditor: Monaco.editor.IStandaloneCodeEditor;
  goSourceEditor: Monaco.editor.IStandaloneCodeEditor;
  getCode: () => string;
  setGoSource: (source: string) => void;
  setBridge: (bridge: LisetteBridge) => void;
  addMarkers: (diagnostics: DiagnosticItem[]) => void;
  setTheme: (themeName: string) => void;
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
  initialCode?: string,
): Promise<EditorSetupResult> {
  const monaco = await import("monaco-editor");

  registerThemes(monaco);
  registerLanguage(monaco);
  const theme = preferredTheme();

  let bridge: LisetteBridge | null = null;

  // Completion provider backed by WASM bridge
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

  // Format provider – wires Alt+Shift+F to the WASM formatter
  registerFormatProvider(monaco, async (code) => {
    if (!bridge) return null;
    const result = await bridge.format(code);
    return result.ok ? (result.formatted ?? null) : null;
  });

  // Hover provider backed by WASM bridge
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

  // Go-to-definition provider backed by WASM bridge
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

  // Signature help provider backed by WASM bridge
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

  // Upgrade Monarch tokenizer → official TextMate grammar (async, non-blocking).
  // wireTmGrammars re-tokenizes all open models automatically once loaded.
  wireTextMateGrammar(monaco).catch((err) => {
    console.warn("[textmate] Failed to load TM grammar, using Monarch fallback:", err);
  });

  // Main Lisette editor
  const mainEditor = monaco.editor.create(editorContainer, {
    value: initialCode ?? INITIAL_CODE,
    language: LANG_ID,
    theme,
    fontSize: 14,
    lineHeight: 22,
    fontFamily: '"Fira Code", "Cascadia Code", "JetBrains Mono", ui-monospace, monospace',
    fontLigatures: true,
    minimap: { enabled: false },
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
    bracketPairColorization: { enabled: true },
    guides: { bracketPairs: false, indentation: true },
    smoothScrolling: true,
    cursorBlinking: "smooth",
    cursorSmoothCaretAnimation: "on",
    padding: { top: 12, bottom: 12 },
  });

  // Read-only Go output editor
  const goSourceEditor = monaco.editor.create(goSourceContainer, {
    value: "// Compiled Go source will appear here after running your code.",
    language: "go",
    theme,
    fontSize: 13,
    lineHeight: 21,
    fontFamily: '"Fira Code", "Cascadia Code", "JetBrains Mono", ui-monospace, monospace',
    fontLigatures: true,
    minimap: { enabled: false },
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
    setTheme: (themeName: string) => {
      monaco.editor.setTheme(themeName);
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
