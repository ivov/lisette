const esbuild = require("esbuild");
const production = process.argv.includes("--production");

esbuild
  .build({
    entryPoints: ["src/extension.ts"],
    bundle: true,
    format: "cjs",
    minify: production,
    sourcemap: !production,
    platform: "node",
    outfile: "dist/extension.js",
    external: ["vscode"],
  })
  .catch(() => process.exit(1));
