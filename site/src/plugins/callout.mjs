/**
 * Turns a `// !callout[/target/] text` marker line into a box under the token it names.
 *
 * The bracketed pattern is optional. `-right` puts the box after the code on the line,
 * and consecutive `-right` callouts align to one column. `-above` puts it before the line.
 * `-ok` and `-error` tint it green and red. `+8` pads by eight characters and overrides the
 * shared column. Backticks in the text render as inline code.
 */
import { AttachedPluginData, definePlugin } from "@expressive-code/core";
import { h, select } from "@expressive-code/core/hast";

const MARKER =
  /^(\s*)(?:\/\/|#)\s*!callout(?:-(ok|error))?(-right)?(?:\+(\d+))?(-above)?(-center)?(?:\[\/(.+?)\/\])?(?:\s+(.+?))?\s*$/;

const calloutData = new AttachedPluginData(() => ({ callouts: [] }));

const NUMBERS = /^-?\d[\d_]*(\.\d+)?(\s*,\s*-?\d[\d_]*(\.\d+)?)*$/;

const TAG_OPTIONS = new Set(["omitempty", "!omitempty", "skip", "snake_case", "camel_case"]);

const KEYWORDS = new Set([
  ..."if else match while for return break continue loop in".split(" "),
  ..."let mut import defer task select try recover assert".split(" "),
  "as",
  "self",
]);

const literalClass = (code) => {
  if (/^(["']).*\1$/s.test(code)) return "callout-string";
  if (/^#\[.*\]$/s.test(code) || TAG_OPTIONS.has(code)) return "callout-attribute";
  if (KEYWORDS.has(code)) return "callout-keyword";
  if (NUMBERS.test(code)) return "callout-value";
  if (/^\w*\s*\{.*\}$/s.test(code)) return "callout-value";
  if (/^[A-Z]\w*\.$/.test(code)) return "callout-type";
  return null;
};

// A number must stand alone, or the digits ending a name such as `int64` would paint as a literal.
const NESTED = /("[^"]*"|'[^']*'|(?<![\w.])-?\d[\d_]*(?:\.\d+)?(?!\w))/;

const codeNodes = (part) => {
  const kind = literalClass(part);
  if (kind) return [h("code", { class: kind }, part)];
  const pieces = part.split(NESTED).filter((piece) => piece !== "");
  if (pieces.length < 2) return [h("code", {}, part)];
  return pieces.map((piece) => {
    const nested = literalClass(piece);
    return h("code", nested ? { class: nested } : {}, piece);
  });
};

const inlineCode = (text) =>
  text
    .split(/`([^`]+)`/)
    .flatMap((part, index) => (index % 2 === 0 ? [part] : codeNodes(part)))
    .filter((part) => part !== "");

const breakLines = (node) => {
  if (typeof node === "string") {
    return node
      .split("\\n")
      .flatMap((line, index) => (index ? [h("br"), line] : [line]))
      .filter((part) => part !== "");
  }
  const text = node.children?.[0]?.value ?? "";
  if (!text.includes("\\n")) return [node];
  return text
    .split("\\n")
    .flatMap((line, index) => (index ? [h("br"), h("code", node.properties, line)] : [h("code", node.properties, line)]));
};

const renderLabel = (text) => inlineCode(text).flatMap(breakLines);

// Offsets divide by the scale factor because `ch` is measured at the callout's smaller font size.
const stackedStyle = (callout) => {
  const parts = [`--callout-column: ${callout.column}`];
  if (callout.span !== undefined) {
    parts.push(`--callout-span: ${callout.span}`);
    parts.push(
      "--callout-notch-start: calc(" +
        `${callout.span} * 0.5ch / var(--callout-scale) - var(--callout-notch) / 2)`,
    );
  }
  return parts.join(";");
};

// Consecutive inline callouts share a column, each padded to the longest line in the run. A blank line or an attribute does not end a run; anything else does.
const bridges = (text) => text.trim() === "" || text.trim().startsWith("#[");

const alignRuns = (callouts, lines) => {
  const inline = callouts.filter((entry) => entry.right);
  let run = [];
  const settle = () => {
    if (run.length > 1) {
      const widest = Math.max(...run.map((entry) => entry.width));
      for (const entry of run) entry.pad = widest - entry.width;
    }
    run = [];
  };
  for (const entry of inline) {
    const previous = run.at(-1);
    const joined =
      previous &&
      lines.slice(previous.lineIndex + 1, entry.lineIndex).every((text) => bridges(text));
    if (previous && !joined) settle();
    run.push(entry);
  }
  settle();
};

const rightStyle = (callout) => {
  const pad = callout.offset ?? callout.pad;
  return pad ? `--callout-pad: ${pad}` : undefined;
};

const box = (callout) =>
  h(
    "span",
    {
      class: [
        "callout",
        callout.tone && `callout-${callout.tone}`,
        callout.right && "callout-right",
        callout.above && "callout-above",
        callout.center && "callout-center",
      ]
        .filter(Boolean)
        .join(" "),
      style: callout.right ? rightStyle(callout) : stackedStyle(callout),
    },
    renderLabel(callout.label),
  );

// Each marker line is deleted, so later callouts shift up by the number already removed above.
const collect = (texts) => {
  const callouts = [];
  const markerIndices = [];
  for (const [index, text] of texts.entries()) {
    const match = text.match(MARKER);
    if (!match) continue;
    const [, indent, tone, right, offset, above, center, pattern, remark] = match;
    // Skip past markers stacked below this one, so several markers can name one line.
    let at = index + 1;
    while (texts[at] !== undefined && MARKER.test(texts[at])) at += 1;
    const target = texts[at];
    if (target === undefined) continue;
    const label = remark ?? tone;
    if (!label) continue;
    let column = indent.length;
    let span;
    if (pattern) {
      const found = target.match(new RegExp(pattern));
      if (found && found.index >= 0) {
        column = found.index;
        span = found[0].length;
      }
    } else {
      column = target.length - target.trimStart().length;
    }
    markerIndices.push(index);
    callouts.push({
      lineIndex: index + 1 - markerIndices.length,
      column,
      span,
      label,
      tone,
      right: Boolean(right),
      above: Boolean(above),
      center: Boolean(center),
      offset: offset ? Number(offset) : undefined,
      width: target.length,
    });
  }
  const kept = texts.filter((_, index) => !markerIndices.includes(index));
  alignRuns(callouts, kept);
  return { callouts, markerIndices };
};

const styles = `
  .ec-line.callout-line .code {
    padding-block: 0.35rem;
  }
  .callout {
    --callout-notch: 0.5rem;
    --callout-line: var(--sl-color-accent);
    --callout-fill: var(--sl-color-black);
    --callout-scale: 0.85;
    position: relative;
    display: inline-block;
    margin-inline-start: calc(var(--callout-column) * 1ch / var(--callout-scale));
    padding: 0.15rem 0.6rem;
    border: 1px solid var(--callout-line);
    border-radius: 0.2rem;
    background: var(--callout-fill);
    color: var(--sl-color-white);
    font-size: calc(var(--callout-scale) * 1em);
    line-height: 1.4;
    white-space: normal;
  }
  .callout-ok {
    --callout-line: var(--sl-color-green);
    --callout-fill: var(--sl-color-green-low);
  }
  .callout-error {
    --callout-line: #ff5874;
    --callout-fill: color-mix(in srgb, #ff5874 18%, var(--sl-color-black));
  }
  .callout::before {
    content: "";
    position: absolute;
    inset-block-start: calc(var(--callout-notch) / -2 - 0.5px);
    inset-inline-start: min(
      var(--callout-notch-start, 0.7rem),
      100% - var(--callout-notch) - 0.7rem
    );
    width: var(--callout-notch);
    height: var(--callout-notch);
    border-block-start: 1px solid var(--callout-line);
    border-inline-start: 1px solid var(--callout-line);
    background: var(--callout-fill);
    transform: rotate(45deg);
  }
  .callout-above::before {
    inset-block-start: auto;
    inset-block-end: calc(var(--callout-notch) / -2 - 0.5px);
    border-block-start: none;
    border-inline-start: none;
    border-block-end: 1px solid var(--callout-line);
    border-inline-end: 1px solid var(--callout-line);
  }
  .callout-center {
    margin-inline-start: calc(
      (var(--callout-column) + var(--callout-span, 0) * 0.5) * 1ch / var(--callout-scale)
    );
    transform: translateX(-50%);
  }
  .callout-center::before {
    inset-inline-start: calc(50% - var(--callout-notch) / 2);
  }
  .callout code {
    background: none;
    border: none;
    padding: 0;
    font-size: 1em;
    color: var(--sl-color-accent-high);
  }
  .callout code.callout-string {
    color: #34d399;
  }
  .callout code.callout-value {
    color: #f78c6c;
  }
  .callout code.callout-attribute {
    color: #8b9ba8;
  }
  .callout code.callout-type {
    color: #c5e478;
  }
  .callout code.callout-keyword {
    color: #c792ea;
  }
  .callout-right {
    /* Whole pixels, so halving for the triangle cannot round unevenly and skew the tip. */
    --callout-height: 18px;
    --callout-arrow: 7px;
    display: inline-flex;
    align-items: center;
    height: var(--callout-height);
    margin-inline-start: calc(
      var(--callout-pad, 0) * 1ch / var(--callout-scale) + 2ch + var(--callout-arrow)
    );
    padding-block: 0;
    white-space: pre;
    border-inline-start: none;
    border-start-start-radius: 0;
    border-end-start-radius: 0;
  }
  .callout-right::before,
  .callout-right::after {
    content: "";
    position: absolute;
    width: 0;
    height: 0;
    background: none;
    transform: none;
    inset-block-start: -1px;
    inset-inline-start: calc(var(--callout-arrow) * -1);
    border: 0 solid transparent;
    border-block-width: calc(var(--callout-height) / 2);
    border-inline-end: var(--callout-arrow) solid var(--callout-line);
  }
  .callout-right::after {
    inset-inline-start: calc(var(--callout-arrow) * -1 + 1.27px);
    border-inline-end-color: var(--callout-fill);
  }
`;

export const callout = () =>
  definePlugin({
    name: "callout",
    hooks: {
      preprocessCode: ({ codeBlock }) => {
        const { callouts, markerIndices } = collect(codeBlock.getLines().map((line) => line.text));
        if (!markerIndices.length) return;
        codeBlock.deleteLines(markerIndices);
        calloutData.setFor(codeBlock, { callouts });
      },
      postprocessRenderedLine: ({ codeBlock, lineIndex, renderData }) => {
        const { callouts } = calloutData.getOrCreateFor(codeBlock);
        const inline = callouts.find((entry) => entry.right && entry.lineIndex === lineIndex);
        if (!inline) return;
        const code = select(".code", renderData.lineAst);
        if (!code) return;
        code.children.push(box(inline));
      },
      postprocessRenderedBlock: ({ codeBlock, renderData, renderEmptyLine, addStyles }) => {
        const { callouts } = calloutData.getOrCreateFor(codeBlock);
        if (!callouts.length) return;
        addStyles(styles);
        const code = select("code", renderData.blockAst);
        if (!code) return;

        // Splice from the bottom up so the positions of the lines above stay valid.
        for (const stacked of callouts.filter((entry) => !entry.right).reverse()) {
          const positions = code.children.flatMap((child, position) =>
            child.type === "element" ? [position] : [],
          );
          const after = positions[stacked.lineIndex];
          if (after === undefined) continue;
          const { lineAst, codeWrapper } = renderEmptyLine();
          lineAst.properties.className = [lineAst.properties.className ?? []]
            .flat()
            .concat("callout-line");
          codeWrapper.children.push(box(stacked));
          // An `-above` box takes the marker's own place, so it sits before the line it names.
          code.children.splice(stacked.above ? after : after + 1, 0, lineAst);
        }
      },
    },
  });
