import type * as Monaco from "monaco-editor";

export const THEME = "lisette-dark";

// Night Owl as Starlight bundles it, with the overrides from `site/ec.config.mjs`.
const PLAIN = "d6deeb";
const KEYWORD = "c792ea";
const OPERATOR = "7fdbca";
const TYPE = "c5e478";
const STRING = "34d399";
const FUNCTION = "82aaff";
const NUMBER = "f78c6c";
const COMMENT = "637777";
const ATTRIBUTE = "8b9ba8";
const LITERAL = "ff5874";

// Monaco's Go tokenizer emits the builtin types as `keyword.<name>`.
const GO_TYPES = [
  "bool", "byte", "complex64", "complex128", "error", "float32", "float64",
  "int", "int8", "int16", "int32", "int64", "rune", "string",
  "uint", "uint8", "uint16", "uint32", "uint64", "uintptr",
];

const rules = (foreground: string, tokens: string[]): Monaco.editor.ITokenThemeRule[] =>
  tokens.map((token) => ({ token, foreground }));

export function registerTheme(monaco: typeof Monaco): void {
  monaco.editor.defineTheme(THEME, {
    base: "vs-dark",
    inherit: false,
    rules: [
      { token: "", foreground: PLAIN, background: "181825" },
      ...rules(COMMENT, ["comment"]),
      ...rules(STRING, ["string", "constant.character", "punctuation.definition.string"]),
      ...rules(NUMBER, ["constant.numeric", "constant.character.escape", "number", "string.escape"]),
      ...rules(LITERAL, ["punctuation.section.interpolation", "string.invalid", "keyword.nil"]),
      ...rules(PLAIN, [
        "meta.interpolation",
        "storage.type.string",
        "string.interpolation",
        "keyword.operator.comparison",
        "identifier",
        "delimiter",
        "punctuation",
      ]),
      ...rules(KEYWORD, [
        "keyword",
        "storage",
        "constant.language.boolean",
        "keyword.operator.as",
        "keyword.operator.arithmetic",
        "keyword.operator.assignment",
        "keyword.operator.bitwise",
        "keyword.operator.logical",
        "variable.language.self",
      ]),
      ...rules(OPERATOR, ["keyword.operator", "operator", "variable.language"]),
      ...rules(TYPE, [
        "entity.name.type",
        "support.type",
        "constant.language",
        "type",
        "constant",
        ...GO_TYPES.map((name) => `keyword.${name}`),
      ]),
      ...rules(FUNCTION, ["entity.name.function", "support.function", "function"]),
      ...rules(ATTRIBUTE, [
        "meta.annotation",
        "punctuation.definition.annotation",
        "entity.name.tag.annotation",
        "attribute",
      ]),
    ],
    colors: {
      "editor.background": "#181825",
      "editor.foreground": "#d6deeb",
      "editor.lineHighlightBackground": "#1e1e2e",
      "editor.lineHighlightBorder": "#1e1e2e",
      "editor.selectionBackground": "#585b7080",
      "editor.inactiveSelectionBackground": "#585b7040",
      "editorCursor.foreground": "#b294f0",
      "editorLineNumber.foreground": "#6c7086",
      "editorLineNumber.activeForeground": "#a6adc8",
      "editorIndentGuide.background1": "#313244",
      "editorIndentGuide.activeBackground1": "#45475a",
      "editorBracketMatch.background": "#31324480",
      "editorBracketMatch.border": "#45475a",
      "editorWidget.background": "#1e1e2e",
      "editorWidget.border": "#313244",
      "editorSuggestWidget.background": "#1e1e2e",
      "editorSuggestWidget.border": "#313244",
      "editorSuggestWidget.selectedBackground": "#313244",
      "editorSuggestWidget.selectedForeground": "#cdd6f4",
      "editorSuggestWidget.highlightForeground": "#b294f0",
      "editorSuggestWidget.focusHighlightForeground": "#b294f0",
      "editorHoverWidget.background": "#1e1e2e",
      "editorHoverWidget.border": "#313244",
      "editorError.foreground": "#f87171",
      "editorWarning.foreground": "#fbbf24",
      "editorInfo.foreground": "#60a5fa",
      "scrollbar.shadow": "#00000000",
      "scrollbarSlider.background": "#ffffff17",
      "scrollbarSlider.hoverBackground": "#ffffff40",
      "scrollbarSlider.activeBackground": "#ffffff40",
      "input.background": "#181825",
      "input.border": "#313244",
      "focusBorder": "#b294f0",
    },
  });
}
