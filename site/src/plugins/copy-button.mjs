/**
 * Keeps the copy button only on blocks worth pasting, which is commands to run. `copy` in
 * a block's meta forces it on, `nocopy` forces it off.
 */
const COPYABLE = new Set(["sh", "bash", "shell", "shellscript", "zsh", "console"]);

const isCopyButton = (node) =>
  node.type === "element" && [node.properties?.className ?? []].flat().includes("copy");

const stripCopyButton = (node) => {
  if (!node.children) return;
  node.children = node.children.filter((child) => !isCopyButton(child));
  node.children.forEach(stripCopyButton);
};

export const copyButton = () => ({
  name: "copy-button",
  hooks: {
    postprocessRenderedBlock: ({ codeBlock, renderData }) => {
      const forced = codeBlock.metaOptions.getBoolean("copy");
      const suppressed = codeBlock.metaOptions.getBoolean("nocopy");
      const keep = forced ?? (!suppressed && COPYABLE.has(codeBlock.language));
      if (!keep) stripCopyButton(renderData.blockAst);
    },
  },
});
