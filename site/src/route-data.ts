import { defineRouteMiddleware } from "@astrojs/starlight/route-data";
import { getEntry, render } from "astro:content";

export const onRequest = defineRouteMiddleware(async (context) => {
  const route = context.locals.starlightRoute;
  if (route.id !== "docs/changelog" || !route.toc) return;

  const entry = await getEntry("changelogOlder", "changelog");
  if (!entry) return;
  const { headings } = await render(entry);

  const { minHeadingLevel, maxHeadingLevel } = route.toc;
  for (const heading of headings) {
    if (heading.depth < minHeadingLevel || heading.depth > maxHeadingLevel) continue;
    let list = route.toc.items;
    while (list.at(-1) && list.at(-1)!.depth < heading.depth) list = list.at(-1)!.children;
    list.push({ ...heading, children: [] });
  }
});
