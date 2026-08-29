/**
 * Copies `CHANGELOG.md` into the docs collection. Copied rather than imported as a
 * component, which is what lets Starlight build a table of contents from the headings.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = dirname(dirname(fileURLToPath(import.meta.url)));
const SOURCE = join(SITE, "..", "CHANGELOG.md");
const TARGET = join(SITE, "src", "content", "docs", "changelog.md");

const source = readFileSync(SOURCE, "utf8");

const lines = source.split("\n");
const stripped = (lines[0].trim() === "# Changelog" ? lines.slice(1) : lines).join("\n");

// git-cliff writes a release heading as `## [0.11.2](compare) - 2026-08-09`.
const body = stripped.replace(
  /^## \[([^\]]+)\]\(([^)]+)\) - (\d{4}-\d{2}-\d{2})$/gm,
  '## [v$1]($2) <span class="release-date">($3)</span>',
);

const page = `---
title: "Changelog"
description: "Release history for the Lisette compiler"
pagefind: false
---

${body.trimStart()}`;

mkdirSync(dirname(TARGET), { recursive: true });
writeFileSync(TARGET, page);

const releases = (body.match(/^## /gm) ?? []).length;
console.log(`changelog: wrote ${releases} releases to src/content/docs/changelog.md`);
