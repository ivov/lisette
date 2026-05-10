# VS Code extension for Lisette

VSCode language support for [Lisette](https://github.com/ivov/lisette).

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

2. Install the [Lisette extension](https://marketplace.visualstudio.com/items?itemName=ivov.lisette) from the Visual Studio marketplace.

## Development

1. Build the Lisette binary (`just build` from the repo root).

2. Prepare the extension:

    ```bash
    cd editors/vscode
    pnpm install
    pnpm run compile
    ```

3. Open the `editors/vscode` dir in VSCode and press F5 to launch the Extension Development Host (EDH).
4. Create a test project. In the EDH, open the test project.
5. in the EDH's settings (`Cmd+,`), set `lisette.serverPath` to the absolute path of the debug binary, e.g. `/path/to/lisette/target/release/lis`
6. In the VSCode window open on the `editors/vscode` dir, press Shift+F5 to stop, then F5 to relaunch the EDH with LSP connected.

To iterate:

- Make your changes and run `just build` to rebuild the binary
- In the VSCode window open on the `editors/vscode` dir, press Shift+F5 to stop, then F5 to relaunch the EDH with LSP connected.

## Publishing

To test locally without publishing:

```bash
pnpm run package
code --install-extension lisette-0.1.0.vsix
```

To publish:

1. Bump `version` in `package.json`
2. `pnpm run publish`
3. `npx ovsx publish lisette-<version>.vsix --pat $OVSX_PAT`