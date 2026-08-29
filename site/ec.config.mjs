// Options holding functions live here, not in `astro.config.mjs`, because `<Code>` requires the integration's own options to be JSON-serializable.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { defineEcConfig } from "@astrojs/starlight/expressive-code";
import { callout } from "./src/plugins/callout.mjs";
import { copyButton } from "./src/plugins/copy-button.mjs";
import { cornerFile } from "./src/plugins/corner-file.mjs";
import { searchIgnore } from "./src/plugins/search-ignore.mjs";
import { pluginFramesTexts } from "@expressive-code/plugin-frames";
import tomlGrammar from "@shikijs/langs/toml";

// Resolved from the project root, not from this file. At build time Astro reloads this config from a copy under `dist/`, so walking up from `import.meta.url` misses the grammar and every Lisette block falls back to plain text.
const lisette = {
  ...JSON.parse(
    readFileSync(join(process.cwd(), "..", "editors/vscode/syntaxes/lisette.tmLanguage.json"), "utf8"),
  ),
  name: "lisette",
};

// Shiki's TOML grammar names the quotes of a quoted key but not the text between them, and an inner capture replaces the outer over its range.
const KEY_SCOPE = "variable.other.key.toml";

const namedQuotedKeys = (grammar) => {
  const walk = (node) => {
    if (Array.isArray(node)) return node.forEach(walk);
    if (!node || typeof node !== "object") return;
    if (node.captures?.["1"]?.name === KEY_SCOPE && node.captures["3"] && !node.captures["3"].name) {
      node.captures["3"].name = KEY_SCOPE;
    }
    Object.values(node).forEach(walk);
  };
  walk(grammar);
  return grammar;
};

const GROUPS = [
  {
    color: "#C5E478",
    scopes: ["entity.name.type", "support.type.primitive", "constant.language"],
  },
  {
    color: "#34d399",
    scopes: [
      "string.quoted",
      "string.interpolated",
      "punctuation.definition.string",
      "constant.character",
    ],
  },
];

// Ties between rules of equal specificity go to the earlier one, so the theme's own entries must give up a scope before an added rule can take it.
const claimed = new Set(GROUPS.flatMap((group) => group.scopes));

pluginFramesTexts.overrideTexts(undefined, { copyButtonCopied: "Copied" });

export default defineEcConfig({
  shiki: { langs: [lisette, ...tomlGrammar.map((g) => namedQuotedKeys(structuredClone(g)))] },
  plugins: [copyButton(), callout(), cornerFile(), searchIgnore()],
  // Off because the heuristic reads any early comment as a file name, which swallowed the license line of the `//!` example into a tab.
  frames: { extractFileNameFromCode: false },
  styleOverrides: {
    frames: {
      tooltipSuccessBackground: "var(--sl-color-accent)",
      tooltipSuccessForeground: "white",
    },
  },
  customizeTheme: (theme) => {
    theme.settings = theme.settings.flatMap((rule) => {
      const scopes = Array.isArray(rule.scope) ? rule.scope : rule.scope ? [rule.scope] : [];
      if (!scopes.length) return [rule];
      const kept = scopes.filter((scope) => !claimed.has(scope));
      if (kept.length === scopes.length) return [rule];
      return kept.length ? [{ ...rule, scope: kept }] : [];
    });
    for (const { scopes, color } of GROUPS) {
      theme.settings.push({ scope: scopes, settings: { foreground: color } });
    }

    theme.settings.push({
      scope: ["source.toml punctuation.definition.variable"],
      settings: { foreground: theme.fg },
    });

    theme.settings.push({
      scope: ["source.lisette meta.interpolation", "source.lisette storage.type.string"],
      settings: { foreground: theme.fg },
    });
    theme.settings.push({
      scope: ["source.lisette punctuation.section.interpolation"],
      settings: { foreground: "#ff5874" },
    });

    theme.settings.push({
      scope: [
        "source.lisette constant.language.boolean",
        "source.go constant.language.boolean",
      ],
      settings: { foreground: "#c792ea" },
    });

    theme.settings.push({
      scope: [
        "source.toml variable.other.key",
        "source.toml punctuation.definition.variable",
      ],
      settings: { foreground: "#C5E478" },
    });

    // The grammar scopes the angle brackets of a generic as comparison operators, so `Result<Ref<File>, error>` comes out striped without this.
    theme.settings.push({
      scope: ["source.lisette keyword.operator.comparison"],
      settings: { foreground: theme.fg },
    });

    theme.settings.push({
      scope: ["source.lisette keyword.operator.as", "source.lisette variable.language.self"],
      settings: { foreground: "#C792EA" },
    });

    theme.settings.push({
      scope: ["source.lisette support.function.builtin"],
      settings: { foreground: "#82AAFF" },
    });

    theme.settings.push({
      scope: [
        "source.lisette meta.annotation",
        "source.lisette punctuation.definition.annotation",
        "source.lisette entity.name.tag.annotation",
      ],
      settings: { foreground: "#8B9BA8" },
    });

    theme.settings.push({
      scope: [
        "source.rust meta.attribute",
        "source.rust meta.attribute entity.name.type",
        "source.rust punctuation.definition.attribute",
      ],
      settings: { foreground: "#8B9BA8" },
    });

    theme.settings.push({
      scope: [
        "source.lisette meta.annotation string.quoted",
        "source.lisette meta.annotation punctuation.definition.string",
      ],
      settings: { foreground: GROUPS[1].color },
    });
  },
});
