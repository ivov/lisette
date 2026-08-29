/**
 * Keeps code blocks out of the search index, which they made up half of. Set on the `<pre>`
 * rather than the frame, so a block's title in the caption stays searchable.
 */
import { select } from "expressive-code/hast";

export const searchIgnore = () => ({
  name: "search-ignore",
  hooks: {
    postprocessRenderedBlock: ({ renderData }) => {
      const code = select("pre", renderData.blockAst);
      if (code) code.properties.dataPagefindIgnore = "";
    },
  },
});
