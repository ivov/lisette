import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";

/** Every page again as raw markdown at `<page>.md`, for readers and coding agents. */
export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");
  return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

export const GET: APIRoute = ({ props }) =>
  new Response(`# ${props.entry.data.title}\n\n${props.entry.body}`, {
    headers: { "content-type": "text/markdown; charset=utf-8" },
  });
