import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import monacoEditorPlugin from "vite-plugin-monaco-editor";

export default defineConfig({
  resolve: {
    alias: {
      // The browser entry is a UMD build that wants a global Monaco.
      "monaco-vim": fileURLToPath(new URL("node_modules/monaco-vim/dist/index.mjs", import.meta.url)),
    },
  },
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
  optimizeDeps: {
    exclude: ["monaco-editor", "monaco-vim"],
  },
  worker: {
    format: "es",
  },
});
