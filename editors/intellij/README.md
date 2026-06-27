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
    lis version # -> lis 0.5.0 (go 1.25.10)
    ```

2. Install the plugin. Either:

    - **From the JetBrains Marketplace:** in your JetBrains IDE, open **Settings → Plugins → Marketplace**, search for "Lisette", and click **Install**.
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

To produce a distributable zip: `./gradlew buildPlugin` and output at `build/distributions/lisette-intellij-<version>.zip`.

## Publishing

The plugin is published on the [JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31270-lisette) (plugin ID 31270). To release a new version:

1. Bump `version` in `build.gradle.kts` and `<version>` in `src/main/resources/META-INF/plugin.xml`, and add a `<change-notes>` entry. The Marketplace rejects re-uploads of an existing version.

2. Build the zip with `./gradlew buildPlugin`. The build vendors the current VSCode TextMate grammar, so highlighting tracks `editors/vscode/syntaxes/lisette.tmLanguage.json` as of build time.

3. Sign in as the vendor, open the [plugin page](https://plugins.jetbrains.com/plugin/31270-lisette), and use **Upload update** to upload `build/distributions/lisette-intellij-<version>.zip`. The page's name, description, and change notes refresh from the bundled `plugin.xml`.
