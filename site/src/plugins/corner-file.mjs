/**
 * Puts a filename in a block's top-right corner. `file="strconv.go"` in a block's meta
 * turns it on. Expressive Code's own `title` draws a header bar, which splits a welded pair.
 */
import { h } from "@expressive-code/core/hast";

const styles = `
  .expressive-code .frame:has(> .corner-file) {
    position: relative;
  }
  .corner-file {
    position: absolute;
    inset-block-start: 0.55rem;
    inset-inline-end: 0.9rem;
    z-index: 1;
    pointer-events: none;
    font-family: var(--__sl-font-mono);
    font-size: var(--sl-text-xs);
    color: var(--sl-color-gray-3);
  }
`;

export const cornerFile = () => ({
  name: "corner-file",
  hooks: {
    postprocessRenderedBlock: ({ codeBlock, renderData, addStyles }) => {
      const name = codeBlock.metaOptions.getString("file");
      if (!name) return;
      addStyles(styles);
      renderData.blockAst.children.push(h("div", { class: "corner-file" }, name));
    },
  },
});
