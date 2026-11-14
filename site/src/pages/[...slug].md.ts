import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection, getEntry } from "astro:content";

/** Every page again as raw markdown at `<page>.md`, for readers and coding agents. */
export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  const olderReleases = (await getEntry("changelogOlder", "changelog"))?.body ?? "";
  return docs.map((entry) => ({
    params: { slug: entry.id },
    props: {
      title: entry.data.title,
      body: entry.body?.replace(/^<div id="changelog-older".*$/m, () => olderReleases),
    },
  }));
};

export const GET: APIRoute = ({ props }) =>
  new Response(`# ${props.title}\n\n${props.body}`, {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
