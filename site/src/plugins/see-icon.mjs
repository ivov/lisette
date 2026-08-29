/**
 * Replaces the leading emoji of a "See ..." paragraph with an icon. `📚` marks a link to
 * another chapter, `🐙` one that leaves for the repo. A rehype plugin because a `.md` page
 * cannot mount a component. The prelude generator inlines its own copy.
 */
const icon = (...paths) => ({
  type: "element",
  tagName: "svg",
  properties: {
    className: ["see-icon"],
    "aria-hidden": "true",
    viewBox: "0 0 24 24",
    fill: "currentColor",
  },
  children: paths.map((d) => ({
    type: "element",
    tagName: "path",
    properties: { d },
    children: [],
  })),
});

const BOOK = icon(
  "M21.17 2.06A13.1 13.1 0 0 0 19 1.87a12.94 12.94 0 0 0-7 2.05 12.94 12.94 0 0 0-7-2 13.1 13.1 0 0 0-2.17.19 1 1 0 0 0-.83 1v12a1 1 0 0 0 1.17 1 10.9 10.9 0 0 1 8.25 1.91l.12.07h.11a.91.91 0 0 0 .7 0h.11l.12-.07A10.899 10.899 0 0 1 20.83 16 1 1 0 0 0 22 15V3a1 1 0 0 0-.83-.94ZM11 15.35a12.87 12.87 0 0 0-6-1.48H4v-10c.333-.02.667-.02 1 0a10.86 10.86 0 0 1 6 1.8v9.68Zm9-1.44h-1a12.87 12.87 0 0 0-6 1.48V5.67a10.86 10.86 0 0 1 6-1.8c.333-.02.667-.02 1 0v10.04Zm1.17 4.15a13.098 13.098 0 0 0-2.17-.19 12.94 12.94 0 0 0-7 2.05 12.94 12.94 0 0 0-7-2.05c-.727.003-1.453.066-2.17.19A1 1 0 0 0 2 19.21a1 1 0 0 0 1.17.79 10.9 10.9 0 0 1 8.25 1.91 1 1 0 0 0 1.16 0A10.9 10.9 0 0 1 20.83 20a1 1 0 0 0 1.17-.79 1 1 0 0 0-.83-1.15Z",
);

const GITHUB = icon(
  "M12 .3a12 12 0 0 0-3.8 23.38c.6.12.83-.26.83-.57L9 21.07c-3.34.72-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.08-.74.09-.73.09-.73 1.2.09 1.83 1.24 1.83 1.24 1.08 1.83 2.81 1.3 3.5 1 .1-.78.42-1.31.76-1.61-2.67-.3-5.47-1.33-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.14-.3-.54-1.52.1-3.18 0 0 1-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.28-1.55 3.29-1.23 3.29-1.23.64 1.66.24 2.88.12 3.18a4.65 4.65 0 0 1 1.23 3.22c0 4.61-2.8 5.63-5.48 5.92.42.36.81 1.1.81 2.22l-.01 3.29c0 .31.2.69.82.57A12 12 0 0 0 12 .3Z",
);

const MARKERS = [
  ["📚 ", BOOK],
  ["🐙 ", GITHUB],
];

export const seeIcon = () => (tree) => {
  const walk = (node) => {
    for (const child of node.children ?? []) {
      const first = child.children?.[0];
      const marker =
        child.type === "element" && child.tagName === "p" && first?.type === "text"
          ? MARKERS.find(([emoji]) => first.value.startsWith(emoji))
          : undefined;
      if (marker) {
        const [emoji, node] = marker;
        first.value = first.value.slice(emoji.length);
        child.children.unshift(node);
        continue;
      }
      walk(child);
    }
  };
  walk(tree);
};
