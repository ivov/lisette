import { defineCollection } from "astro:content";
import { docsLoader, i18nLoader } from "@astrojs/starlight/loaders";
import { docsSchema, i18nSchema } from "@astrojs/starlight/schema";
import { topicSchema } from "starlight-sidebar-topics/schema";

// Every doc is addressed under `docs/`, leaving the root free for the landing page. Astro's `base` would prefix every route, including that landing page, so the prefix lives on the ids.
const withDocsPrefix = ({ entry }: { entry: string }) => `docs/${entry.replace(/\.mdx?$/, "")}`;

export const collections = {
  docs: defineCollection({
    loader: docsLoader({ generateId: withDocsPrefix }),
    schema: docsSchema({ extend: topicSchema }),
  }),
  // Carries the one string overridden in `src/content/i18n/en.json`.
  i18n: defineCollection({ loader: i18nLoader(), schema: i18nSchema() }),
};
