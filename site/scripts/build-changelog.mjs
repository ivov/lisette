/**
 * Copies `CHANGELOG.md` into the docs collection. Copied rather than imported as a
 * component, which is what lets Starlight build a table of contents from the headings.
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RECENT_RELEASES = 10;

const SITE = dirname(dirname(fileURLToPath(import.meta.url)));
const SOURCE = join(SITE, "..", "CHANGELOG.md");
const TARGET = join(SITE, "src", "content", "docs", "changelog.md");
const OLDER_TARGET = join(SITE, "src", "content", "changelog-older", "changelog.md");

const source = readFileSync(SOURCE, "utf8");

const lines = source.split("\n");
const stripped = (lines[0].trim() === "# Changelog" ? lines.slice(1) : lines).join("\n");

// git-cliff writes a release heading as `## [0.11.2](compare) - 2026-08-09`.
const body = stripped.replace(
  /^## \[([^\]]+)\]\(([^)]+)\) - (\d{4}-\d{2}-\d{2})$/gm,
  '## [v$1]($2) <span class="release-date">($3)</span>',
);

const [intro, ...releases] = body.split(/^(?=## )/m);
const recent = releases.slice(0, RECENT_RELEASES);
const older = releases.slice(RECENT_RELEASES);

const olderHolder =
  older.length > 0 ? `\n<div id="changelog-older" data-src="/docs/changelog/older/"></div>\n` : "";

const page = `---
title: "Changelog"
description: "Release history for the Lisette compiler"
pagefind: false
---

${(intro + recent.join("")).trimStart()}${olderHolder}`;

mkdirSync(dirname(TARGET), { recursive: true });
writeFileSync(TARGET, page);
mkdirSync(dirname(OLDER_TARGET), { recursive: true });
writeFileSync(OLDER_TARGET, older.join(""));

console.log(
  `changelog: wrote ${recent.length} releases to src/content/docs/changelog.md and ${older.length} to src/content/changelog-older/changelog.md`,
);
