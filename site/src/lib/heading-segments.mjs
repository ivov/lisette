/**
 * Recovers the inline code spans of a heading. Astro flattens each heading to a plain
 * string before Starlight sees it, so the spans are read back from the raw markdown.
 */

const HEADING = /^(#{1,6})[ \t]+(.+?)[ \t]*#*[ \t]*$/;
const FENCE = /^\s*(```|~~~)/;

/** Splits `a \`b\` c` into `[{ text: "a ", code: false }, { text: "b", code: true }, ...]`. */
const split = (markdown) =>
  markdown
    .split(/`([^`]+)`/)
    .map((text, index) => ({ text, code: index % 2 === 1 }))
    .filter((segment) => segment.text !== "");

/* Text is the join of the segments, exactly what Astro produced, so no slug has to be recomputed. */
export const headingSegments = (body) => {
  const segments = new Map();
  let fenced = false;
  for (const line of (body ?? "").split("\n")) {
    if (FENCE.test(line)) {
      fenced = !fenced;
      continue;
    }
    if (fenced) continue;
    const match = line.match(HEADING);
    if (!match) continue;
    const parts = split(match[2]);
    if (!parts.some((part) => part.code)) continue;
    segments.set(parts.map((part) => part.text).join(""), parts);
  }
  return segments;
};
