/**
 * A Shiki theme that paints Lisette from the landing page's own tokens. Shiki copies a
 * theme color into the style attribute unread, so naming the `--syn-*` variables here
 * leaves `landing.css` the one place any of them is written.
 */

const KEYWORD = "var(--syn-keyword)";
const PLAIN = "var(--syn-plain)";
const STRING = "var(--syn-string)";

const rule = (foreground, ...scope) => ({ scope, settings: { foreground } });

export const landingTheme = {
  name: "lisette-landing",
  type: "dark",
  colors: {
    "editor.background": "var(--bg-panel)",
    "editor.foreground": PLAIN,
  },
  settings: [
    { settings: { foreground: PLAIN, background: "var(--bg-panel)" } },

    rule(
      KEYWORD,
      "keyword.control",
      "keyword.other",
      "storage.type",
      "storage.modifier",
      "keyword.operator.as",
      "variable.language.self",
    ),

    rule(
      KEYWORD,
      "keyword.operator.arithmetic",
      "keyword.operator.assignment",
      "keyword.operator.bitwise",
      "keyword.operator.not",
    ),
    rule(
      "var(--syn-op)",
      "keyword.operator.arrow",
      "keyword.operator.propagation",
      "keyword.operator.range",
      "variable.language.wildcard",
    ),

    // The grammar scopes the angle brackets of a generic as comparison operators, so `Result<Ref<File>, error>` comes out striped without this.
    rule(PLAIN, "keyword.operator.comparison"),

    rule(
      "var(--syn-type)",
      "entity.name.type",
      "support.type.primitive",
      "constant.language",
    ),
    rule("var(--syn-fn)", "entity.name.function"),
    rule("var(--syn-num)", "constant.numeric"),

    // Stated after `constant.language` so it wins the tie.
    rule(KEYWORD, "constant.language.boolean"),

    rule(
      STRING,
      "string.quoted",
      "string.interpolated",
      "punctuation.definition.string",
      "constant.character",
    ),

    rule(PLAIN, "meta.interpolation", "storage.type.string"),
    rule("var(--syn-lit)", "punctuation.section.interpolation"),

    rule("var(--syn-comment)", "comment"),
    rule("var(--syn-punct)", "punctuation.separator"),

    rule(
      "var(--syn-attr)",
      "meta.annotation",
      "punctuation.definition.annotation",
      "entity.name.tag.annotation",
    ),
    rule(STRING, "meta.annotation string.quoted", "meta.annotation punctuation.definition.string"),
  ],
};
