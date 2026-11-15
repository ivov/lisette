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
  showProblems: () => void;
}

export interface DiagnosticItem {
  severity: "error" | "warning" | "info";
  message: string;
  line: number;
  col: number;
  endLine?: number;
  endCol?: number;
}

function colorCode(line: Element) {
  const parts = (line.textContent ?? "").split(/`([^`]*)`/);
  if (parts.length === 1) return;
  line.replaceChildren(
    ...parts.map((part, index) => {
      if (index % 2 === 0) return part;
      const code = document.createElement("span");
      code.className = "message-code";
      code.textContent = part;
      return code;
    }),
  );
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

  const problemsBox = () =>
    mainEditor.getContribution("editor.contrib.markerController") as unknown as {
      showAtMarker(marker: Monaco.IRange): void;
      close(focusEditor: boolean): void;
      _widget?: {
        position?: Monaco.IPosition;
        computeRequiredHeight(): number;
        _doLayoutBody(height: number, width: number): void;
        showAtMarker(marker: Monaco.editor.IMarkerData, index: number, total: number): void;
        _message: {
          getHeightInLines(): number;
          update(marker: Monaco.editor.IMarkerData): void;
          _messageBlock: HTMLElement;
        };
        _relayout(): void;
      };
    } | null;

  const adjustedBoxes = new WeakSet<object>();

  // Monaco opens at the most severe marker after this position, so the widget is then given this marker.
  const showBox = (marker: Monaco.editor.IMarkerData) => {
    const box = problemsBox();
    box?.showAtMarker(marker);
    const widget = box?._widget;
    if (!widget) return;
    if (!adjustedBoxes.has(widget)) {
      adjustedBoxes.add(widget);
      const lineHeight = mainEditor.getOption(monaco.editor.EditorOption.lineHeight);
      const headHeight = Math.ceil(lineHeight * 1.2);
      const updateMessage = widget._message.update.bind(widget._message);
      widget._message.update = (shown) => {
        updateMessage(shown);
        for (const line of widget._message._messageBlock.querySelectorAll(":scope > div")) colorCode(line);
      };
      const layoutBody = widget._doLayoutBody.bind(widget);
      widget._doLayoutBody = (height, width) => layoutBody(height + headHeight, width);
      widget.computeRequiredHeight = () => {
        const text = mainEditor.getDomNode()?.querySelector<HTMLElement>(".marker-widget .message");
        const wrapped = text ? Math.round(text.offsetHeight / lineHeight) : 0;
        return 2 + Math.max(wrapped, widget._message.getHeightInLines());
      };
    }
    widget.showAtMarker(marker, 1, 1);
    widget._relayout();
  };

  mainEditor.onDidLayoutChange(() => problemsBox()?._widget?._relayout());

  const pillLines = mainEditor.createDecorationsCollection();
  let pills: Monaco.editor.IContentWidget[] = [];

  mainEditor.onDidChangeModelContent(() => {
    for (const pill of pills) mainEditor.layoutContentWidget(pill);
  });

  const makePill = (index: number, onLine: Monaco.editor.IMarkerData[]): Monaco.editor.IContentWidget => {
    const [first] = onLine;
    const kind = first.severity === monaco.MarkerSeverity.Error
      ? "error"
      : first.severity === monaco.MarkerSeverity.Warning ? "warning" : "info";
    const node = document.createElement("button");
    node.className = `problem-pill problem-pill-${kind}`;
    const symbol = kind === "error" ? "✕" : kind === "warning" ? "▲" : "●";
    const icon = document.createElement("span");
    icon.className = "problem-pill-symbol";
    icon.textContent = symbol;
    const label = kind === "info" ? "advisory" : kind;
    node.append(icon, ` ${onLine.length > 1 ? `${onLine.length} problems` : label}`);
    node.title = "Show details";
    node.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
      const box = problemsBox();
      const line = pillLines.getRange(index)?.startLineNumber;
      if (box?._widget?.position?.lineNumber === line) box?.close(true);
      else showBox(first);
    });
    return {
      getId: () => `problem-pill-${index}`,
      getDomNode: () => node,
      getPosition: () => {
        const model = mainEditor.getModel();
        const line = pillLines.getRange(index)?.startLineNumber;
        if (!model || !line) return null;
        return {
          position: { lineNumber: line, column: model.getLineMaxColumn(line) },
          preference: [monaco.editor.ContentWidgetPositionPreference.EXACT],
        };
      },
    };
  };

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
      if (markers.length === 0) problemsBox()?.close(false);

      const byLine = new Map<number, Monaco.editor.IMarkerData[]>();
      for (const marker of [...markers].sort((a, b) => b.severity - a.severity)) {
        if (marker.startLineNumber > model.getLineCount()) continue;
        byLine.set(marker.startLineNumber, [...(byLine.get(marker.startLineNumber) ?? []), marker]);
      }
      for (const pill of pills) mainEditor.removeContentWidget(pill);
      pillLines.set([...byLine.keys()].map((line) => ({
        range: new monaco.Range(line, 1, line, 1),
        options: { stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges },
      })));
      pills = [...byLine.values()].map((onLine, index) => makePill(index, onLine));
      for (const pill of pills) mainEditor.addContentWidget(pill);
    },
    showProblems: () => {
      const model = mainEditor.getModel();
      if (!model) return;
      const [first] = monaco.editor.getModelMarkers({ resource: model.uri, owner: "lisette" });
      if (first) showBox(first);
    },
  };
}
