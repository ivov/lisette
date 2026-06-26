# Zed extension for Lisette

Zed language support for [Lisette](https://github.com/ivov/lisette).

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
    lis version # -> lisette 0.2.1 (go 1.25.10)
    ```

2. Search for "Lisette" in Zed's extensions panel and click "Install".

## Development

1. Install the WASM target: `rustup target add wasm32-wasip2`
2. Build the Lisette binary: `just build`
3. Set the binary path at `~/.config/zed/settings.json`

    ```json
    {
      "lsp": {
        "lisette-lsp": {
          "binary": {
            "path": "/path/to/lisette/target/release/lis",
            "arguments": ["lsp"]
          }
        }
      }
    }
    ```

4. In `extension.toml`, temporarily switch the grammar to your local repo, and set `rev` to a commit on your local branch:

    ```toml
    [grammars.lisette]
    repository = "file:///path/to/lisette/repo/root"
    rev = "..."
    ```

5. In Zed, run `zed: install dev extension` and select the `editors/zed` dir.
6. Create a test project and open a `.lis` file.

To iterate:

- Make your changes and run `just build`
- If you changed `editors/tree-sitter-lisette/`, update `rev` in `extension.toml` and reinstall the dev extension.
- Run `editor: restart language server`

## Publishing

### First time

1. Fork [`zed-industries/extensions`](https://github.com/zed-industries/extensions)
2. Add this repo as a Git submodule and add an entry to `extensions.toml`
3. Open a PR.

### Updating

Run `just zed-release` from the repo root to automate steps 1 and 2. Then review the diff and continue from step 3.

1. Bump `version` in `extension.toml` and `Cargo.toml`, then run `cargo build` in `editors/zed` so the matching `zed_lisette` version in `Cargo.lock` updates too.
2. Find the commit SHA that Zed should build the tree-sitter grammar from and set it in `grammars.lisette.rev` in `extension.toml`.

    ```bash
    git rev-parse HEAD
    # e.g. 5a8800385fdd4e9fe02b758d9d6298b18fd92b72
    ```

    Then set it in `extension.toml`:

    ```toml
    [grammars.lisette]
    rev = "5a8800385fdd4e9fe02b758d9d6298b18fd92b72"
    ```

3. Commit, push, open a PR in this repo with the changes above, and merge it. Note its merge commit SHA.
4. In your fork of [`zed-industries/extensions`](https://github.com/zed-industries/extensions), point the `extensions/lisette` submodule at that merge commit, bump `version` under `[lisette]` in their `extensions.toml` to match, and open a PR.
