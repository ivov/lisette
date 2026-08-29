/**
 * Gives each release's `### Features` heading an id carrying its version, instead of
 * `#features-3`. A rehype plugin because Astro registers these before its heading
 * collector, which then keeps this id rather than assigning a slug.
 */
import Slugger from "github-slugger";

/** Concatenates the text of a heading the way Astro's collector does, to derive the id. */
const headingText = (node) => {
  let text = "";
  const walk = (current) => {
    if (current.type === "text") text += current.value;
    for (const child of current.children ?? []) walk(child);
  };
  walk(node);
  return text;
};

export const releaseAnchors = () => (tree, file) => {
  if (!file.history[0]?.endsWith("changelog.md")) return;

  const slugger = new Slugger();
  let release = "";

  for (const node of tree.children) {
    if (node.type !== "element") continue;
    if (node.tagName === "h2") {
      release = slugger.slug(headingText(node));
      continue;
    }
    if (node.tagName === "h3" && release) {
      node.properties = node.properties ?? {};
      node.properties.id = `${release}-${headingText(node).toLowerCase()}`;
    }
  }
};
