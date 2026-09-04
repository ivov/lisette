// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import catppuccin from "@catppuccin/starlight";
import starlightSidebarTopics from "starlight-sidebar-topics";
import { releaseAnchors } from "./src/plugins/release-anchors.mjs";
import { seeIcon } from "./src/plugins/see-icon.mjs";

export default defineConfig({
  site: "https://lisette.run",
  trailingSlash: "always",
  // `/docs` and `/quickstart` are redirected by `worker.js`, not here, so a crawler gets a real 301 rather than a meta refresh. Nothing may be built at those paths: Cloudflare serves a matching asset before the Worker runs, so a stub would win.
  markdown: { rehypePlugins: [releaseAnchors, seeIcon] },
  integrations: [
    starlight({
      title: "Lisette",
      description: "Little language inspired by Rust that compiles to Go.",
      titleDelimiter: "·",
      customCss: [
        "@fontsource-variable/inter",
        "@fontsource/lexend/latin-600.css",
        "./src/styles/custom.css",
      ],
      expressiveCode: { themes: ["starlight-dark", "starlight-light"] },
      head: [
        {
          tag: "meta",
          attrs: { property: "og:image", content: "https://lisette.run/og.png" },
        },
        { tag: "meta", attrs: { property: "og:image:width", content: "2400" } },
        { tag: "meta", attrs: { property: "og:image:height", content: "1260" } },
        {
          tag: "meta",
          attrs: {
            property: "og:image:alt",
            content: "Lisette: a love letter to Go, written in Rust",
          },
        },
      ],
      components: {
        Header: "./src/components/Header.astro",
        Pagination: "./src/components/Pagination.astro",
        Search: "./src/components/Search.astro",
        Sidebar: "./src/components/Sidebar.astro",
        SiteTitle: "./src/components/SiteTitle.astro",
        TableOfContents: "./src/components/TableOfContents.astro",
        ThemeSelect: "./src/components/ThemeSelect.astro",
        SocialIcons: "./src/components/SocialIcons.astro",
      },
      social: [
        { icon: "desktop", label: "Playground", href: "/play/" },
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/ivov/lisette",
        },
      ],
      plugins: [
        catppuccin({ dark: { flavor: "mocha", accent: "mauve" } }),
        starlightSidebarTopics([
          {
            label: "Introduction",
            id: "intro",
            icon: "rocket",
            link: "/docs/intro/quickstart/",
            items: [
              { slug: "docs/intro/quickstart" },
              {
                label: "Guides",
                items: [
                  { slug: "docs/intro/coming-from-go" },
                  { slug: "docs/intro/coming-from-rust" },
                  { slug: "docs/intro/safety" },
                ],
              },
            ],
          },
          {
            label: "Language Reference",
            icon: "open-book",
            link: "/docs/lexemes/",
            items: [
              {
                label: "Basics",
                items: [
                  { slug: "docs/lexemes" },
                  { slug: "docs/types" },
                  { slug: "docs/bindings" },
                  { slug: "docs/operators" },
                  { slug: "docs/control-flow" },
                ],
              },
              {
                label: "Data",
                items: [
                  { slug: "docs/structs" },
                  { slug: "docs/enums" },
                  { slug: "docs/references" },
                  { slug: "docs/pattern-matching" },
                  { slug: "docs/attributes" },
                ],
              },
              {
                label: "Behavior",
                items: [
                  { slug: "docs/functions" },
                  { slug: "docs/methods" },
                  { slug: "docs/interfaces" },
                  { slug: "docs/failures" },
                  { slug: "docs/concurrency" },
                ],
              },
            ],
          },
          {
            label: "Prelude",
            icon: "star",
            link: "/docs/prelude/",
            items: [
              { slug: "docs/prelude", label: "Overview" },
              {
                label: "Primitives",
                items: [
                  { slug: "docs/prelude/numerics" },
                  { slug: "docs/prelude/booleans" },
                  { slug: "docs/prelude/strings" },
                ],
              },
              {
                label: "Composites",
                items: [
                  { slug: "docs/prelude/option" },
                  { slug: "docs/prelude/result" },
                  { slug: "docs/prelude/partial" },
                  { slug: "docs/prelude/slice" },
                  { slug: "docs/prelude/array" },
                  { slug: "docs/prelude/map" },
                  { slug: "docs/prelude/ref" },
                  { slug: "docs/prelude/channels" },
                  { slug: "docs/prelude/ranges" },
                ],
              },
              {
                label: "Extras",
                items: [
                  { slug: "docs/prelude/functions" },
                  { slug: "docs/prelude/types" },
                  { slug: "docs/prelude/constraints" },
                ],
              },
            ],
          },
          {
            label: "Projects & Tooling",
            icon: "setting",
            link: "/docs/packages/",
            items: [
              {
                label: "Projects",
                items: [
                  { slug: "docs/packages" },
                  { slug: "docs/typedefs" },
                  { slug: "docs/go-standard-library" },
                  { slug: "docs/third-party-go-modules" },
                  { slug: "docs/go-module-overrides" },
                ],
              },
              {
                label: "Tooling",
                items: [
                  { slug: "docs/cli", label: "CLI" },
                  { slug: "docs/tests" },
                  { slug: "docs/scripts" },
                  { slug: "docs/lsp" },
                ],
              },
            ],
          },
          {
            label: "Changelog",
            icon: "list-format",
            link: "/docs/changelog/",
            // The topic needs an item to claim the page. `custom.css` drops the resulting lone link by this attribute.
            items: [{ slug: "docs/changelog", attrs: { "data-sole-entry": "" } }],
          },
        ]),
      ],
    }),
  ],
});
