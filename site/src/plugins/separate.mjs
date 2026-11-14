/** Marks a block with `separate` in its meta, so `custom.css` does not weld it to the block above. */
export const separate = () => ({
  name: "separate",
  hooks: {
    postprocessRenderedBlock: ({ codeBlock, renderData }) => {
      if (!codeBlock.metaOptions.getBoolean("separate")) return;
      renderData.blockAst.properties["data-separate"] = "";
    },
  },
});
