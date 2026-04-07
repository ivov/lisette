# Lisette support for Helix

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

2. Add to your `languages.toml` config:

    ```toml
    [language-server.lisette-lsp]
    command = "lis"
    args = ["lsp"]

    [[language]]
    name = "lisette"
    scope = "source.lisette"
    injection-regex = "lis|lisette"
    file-types = ["lis"]
    roots = ["lisette.toml"]
    auto-format = true
    comment-tokens = ["//", "///"]
    language-servers = ["lisette-lsp"]
    indent = { tab-width = 2, unit = "  " }

    [[grammar]]
    name = "lisette"
    source = { git = "https://github.com/ivov/lisette", rev = "dd62a38f70bbde085d4f23557305d455299f4774", subpath = "editors/tree-sitter-lisette" }
    ```

3. Fetch and build the tree-sitter grammar:

    ```bash
    hx --grammar fetch
    hx --grammar build
    ```

4. Copy the query files so Helix can use them for syntax highlighting:

    ```bash
    mkdir -p ~/.config/helix/runtime/queries/lisette
    cp ~/.config/helix/runtime/grammars/sources/lisette/editors/tree-sitter-lisette/queries/* \
      ~/.config/helix/runtime/queries/lisette/
    ```
