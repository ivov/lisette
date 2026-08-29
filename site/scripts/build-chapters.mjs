/**
 * Copies the reference chapters from `crates/cli/reference/` into the docs collection.
 *
 * They live in the crate because `reference.rs` embeds them with `include_str!`, and cargo
 * packages only what sits inside the crate directory.
 */
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = dirname(dirname(fileURLToPath(import.meta.url)));
const SOURCE = join(SITE, "..", "crates", "cli", "reference");
const TARGET = join(SITE, "src", "content", "docs");

const chapters = readdirSync(SOURCE).filter((name) => name.endsWith(".md"));

if (chapters.length === 0) {
  throw new Error(`No chapters found in ${SOURCE}`);
}

mkdirSync(TARGET, { recursive: true });
for (const name of chapters) {
  writeFileSync(join(TARGET, name), readFileSync(join(SOURCE, name), "utf8"));
}

console.log(`chapters: wrote ${chapters.length} chapters to src/content/docs/`);
