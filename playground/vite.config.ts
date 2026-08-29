import path from "node:path";
import { defineConfig } from "vite";
import monacoEditorPlugin from "vite-plugin-monaco-editor";

export default defineConfig({
  // Built output goes into the site's public directory, which Astro copies to the root of
  // its own output, so the playground is served at lisette.run/play. The base URL matches.
  base: "/play/",
  build: {
    outDir: "../site/public/play",
    emptyOutDir: true,
    target: "es2020",
  },
  plugins: [
    (monacoEditorPlugin as unknown as typeof monacoEditorPlugin.default).default(
      {
        languageWorkers: ["editorWorkerService"],
        // Without this override the plugin appends the base path to outDir,
        // producing play/play/monacoeditorwork (double "play").
        customDistPath: (_root, buildOutDir) =>
          path.join(buildOutDir, "monacoeditorwork"),
      }
    ),
  ],
  server: {
    headers: {
      // Enables SharedArrayBuffer in local dev (Monaco can use it).
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  optimizeDeps: {
    exclude: ["monaco-editor"],
  },
  worker: {
    format: "es",
  },
});
