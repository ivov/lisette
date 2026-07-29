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

The WASM compiler must be built before `npm run dev` will work — `public/wasm/` is not committed:

```sh
npm run build:wasm   # requires Rust + wasm-pack
```

### Production build

From the repo root, the `just rebuild-playground` recipe runs `npm install`, `npm run build:wasm`, and `npm run build`. The output is written to `docs/play/` (configured in `vite.config.ts`), which is committed and served by GitHub Pages at `lisette.run/play`.

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
│   ├── Cargo.toml               # wasm-bindgen crate; path deps on lisette-* crates
│   └── src/lib.rs               # format / check / compile / complete / hover
└── public/
    ├── wasm/                    # WASM module — built by `npm run build:wasm`, not committed
    ├── lisette.tmLanguage.json  # symlink → ../../editors/vscode/syntaxes/lisette.tmLanguage.json
    ├── onig.wasm                # Oniguruma regex engine for TM grammar
    └── onigasm.wasm
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

## Deploying

Deployment is content-based: GitHub Pages serves whatever is in `docs/play/` on `main`. To deploy a new version:

1. Run `just rebuild-playground` from the repo root
2. Commit the regenerated `docs/play/` tree
3. Merge to `main`

## Rebuilding the WASM compiler

```sh
# Install Rust and wasm-pack if needed
curl https://sh.rustup.rs -sSf | sh
cargo install wasm-pack

npm run build:wasm
```

The crate builds the Lisette compiler crates from this checkout (`lisette-format`, `lisette-syntax`, `lisette-semantics`, `lisette-passes`, `lisette-emit`, `lisette-diagnostics`, `lisette-deps`), so it compiles whatever is in your working tree rather than what is on `main`. A full rebuild takes a few minutes on first run.

Because those are path dependencies, changes to the compiler crates can break this crate, and nothing in CI builds it. If `npm run build:wasm` fails to compile, the bindings in `wasm/src/lib.rs` have drifted from the current crate APIs and need updating before the playground can be rebuilt.
