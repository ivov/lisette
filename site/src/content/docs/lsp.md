---
title: "LSP"
description: "Editor support for Lisette through the Language Server Protocol"
---

Lisette speaks the Language Server Protocol, with extensions for multiple IDEs:

| Editor | Install |
| --- | --- |
| VSCode | [Lisette extension](https://marketplace.visualstudio.com/items?itemName=ivov.lisette) in the VSCode marketplace |
| Zed | [Lisette extension](https://zed.dev/extensions/lisette) in the Zed extension registry |
| JetBrains | [Lisette extension](https://plugins.jetbrains.com/plugin/31270-lisette) in the JetBrains Marketplace |
| Neovim | Install [this plugin](https://github.com/ivov/lisette/tree/main/editors/nvim#installation) |
| Helix | Add [this config](https://github.com/ivov/lisette/tree/main/editors/helix#installation) to `languages.toml` |
| Emacs | Use the community [`lisette-ts-mode`](https://github.com/escherize/lisette-ts-mode) package |

For all IDEs, ensure the `lis` binary is on your `PATH` first.

For unlisted IDEs, point the LSP client at `lis lsp`, which serves over stdio.

## Capabilities

| Capability | Behavior |
| --- | --- |
| Diagnostics | Errors and lints as you type |
| Quick fixes | One-click autofixes for lints |
| Hover | Type and doc comment under the cursor |
| Completions | Members after `.` and attributes after `#` and `[` |
| Signature help | Parameters while typing a call, on `(` and `,` |
| Go to definition | Jumps to the declaration, including typedefs for Go symbols |
| References | Every use of a symbol |
| Rename | Across the project, with a preview |
| Formatting | Whole document, the same output as `lis format` |
| Document symbols | The outline panel |
| Inlay hints | Inferred types and parameter names |
