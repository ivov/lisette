# Neovim plugin for Lisette

Neovim language support for [Lisette](https://github.com/ivov/lisette). Requires Neovim 0.11+.

## Features

- Syntax highlighting
- Diagnostics
- Hover
- Completions
- Go-to-definition
- References
- Rename
- Signature help
- Formatting
- Document symbols

## Installation

1. Install the Lisette binary:

    ```bash
    cargo install lisette
    lis version # -> lisette 0.1.0 (go 1.25.5)
    ```

2. Add to your `lazy.nvim` config:

    ```lua
    {
      "ivov/lisette",
      ft = "lisette",
      config = function(plugin)
        vim.opt.rtp:append(plugin.dir .. "/editors/nvim")
      end,
    }
    ```

## Development

1. Build the Lisette binary: `just build`

2. Add to `~/.config/nvim/lua/plugins/lisette.lua`

    ```lua
    {
      dir = "path/to/lisette/editors/nvim",
      ft = "lisette",
    }
    ```

3. In [`lsp/lisette.lua`](lsp/lisette.lua), temporarily set an absolute path to the debug binary:

    ```lua
    cmd = { "path/to/lisette/target/release/lis", "lsp" },
    ```

4. Create a test project and open it in Neovim. Verify the LSP is running with `:checkhealth vim.lsp`.

To iterate:

- Make your changes and run `just build` to rebuild the binary.
- Restart the LSP with `:LspRestart` or reopen the file.

To debug syntax highlighting, run `:Inspect` on any token to see which highlight group it resolves to. To debug LSP, check `:LspLog`.
