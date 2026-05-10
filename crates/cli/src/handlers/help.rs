use crate::cli_error;
use crate::output::{print_dimmed, print_help};

const VERSION: &str = env!("CARGO_PKG_VERSION");
include!(concat!(env!("OUT_DIR"), "/go_version.rs"));

pub fn print_main_help() {
    print_help(
        "Lisette compiler and toolchain.

Usage:
    `lis` <command>

Commands:
    `new`        Create a new project
    `build`, `b`   Compile a project to Go
    `run`, `r`     Compile and run a project
    `format`, `f`  Format a project
    `check`, `c`   Validate a project
    `add`        Add a third-party Go dependency
    `sync`       Reconcile project manifest

Extras:
    `version`    Print compiler version
    `help`       Show help for a command
    `doc`        Browse symbols and packages
    `learn`      Create a new sample project
    `complete`   Shell completion scripts
    `lsp`        Start the language server",
    );
    println!();
    print_dimmed("New to Lisette? https://lisette.run/quickstart");
}

pub fn print_help_prompt() {
    print_help(
        "Show help for a command.

Usage:
    `lis help` <command>

Commands:
    `new`, `build`, `run`, `format`, `check`, `add`, `sync`, `doc`

Extras:
    `version`, `help`, `learn`, `complete`, `lsp`",
    );
}

