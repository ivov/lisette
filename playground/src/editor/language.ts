import type * as Monaco from "monaco-editor";

export const LANG_ID = "lisette";

// All keywords from the Lisette language reference + VSCode extension grammar
export const LISETTE_KEYWORDS = [
  // control flow
  "as", "break", "continue", "else", "for", "if", "in", "loop",
  "match", "return", "while",
  // storage
  "const", "enum", "fn", "impl", "interface", "struct", "type", "var",
  // modifiers
  "pub", "mut",
  // others
  "defer", "import", "let", "recover", "select", "task", "try",
  // special variables (treated as keywords for completion purposes)
  "self",
];

// Built-in primitive types
export const LISETTE_PRIMITIVE_TYPES = [
  "int", "int8", "int16", "int32", "int64",
  "uint", "uint8", "uint16", "uint32", "uint64",
  "uintptr", "byte", "rune",
  "float32", "float64",
  "complex64", "complex128",
  "bool", "string", "error",
  // Built-in type + built-in functions
  "Never", "panic", "assert_type",
];

// Generic/compound types
export const LISETTE_COMPOUND_TYPES = [
  "Option", "Result", "Slice", "Map", "Ref", "Channel", "Sender", "Receiver",
  "Unknown",
  "Range", "RangeInclusive", "RangeFrom", "RangeTo", "RangeToInclusive",
];

// Enum variants / value constructors
export const LISETTE_CONSTRUCTORS = [
  "Some", "None", "Ok", "Err",
  "true", "false",
];

export const ALL_TYPES = [...LISETTE_PRIMITIVE_TYPES, ...LISETTE_COMPOUND_TYPES];

