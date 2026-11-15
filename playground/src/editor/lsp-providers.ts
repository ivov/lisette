import type * as Monaco from "monaco-editor";
import { LANG_ID } from "./language.js";
import {
  DOCUMENT_URI,
  type LspClient,
  type LspDiagnostic,
  type LspPosition,
  type LspRange,
  type LspTextEdit,
} from "../runner/lsp-client.js";

interface LspLocation {
  uri: string;
  range: LspRange;
}

interface LspLocationLink {
  targetUri: string;
  targetSelectionRange: LspRange;
}

type LspMarkedString = string | { language: string; value: string };

interface LspHover {
  contents: LspMarkedString | LspMarkedString[] | { kind: string; value: string };
  range?: LspRange;
}

interface LspCompletionItem {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: string | { kind: string; value: string };
  sortText?: string;
  filterText?: string;
  insertText?: string;
  textEdit?: LspTextEdit | { newText: string; insert: LspRange; replace: LspRange };
  additionalTextEdits?: LspTextEdit[];
}

interface LspSignatureHelp {
  signatures: {
    label: string;
    documentation?: string | { kind: string; value: string };
    parameters?: { label: string | [number, number]; documentation?: string | { kind: string; value: string } }[];
    activeParameter?: number;
  }[];
  activeSignature?: number;
  activeParameter?: number;
}

interface LspWorkspaceEdit {
  changes?: Record<string, LspTextEdit[]>;
}

interface LspDocumentSymbol {
  name: string;
  detail?: string;
  kind: number;
  range: LspRange;
  selectionRange: LspRange;
  children?: LspDocumentSymbol[];
}

interface LspInlayHint {
  position: LspPosition;
  label: string | { value: string }[];
  kind?: number;
  paddingLeft?: boolean;
  paddingRight?: boolean;
}

interface LspCodeAction {
  title: string;
  kind?: string;
  diagnostics?: LspDiagnostic[];
  isPreferred?: boolean;
  edit?: LspWorkspaceEdit;
}

type PrepareRename =
  | LspRange
  | { range: LspRange; placeholder: string }
  | { defaultBehavior: boolean };