pub fn print_command_help(command: &str) {
    match command {
        "new" => print_help(
            "`lis new` <name>

Create a new Lisette project in the current directory.

    .
    └── `hello_world`/
        ├── src/
        │   └── main.lis
        ├── lisette.toml
        ├── README.md
        ├── AGENTS.md
        └── .gitignore

Arguments:
    <name>    Name of the project, e.g. `hello_world`",
        ),

        "build" | "b" => print_help(
            "`lis build` [path] [options]

Compile a Lisette project.

Arguments:
    [path]    Path to project dir (default: current dir)

Options:
    `--debug`    Include `//line` directives in generated Go code for stack traces

Examples:
    `lis build`                          Build project in current dir
    `lis build` {path/to/project/dir:g}      Build project in specific dir
    `lis build` {--debug:g}                  Build with source mapping directives",
        ),

        "run" | "r" => print_help(
            "`lis run` [target:g] [options] [-- args...]

Compile and execute a Lisette file or project.

Arguments:
    [target:g]         Project dir or `.lis` file (default: current dir)
    [args]           Arguments to pass to the program (after --)

Options:
    `--debug`    Include `//line` directives in generated Go code for stack traces

Examples:
    `lis run`                            Run project in current dir
    `lis run` {path/to/project/dir:g}        Run project in specific dir
    `lis run` {script.lis:g}                 Run a standalone script
    `lis run` {script.lis:g} {-- arg}          Pass argument to script",
        ),

        "format" | "f" => print_help(
            "`lis format` [path] [--check]

Format Lisette source files.

Arguments:
    [path]      Path to file or dir (default: current dir)

Options:
    [--check]     Check if files are formatted without modifying them

Examples:
    `lis format`                   Format project in current dir
    `lis format` {src/main.lis:g}      Format a single file
    `lis format` {--check}           Check formatting in current dir",
        ),

        "check" | "c" => print_help(
            "`lis check` [path]

Find errors and warnings in Lisette source files.

Arguments:
    [path]    Path to file or dir (default: current dir)

Examples:
    `lis check`                          Check project in current dir
    `lis check` {path/to/project/dir:g}      Check project in specific dir
    `lis check` {script.lis:g}               Check single file",
        ),

        "add" => print_help(
            "`lis add` <dependency>

Add a third-party Go dependency to your Lisette project. Will download the
Go module, record the module and its transitives in `lisette.toml`, and
generate typedefs for the target module and any imported packages.

Arguments:
    <dependency>    Go module path with optional `@version` (default: latest)

Examples:
    `lis add` {google/uuid:g}                   Latest version
    `lis add` {google/uuid@v1.6.0:g}            Exact version
    `lis add` {google/uuid@2d3c2a9:g}           Exact commit hash or branch
    `lis add` {go.uber.org/zap:g}               Full path for non-GitHub host",
        ),

        "sync" => print_help(
            "`lis sync`

Tidy `lisette.toml` against the `go:` imports in `src/`, similar to
`go mod tidy`. Will drop dependency entries no longer reached by any import, and
generate typedefs for every imported package. Run this after removing imports,
deleting source files, or pulling new code.",
        ),

        "lsp" => print_help(
            "`lis lsp`

Start the Lisette language server over stdio, for use by editor extensions.",
        ),

        "bindgen" => print_help(
            "`lis bindgen` <package> [options]

Generate `.d.lis` type definition bindings for a Go package.

Arguments:
    <package>    Go package path (e.g., `fmt`, `net/http`)

Options:
    `-o`, `--output` <path>    Output file path (default: <package>`.d.lis`)
    `-f`, `--force`            Regenerate even if output exists
    `-v`, `--verbose`          Show verbose output

Examples:
    `lis bindgen` {fmt:g}                      Generate typedef for `fmt` as `fmt.d.lis`
    `lis bindgen` {net/http:g} {-o http.d.lis}   Generate typedef for `net/http` as `http.d.lis`
    `lis bindgen` {encoding/json:g} {-v}         Generate typedef for `encoding/json` with verbose logs",
        ),

        "learn" => print_help(
            "`lis learn`

Create a sample Lisette project in the current directory.

    .
    └── learn-lisette/
        ├── src/
        │   ├── main.lis
        │   ├── models/
        │   │   ├── props.lis
        │   │   └── task.lis
        │   ├── store/
        │   │   └── store.lis
        │   ├── commands/
        │   │   └── commands.lis
        │   └── display/
        │       └── display.lis
        ├── lisette.toml
        ├── README.md
        ├── AGENTS.md
        └── .gitignore

The sample is a CLI task manager that demonstrates enums, structs, pattern
matching, error handling, closures, Go interop, and concurrency.",
        ),

        "doc" => print_help(
            "`lis doc` [query]

Browse symbols and packages.

Arguments:
    [query]              Symbol or package to look up (omit to list all in stdlib)
    `-s`, `--search` <term>  Search across symbols and packages

Examples:
    `lis doc`                          List all prelude types and functions
    `lis doc` {Option:g}                   Show {Option:g} definition and its methods
    `lis doc` {Option.map:g}               Show the {map:g} method on {Option:g}
    `lis doc` {Slice:g}                    Show {Slice:g} definition and its methods
    `lis doc` {go:strings:g}               Browse the {strings:g} Go package
    `lis doc` `-s` {split:g}                 Look up {split:g}",
        ),

        "complete" => print_help(
            "`lis complete` <shell>

Generate shell completion scripts.

Arguments:
    <shell>    Shell to generate completions for (`bash`, `zsh`, or `fish`)

Examples:
    `lis complete bash` > ~/.local/share/bash-completion/completions/lis
    `lis complete fish` > ~/.config/fish/completions/lis.fish

    For zsh, add to ~/.zshrc (before compinit):
        fpath=(~/.zfunc $fpath)
    Then generate:
        mkdir -p ~/.zfunc && `lis complete zsh` > ~/.zfunc/_lis",
        ),

        "help" => print_help(
            "`lis help` <command>

Show help for a command.

Arguments:
    <command>    Command to get help for (e.g., `run`, `build`)",
        ),

        "version" => print_help(
            "`lis version`

Print compiler version (Lisette and Go toolchain).",
        ),

        unknown => {
            let hint = match crate::command::Command::suggest(unknown) {
                Some(suggestion) => format!("Did you mean `{}`?", suggestion),
                None => "Run `lis help` for available commands".to_string(),
            };
            cli_error!(
                "Unknown command",
                format!("`{}` is not a lis command", unknown),
                hint
            );
        }
    }
}

pub fn print_version() {
    println!("lis {} (go {})", VERSION, GO_VERSION);
}
