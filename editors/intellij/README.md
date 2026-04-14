# JetBrains plugin for Lisette

JetBrains language support for [Lisette](https://github.com/ivov/lisette). Works in GoLand, IntelliJ IDEA Ultimate, and other LSP-capable JetBrains IDEs on 2024.3 or later.

## Features

- Syntax highlighting
- Diagnostics
- Hover
- Completions
- Go-to-definition
- References
- Signature help
- Formatting
- Document symbols

Rename is not wired up. JetBrains' LSP API does not bridge `textDocument/rename` as of IntelliJ Platform 2024.3, so there is no platform hook to hand it off to. Use Find Usages as a workaround.

## Installation

1. Install the Lisette binary:

    ```bash
    cargo install lisette
    lis version # -> lisette 0.1.0 (go 1.25.5)
    ```

2. Install the plugin. Either:

    - **From the JetBrains Marketplace** (pending publication as of April 2026): in your JetBrains IDE, open **Settings → Plugins → Marketplace**, search for "Lisette", and click **Install**.
    - **From disk:** build the zip yourself with `./gradlew buildPlugin` (see [Development](#development) below), then open **Settings → Plugins → ⚙️ → Install Plugin from Disk...** and select `editors/intellij/build/distributions/lisette-intellij-<version>.zip`.

    Restart the IDE when prompted.

## Development

1. Make sure `lis` is on your `PATH`, since the plugin spawns `lis lsp` as a subprocess.

2. Launch a sandbox IDE with the plugin loaded:

    ```bash
    cd editors/intellij
    ./gradlew runIde
    ```

    First run downloads ~1 GB of IntelliJ Platform artifacts and a JDK 17 toolchain via Foojay.

3. Create a test project and open a `.lis` file in the sandbox IDE.

To produce a distributable zip: `./gradlew buildPlugin` and output at `build/distributions/lisette-intellij-0.1.0.zip`.