export function registerLspProviders(monaco: typeof Monaco, lsp: LspClient): void {
  const kinds = monaco.languages.CompletionItemKind;
  const completionKinds: Record<number, Monaco.languages.CompletionItemKind> = {
    1: kinds.Text,
    2: kinds.Method,
    3: kinds.Function,
    4: kinds.Constructor,
    5: kinds.Field,
    6: kinds.Variable,
    7: kinds.Class,
    8: kinds.Interface,
    9: kinds.Module,
    10: kinds.Property,
    11: kinds.Unit,
    12: kinds.Value,
    13: kinds.Enum,
    14: kinds.Keyword,
    15: kinds.Snippet,
    16: kinds.Color,
    17: kinds.File,
    18: kinds.Reference,
    19: kinds.Folder,
    20: kinds.EnumMember,
    21: kinds.Constant,
    22: kinds.Struct,
    23: kinds.Event,
    24: kinds.Operator,
    25: kinds.TypeParameter,
  };

  const toPosition = (position: Monaco.Position): LspPosition => ({
    line: position.lineNumber - 1,
    character: position.column - 1,
  });

  const toRange = (range: LspRange): Monaco.IRange => ({
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  });

  const toLspRange = (range: Monaco.IRange): LspRange => ({
    start: { line: range.startLineNumber - 1, character: range.startColumn - 1 },
    end: { line: range.endLineNumber - 1, character: range.endColumn - 1 },
  });

  const toTextEdit = (edit: LspTextEdit): Monaco.languages.TextEdit => ({
    range: toRange(edit.range),
    text: edit.newText,
  });

  const toMarkdown = (value: string | { kind: string; value: string } | undefined) =>
    value === undefined
      ? undefined
      : typeof value === "string"
        ? { value }
        : { value: value.value };

  const toWorkspaceEdit = (
    model: Monaco.editor.ITextModel,
    edit: LspWorkspaceEdit,
  ): Monaco.languages.WorkspaceEdit => ({
    edits: (edit.changes?.[DOCUMENT_URI] ?? []).map((textEdit) => ({
      resource: model.uri,
      textEdit: toTextEdit(textEdit),
      versionId: model.getVersionId(),
    })),
  });

  const toSeverity = (severity: number | undefined): Monaco.MarkerSeverity =>
    severity === 1
      ? monaco.MarkerSeverity.Error
      : severity === 2
        ? monaco.MarkerSeverity.Warning
        : severity === 4
          ? monaco.MarkerSeverity.Hint
          : monaco.MarkerSeverity.Info;

  const atPosition = (position: Monaco.Position) => ({
    textDocument: { uri: DOCUMENT_URI },
    position: toPosition(position),
  });

  const toLocations = (
    model: Monaco.editor.ITextModel,
    result: LspLocation | LspLocation[] | LspLocationLink[] | null,
  ): Monaco.languages.Location[] => {
    const list = result === null ? [] : Array.isArray(result) ? result : [result];
    return list.flatMap((location) => {
      const [uri, range] =
        "targetUri" in location
          ? [location.targetUri, location.targetSelectionRange]
          : [location.uri, location.range];
      return uri === DOCUMENT_URI ? [{ uri: model.uri, range: toRange(range) }] : [];
    });
  };

  monaco.languages.registerHoverProvider(LANG_ID, {
    provideHover(_model, position) {
      const hover = lsp.request<LspHover>("textDocument/hover", atPosition(position));
      if (!hover) return null;
      const contents = Array.isArray(hover.contents) ? hover.contents : [hover.contents];
      return {
        contents: contents.map((content) =>
          typeof content === "string"
            ? { value: content }
            : "language" in content
              ? { value: "```" + content.language + "\n" + content.value + "\n```" }
              : { value: content.value },
        ),
        range: hover.range ? toRange(hover.range) : undefined,
      };
    },
  });

  monaco.languages.registerCompletionItemProvider(LANG_ID, {
    triggerCharacters: lsp.capabilities.completionProvider?.triggerCharacters,
    provideCompletionItems(model, position) {
      const result = lsp.request<LspCompletionItem[] | { items: LspCompletionItem[] }>(
        "textDocument/completion",
        atPosition(position),
      );
      const items = result === null ? [] : Array.isArray(result) ? result : result.items;
      const word = model.getWordUntilPosition(position);
      const wordRange: Monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      return {
        suggestions: items.map((item) => {
          const edit = item.textEdit;
          const range = !edit
            ? wordRange
            : "range" in edit
              ? toRange(edit.range)
              : { insert: toRange(edit.insert), replace: toRange(edit.replace) };
          return {
            label: item.label,
            kind: completionKinds[item.kind ?? 1] ?? kinds.Text,
            detail: item.detail,
            documentation: toMarkdown(item.documentation),
            sortText: item.sortText,
            filterText: item.filterText,
            insertText: edit?.newText ?? item.insertText ?? item.label,
            range,
            additionalTextEdits: item.additionalTextEdits?.map(toTextEdit),
          };
        }),
      };
    },
  });

  monaco.languages.registerSignatureHelpProvider(LANG_ID, {
    signatureHelpTriggerCharacters: lsp.capabilities.signatureHelpProvider?.triggerCharacters,
    provideSignatureHelp(_model, position) {
      const help = lsp.request<LspSignatureHelp>("textDocument/signatureHelp", atPosition(position));
      if (!help || help.signatures.length === 0) return null;
      return {
        value: {
          signatures: help.signatures.map((signature) => ({
            label: signature.label,
            documentation: toMarkdown(signature.documentation),
            parameters: (signature.parameters ?? []).map((parameter) => ({
              label: parameter.label,
              documentation: toMarkdown(parameter.documentation),
            })),
            activeParameter: signature.activeParameter,
          })),
          activeSignature: help.activeSignature ?? 0,
          activeParameter: help.activeParameter ?? 0,
        },
        dispose: () => {},
      };
    },
  });

  monaco.languages.registerDefinitionProvider(LANG_ID, {
    provideDefinition(model, position) {
      return toLocations(
        model,
        lsp.request<LspLocation | LspLocation[] | LspLocationLink[]>(
          "textDocument/definition",
          atPosition(position),
        ),
      );
    },
  });

  monaco.languages.registerReferenceProvider(LANG_ID, {
    provideReferences(model, position, context) {
      return toLocations(
        model,
        lsp.request<LspLocation[]>("textDocument/references", {
          ...atPosition(position),
          context: { includeDeclaration: context.includeDeclaration },
        }),
      );
    },
  });

  monaco.languages.registerRenameProvider(LANG_ID, {
    resolveRenameLocation(model, position) {
      const { result, error } = lsp.requestWithError<PrepareRename>(
        "textDocument/prepareRename",
        atPosition(position),
      );
      if (error) return { range: new monaco.Range(1, 1, 1, 1), text: "", rejectReason: error };
      if (!result || "defaultBehavior" in result) {
        return {
          range: new monaco.Range(1, 1, 1, 1),
          text: "",
          rejectReason: "This name cannot be renamed.",
        };
      }
      const range = "range" in result ? result.range : result;
      return {
        range: toRange(range),
        text: "placeholder" in result ? result.placeholder : model.getValueInRange(toRange(range)),
      };
    },
    provideRenameEdits(model, position, newName) {
      const { result, error } = lsp.requestWithError<LspWorkspaceEdit>("textDocument/rename", {
        ...atPosition(position),
        newName,
      });
      if (error) return { edits: [], rejectReason: error };
      return toWorkspaceEdit(model, result ?? {});
    },
  });

  monaco.languages.registerDocumentSymbolProvider(LANG_ID, {
    provideDocumentSymbols() {
      // Monaco symbol kinds start at 0, LSP kinds at 1.
      const toSymbol = (symbol: LspDocumentSymbol): Monaco.languages.DocumentSymbol => ({
        name: symbol.name,
        detail: symbol.detail ?? "",
        kind: symbol.kind - 1,
        tags: [],
        range: toRange(symbol.range),
        selectionRange: toRange(symbol.selectionRange),
        children: symbol.children?.map(toSymbol),
      });
      const symbols = lsp.request<LspDocumentSymbol[]>("textDocument/documentSymbol", {
        textDocument: { uri: DOCUMENT_URI },
      });
      return (symbols ?? []).map(toSymbol);
    },
  });

  monaco.languages.registerInlayHintsProvider(LANG_ID, {
    provideInlayHints(_model, range) {
      const hints = lsp.request<LspInlayHint[]>("textDocument/inlayHint", {
        textDocument: { uri: DOCUMENT_URI },
        range: toLspRange(range),
      });
      return {
        hints: (hints ?? []).map((hint) => ({
          position: {
            lineNumber: hint.position.line + 1,
            column: hint.position.character + 1,
          },
          label:
            typeof hint.label === "string"
              ? hint.label
              : hint.label.map((part) => ({ label: part.value })),
          kind: hint.kind,
          paddingLeft: hint.paddingLeft,
          paddingRight: hint.paddingRight,
        })),
        dispose: () => {},
      };
    },
  });

  monaco.languages.registerCodeActionProvider(LANG_ID, {
    provideCodeActions(model, range) {
      const actions = lsp.request<LspCodeAction[]>("textDocument/codeAction", {
        textDocument: { uri: DOCUMENT_URI },
        range: toLspRange(range),
      });
      return {
        actions: (actions ?? []).map((action) => ({
          title: action.title,
          kind: action.kind,
          isPreferred: action.isPreferred,
          diagnostics: action.diagnostics?.map((diagnostic) => ({
            ...toRange(diagnostic.range),
            severity: toSeverity(diagnostic.severity),
            message: diagnostic.message,
          })),
          edit: action.edit ? toWorkspaceEdit(model, action.edit) : undefined,
        })),
        dispose: () => {},
      };
    },
  });
}