export function registerLanguage(monaco: typeof Monaco): void {
  monaco.languages.register({
    id: LANG_ID,
    extensions: [".lis"],
    aliases: ["Lisette", "lisette"],
    mimetypes: ["text/x-lisette"],
  });

  monaco.languages.setLanguageConfiguration(LANG_ID, {
    comments: { lineComment: "//", blockComment: ["/*", "*/"] },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
      ["<", ">"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"', notIn: ["string"] },
      { open: "'", close: "'", notIn: ["string"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: "<", close: ">" },
    ],
    indentationRules: {
      increaseIndentPattern: /^.*\{[^}"']*$/,
      decreaseIndentPattern: /^(.*\*\/)?\s*\}[;\s]*$/,
    },
    onEnterRules: [
      {
        // Auto-indent inside blocks
        beforeText: /^\s*.*\{$/,
        action: { indentAction: monaco.languages.IndentAction.Indent },
      },
    ],
    wordPattern: /(-?\d*\.\d\w*)|([^\`\~\!\@\#\%\^\&\*\(\)\-\=\+\[\{\]\}\\\|\;\:\'\"\,\.\<\>\/\?\s]+)/,
  });

  monaco.languages.setMonarchTokensProvider(LANG_ID, {
    keywords: LISETTE_KEYWORDS,
    // self is a special variable, not a keyword, but highlight it like one
    selfKeywords: ["self"],
    primitiveTypes: LISETTE_PRIMITIVE_TYPES,
    compoundTypes: LISETTE_COMPOUND_TYPES,
    constructors: LISETTE_CONSTRUCTORS,

    operators: [
      "|>", "->", "=>", "?", "::", ":", "..", "..=",
      "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=",
      "==", "!=", "<", ">", "<=", ">=",
      "&&", "||", "!", "+", "-", "*", "/", "%",
      "&", "|", "^", "~", "<<", ">>",
      ".*",
    ],

    symbols: /[=><!~?:|&+\-*\/\\^%.]+/,

    escapes: /\\(?:[nrtf\\"'0]|x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f]+\})/,

    tokenizer: {
      root: [
        // Format strings f"..."
        [/f"/, "string", "@fstring"],

        // Doc comments
        [/\/\/\/.*$/, "comment.doc"],
        // Line comments
        [/\/\/.*$/, "comment"],
        // Block comments
        [/\/\*/, "comment", "@blockComment"],

        // Attributes
        [/#\[/, "attribute", "@attribute"],

        // @rawgo directive ([@] avoids Monarch treating @ as an attribute reference)
        [/[@]rawgo\b/, "keyword.other"],

        // Keywords and identifiers
        [
          /[a-z_][a-z0-9_]*/,
          {
            cases: {
              "@keywords": "keyword",
              "@selfKeywords": "variable.language",
              "@constructors": "constant",
              "@default": "identifier",
            },
          },
        ],

        // Type names (PascalCase)
        [
          /[A-Z][a-zA-Z0-9_]*/,
          {
            cases: {
              "@compoundTypes": "type",
              "@default": "type.identifier",
            },
          },
        ],

        // Primitive types (lowercase)
        [
          /\b(int8?|int16|int32|int64|uint8?|uint16|uint32|uint64|uintptr|byte|rune|float32|float64|complex64|complex128|bool|string|error)\b/,
          "type.primitive",
        ],

        // Whitespace
        [/\s+/, "white"],

        // Delimiters and brackets
        [/[{}()\[\]]/, "@brackets"],

        // Pipe operator |> (before | alone)
        [/\|>/, "keyword.operator.pipe"],
        // Arrow operators
        [/->/, "keyword.operator"],
        [/=>/, "keyword.operator"],
        // Try/propagate operator
        [/\?/, "keyword.operator"],
        // Range operators
        [/\.\.=/, "operator"],
        [/\.\./, "operator"],
        // Dereference
        [/\.\*/, "operator"],

        // Generic operators
        [
          /@symbols/,
          {
            cases: {
              "@operators": "operator",
              "@default": "",
            },
          },
        ],

        // Numbers
        [/\d*\.\d+([eE][+-]?\d+)?/, "number.float"],
        [/\d+[eE][+-]?\d+/, "number.float"],
        [/0[xX][0-9a-fA-F_]+/, "number.hex"],
        [/0[oO][0-7_]+/, "number.octal"],
        [/0[bB][01_]+/, "number.binary"],
        [/\d[\d_]*[i]/, "number.imaginary"],
        [/\d[\d_]*/, "number"],

        // Strings
        [/"([^"\\]|\\.)*$/, "string.invalid"],
        [/"/, "string", "@string_double"],

        // Character literals
        [/'[^'\\]'/, "string.char"],
        [/'@escapes'/, "string.escape"],
        [/'/, "string.invalid"],

        // Delimiter
        [/[;,]/, "delimiter"],
        [/\./, "delimiter.dot"],
      ],

      fstring: [
        [/[^"\\{]+/, "string"],
        [/@escapes/, "string.escape"],
        [/\{[^}]*\}/, "string.interpolation"],
        [/\\./, "string.escape.invalid"],
        [/"/, "string", "@pop"],
      ],

      string_double: [
        [/[^\\"]+/, "string"],
        [/@escapes/, "string.escape"],
        [/\\./, "string.escape.invalid"],
        [/"/, "string", "@pop"],
      ],

      blockComment: [
        [/[^/*]+/, "comment"],
        [/\/\*/, "comment", "@push"],
        [/\*\//, "comment", "@pop"],
        [/[/*]/, "comment"],
      ],

      attribute: [
        [/[^\]]+/, "attribute"],
        [/\]/, "attribute", "@pop"],
      ],
    },
  });
}

// ─── Snippet definitions ──────────────────────────────────────────────────────
interface SnippetDef {
  label: string;
  insertText: string;
  documentation: string;
}

export const LISETTE_SNIPPETS: SnippetDef[] = [
  {
    label: "fn",
    insertText: "fn ${1:name}(${2:params}) -> ${3:ReturnType} {\n\t$0\n}",
    documentation: "Function definition",
  },
  {
    label: "fn_void",
    insertText: "fn ${1:name}(${2:params}) {\n\t$0\n}",
    documentation: "Function with no return value",
  },
  {
    label: "fn_main",
    insertText: "fn main() {\n\t$0\n}",
    documentation: "Main function",
  },
  {
    label: "match",
    insertText: "match ${1:expr} {\n\t${2:pattern} => $0,\n}",
    documentation: "Match expression",
  },
  {
    label: "if",
    insertText: "if ${1:condition} {\n\t$0\n}",
    documentation: "If expression",
  },
  {
    label: "if_else",
    insertText: "if ${1:condition} {\n\t$0\n} else {\n\t\n}",
    documentation: "If-else expression",
  },
  {
    label: "if_let",
    insertText: "if let ${1:Some(x)} = ${2:expr} {\n\t$0\n}",
    documentation: "If-let pattern match",
  },
  {
    label: "while_let",
    insertText: "while let ${1:Some(x)} = ${2:iter} {\n\t$0\n}",
    documentation: "While-let pattern match",
  },
  {
    label: "for",
    insertText: "for ${1:item} in ${2:collection} {\n\t$0\n}",
    documentation: "For-in loop",
  },
  {
    label: "loop",
    insertText: "loop {\n\t$0\n}",
    documentation: "Infinite loop",
  },
  {
    label: "while",
    insertText: "while ${1:condition} {\n\t$0\n}",
    documentation: "While loop",
  },
  {
    label: "let",
    insertText: "let ${1:name} = $0",
    documentation: "Immutable binding",
  },
  {
    label: "let_mut",
    insertText: "let mut ${1:name} = $0",
    documentation: "Mutable binding",
  },
  {
    label: "struct",
    insertText: "struct ${1:Name} {\n\t${2:field}: ${3:Type},\n}",
    documentation: "Struct definition",
  },
  {
    label: "enum",
    insertText: "enum ${1:Name} {\n\t${2:Variant},\n}",
    documentation: "Enum definition",
  },
  {
    label: "impl",
    insertText: "impl ${1:Type} {\n\tfn ${2:method}(self) {\n\t\t$0\n\t}\n}",
    documentation: "Impl block",
  },
  {
    label: "interface",
    insertText: "interface ${1:Name} {\n\tfn ${2:method}(self) -> ${3:ReturnType}\n}",
    documentation: "Interface definition",
  },
  {
    label: "try",
    insertText: "try {\n\t$0\n}",
    documentation: "Try block for error handling",
  },
  {
    label: "task",
    insertText: "task ${1:function_call}()",
    documentation: "Spawn a goroutine",
  },
  {
    label: "import",
    insertText: 'import "${1:go:fmt}"',
    documentation: "Import a Go package",
  },
  {
    label: "var",
    insertText: "var ${1:name}: ${2:Type}",
    documentation: "Variable declaration (var)",
  },
  {
    label: "lambda",
    insertText: "|${1:x}: ${2:int}| $0",
    documentation: "Lambda expression",
  },
  {
    label: "option_match",
    insertText:
      "match ${1:opt} {\n\tSome(${2:v}) => $0,\n\tNone => ,\n}",
    documentation: "Match on Option",
  },
  {
    label: "result_match",
    insertText:
      "match ${1:res} {\n\tOk(${2:v}) => $0,\n\tErr(${3:e}) => ,\n}",
    documentation: "Match on Result",
  },
];

// ─── Completion provider ──────────────────────────────────────────────────────
export function registerCompletionProvider(
  monaco: typeof Monaco,
  getWasmCompletions: (
    model: Monaco.editor.ITextModel,
    position: Monaco.Position
  ) => Promise<Monaco.languages.CompletionItem[]>
): Monaco.IDisposable {
  return monaco.languages.registerCompletionItemProvider(LANG_ID, {
    triggerCharacters: [".", ":", "(", " ", "|"],
    async provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const range: Monaco.IRange = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };

      const keywordItems: Monaco.languages.CompletionItem[] = LISETTE_KEYWORDS.map(
        (kw) => ({
          label: kw,
          kind: monaco.languages.CompletionItemKind.Keyword,
          insertText: kw,
          sortText: `z_${kw}`, // sort after semantic completions
          range,
        })
      );

      const typeItems: Monaco.languages.CompletionItem[] = ALL_TYPES.map(
        (t) => ({
          label: t,
          kind: monaco.languages.CompletionItemKind.Class,
          insertText: t,
          sortText: `y_${t}`,
          range,
        })
      );

      const constructorItems: Monaco.languages.CompletionItem[] =
        LISETTE_CONSTRUCTORS.map((c) => ({
          label: c,
          kind: monaco.languages.CompletionItemKind.EnumMember,
          insertText: c,
          sortText: `y_${c}`,
          range,
        }));

      const snippetItems: Monaco.languages.CompletionItem[] =
        LISETTE_SNIPPETS.map((s) => ({
          label: s.label,
          kind: monaco.languages.CompletionItemKind.Snippet,
          insertText: s.insertText,
          insertTextRules:
            monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
          documentation: s.documentation,
          sortText: `z_snippet_${s.label}`,
          range,
        }));

      let wasmItems: Monaco.languages.CompletionItem[] = [];
      try {
        wasmItems = await getWasmCompletions(model, position);
      } catch {
        // WASM not ready – fall back to static completions only
      }

      return {
        suggestions: [
          ...wasmItems,
          ...keywordItems,
          ...typeItems,
          ...constructorItems,
          ...snippetItems,
        ],
      };
    },
  });
}

// ─── Hover provider ───────────────────────────────────────────────────────────
export function registerHoverProvider(
  monaco: typeof Monaco,
  getHover: (
    model: Monaco.editor.ITextModel,
    position: Monaco.Position
  ) => Promise<Monaco.languages.Hover | null>
): Monaco.IDisposable {
  return monaco.languages.registerHoverProvider(LANG_ID, {
    async provideHover(model, position) {
      try {
        return await getHover(model, position);
      } catch {
        return null;
      }
    },
  });
}

// ─── Definition provider ─────────────────────────────────────────────────────
export function registerDefinitionProvider(
  monaco: typeof Monaco,
  getDefinition: (
    model: Monaco.editor.ITextModel,
    position: Monaco.Position
  ) => Promise<Monaco.languages.Definition | null>
): Monaco.IDisposable {
  return monaco.languages.registerDefinitionProvider(LANG_ID, {
    async provideDefinition(model, position) {
      try {
        return await getDefinition(model, position);
      } catch {
        return null;
      }
    },
  });
}

// ─── Signature help provider ─────────────────────────────────────────────────
export function registerSignatureHelpProvider(
  monaco: typeof Monaco,
  getSignatureHelp: (
    model: Monaco.editor.ITextModel,
    position: Monaco.Position
  ) => Promise<Monaco.languages.SignatureHelpResult | null>
): Monaco.IDisposable {
  return monaco.languages.registerSignatureHelpProvider(LANG_ID, {
    signatureHelpTriggerCharacters: ["(", ","],
    signatureHelpRetriggerCharacters: [","],
    async provideSignatureHelp(model, position) {
      try {
        return await getSignatureHelp(model, position);
      } catch {
        return null;
      }
    },
  });
}

// ─── Format provider ──────────────────────────────────────────────────────────
export function registerFormatProvider(
  monaco: typeof Monaco,
  doFormat: (code: string) => Promise<string | null>
): Monaco.IDisposable {
  return monaco.languages.registerDocumentFormattingEditProvider(LANG_ID, {
    async provideDocumentFormattingEdits(model) {
      const formatted = await doFormat(model.getValue());
      if (!formatted) return [];
      return [
        {
          range: model.getFullModelRange(),
          text: formatted,
        },
      ];
    },
  });
}
