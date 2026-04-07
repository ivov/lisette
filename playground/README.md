# Lisette Playground

A browser-based playground for the [Lisette](https://lisette.run) programming language — Rust syntax that compiles to Go.

## Features

- **Monaco Editor** with Lisette syntax highlighting (official TextMate grammar from the Lisette VSCode extension, with Monarch fallback)
- **WASM compiler** — the real Lisette compiler (`lisette-format`, `lisette-syntax`, `lisette-semantics`, `lisette-emit`) compiled to WebAssembly and running entirely in the browser
- **Run** — compiles Lisette → Go in-browser, then executes via the Go Playground API (Ctrl+Enter)
- **Format** — reformats code using the official formatter (Alt+Shift+F)
- **Type check** — live squiggle diagnostics ~1 s after you stop typing; full report in the Diagnostics tab
- **Go source** — compiled Go output shown in a dedicated tab
- **Autocomplete** — keyword, type, and snippet completions; WASM-backed semantic completions when available

## Getting started

```sh
npm install
npm run dev
```

Open http://localhost:5173.

The pre-built WASM compiler is committed to `public/wasm/`. If you modify the Rust wrapper in `wasm/` you can rebuild it:

```sh
npm run build:wasm   # requires Rust + wasm-pack
```

### Production build

```sh
npm run build
```

For GitHub Pages, set the base path env var before building:

```sh
VITE_BASE_PATH=/lisette-playground/ npm run build
```

## Project structure

```
├── src/
│   ├── main.ts                  # App entry: toolbar, tabs, keyboard shortcuts
│   ├── style.css                # Dark theme
│   ├── editor/
│   │   ├── index.ts             # Monaco setup, providers
│   │   ├── language.ts          # Monarch tokenizer (fallback), completions, snippets
│   │   ├── textmate.ts          # Official TM grammar via vscode-textmate + onigasm
│   │   └── theme.ts             # lisette-dark colour theme
│   └── runner/
│       ├── wasm-bridge.ts       # Loads public/wasm/lisette_wasm.js, typed interface
│       └── executor.ts          # Sends compiled Go to play.golang.org
├── wasm/
│   ├── Cargo.toml               # wasm-bindgen crate; git deps on lisette-* crates
│   └── src/lib.rs               # format / check / compile / complete / hover
├── public/
│   ├── wasm/                    # Pre-built WASM module (lisette_wasm_bg.wasm + JS glue)
│   ├── lisette.tmLanguage.json  # Official TextMate grammar (from editors/vscode/syntaxes/)
│   └── onigasm.wasm             # Oniguruma regex engine for TM grammar
└── .github/workflows/
    └── deploy.yml               # GitHub Pages deployment
```

## Execution pipeline

```
Lisette source
     │
     ▼  (WASM: lisette-syntax + lisette-semantics + lisette-emit)
  Go source
     │
     ▼  (fetch POST to play.golang.org/compile)
  stdout / stderr
```

## Deploying to GitHub Pages

The workflow in `.github/workflows/deploy.yml` triggers on every push to `main`.

One-time setup in the repository settings:

1. **Settings → Pages → Source** — set to **GitHub Actions**
2. Merge to `main`

The playground will be live at `https://<owner>.github.io/lisette-playground/`.

## Rebuilding the WASM compiler

```sh
# Install Rust and wasm-pack if needed
curl https://sh.rustup.rs -sSf | sh
cargo install wasm-pack

npm run build:wasm
```

The crate pulls the Lisette compiler crates from GitHub at build time (`lisette-format`, `lisette-syntax`, `lisette-semantics`, `lisette-emit`, `lisette-diagnostics`). A full rebuild takes a few minutes on first run.
