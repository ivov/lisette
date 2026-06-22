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
    `new`          Create a new project
    `run`, `r`       Compile and run a project
    `build`, `b`     Compile a project to a binary
    `emit`, `e`      Emit Go code into `target` dir
    `check`, `c`     Lint and typecheck a project
    `format`, `f`    Format a project
    `test`, `t`      Run a project's tests {(in development):d}
    `add`          Add a third-party Go dependency
    `sync`         Tidy up project manifest

Extras:
    `version`      Print compiler version
    `help`         Show help for a command
    `doc`          Browse documentation
    `learn`        Create a new sample project
    `complete`     Shell completion scripts
    `lsp`          Start the language server",
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
    `new`, `run`, `build`, `emit`, `check`, `format`, `test`, `add`, `sync`

Extras:
    `version`, `help`, `doc`, `learn`, `complete`, `lsp`",
    );
}

pub fn print_command_help(command: &str) {
    match command {
        "new" => print_help(
            "`lis new` <project-name>

Create a new Lisette project in the current directory.

    {hello-world}/
    ├── src/
    │   └── main.lis
    ├── lisette.toml
    ├── README.md
    ├── AGENTS.md
    └── .gitignore

Arguments:
    {project-name:g} {(required):d}    Name of the project, e.g. {hello-world}",
        ),

        "build" | "b" => print_help(
            "`lis build` {[path]} {[--flags]:b}

Compile a Lisette project to a binary at the `target/bin` dir.

Arguments:
    {path:g} {(optional):d}                     Path to project dir (default: current dir)

Flags:
    {--sourcemap:b}                         Include `//line` in Go code for stack traces
    {--go-flags:b} {\"<flags>\":g}                Pass flags through to `go build`

Examples:
    `lis build`                           Build project in current dir
    `lis build` {~/projects/demo:g}           Build project in specific dir
    `lis build` {--go-flags:b} {\"-trimpath\":g}    Strip file paths from the binary",
        ),

        "emit" | "e" => print_help(
            "`lis emit` {[path]} {[--flags]:b}

Generate Go code from a Lisette project into the `target` dir.

Arguments:
    {path:g} {(optional):d}             Path to project dir (default: current dir)

Flags:
    {--sourcemap:b}                 Include `//line` in Go code for stack traces

Examples:
    `lis emit`                    Emit Go for project in current dir
    `lis emit` {~/projects/demo:g}    Emit Go for project in specific dir",
        ),

        "run" | "r" => print_help(
            "`lis run` {[target]} {[--flags]:b}

Compile a Lisette project to a binary at `target/bin` and run the binary.

Arguments:
    {target:g} {(optional):d}                        Project dir (default: current dir)

Flags:
    {--sourcemap:b}                              Include `//line` in Go code for stack traces
    {--go-flags:b} {\"<flags>\":g}                     Pass flags through to `go build`

Examples:
    `lis run`                                  Run project in current dir
    `lis run` {~/projects/demo:g}                  Run project in specific dir
    `lis run` {calculate.lis:g}                    Run a standalone script
    `lis run` {greet.lis:g} `--` {john:g}                Pass argument to script
    `lis run` {--go-flags:b} {\"-ldflags='-s -w'\":g}    Pass linker flags to `go build`",
        ),

        "format" | "f" => print_help(
            "`lis format` {[path]} {[--flags]:b}

Format source files in a Lisette project.

Arguments:
    {path:g} {(optional):d}               Path to file or dir (default: current dir)

Flags:
    {--check:b}                       Verify formatting without modifying files

Examples:
    `lis format`                    Format project in current dir
    `lis format` {~/projects/demo:g}    Format project in specific dir
    `lis format` {src/main.lis:g}       Format a single file
    `lis format` {--check:b}            Verify formatting in current dir",
        ),

        "check" | "c" => print_help(
            "`lis check` {[path]} {[--flags]:b}

Lint and typecheck a Lisette project.

Arguments:
    {path:g} {(optional):d}              Path to dir (default: current dir)

Flags:
    {--errors-only:b}                Show only errors
    {--warnings-only:b}              Show only warnings
    {--output:b} {unix}                Machine-readable output

Examples:
    `lis check`                    Check project in current dir
    `lis check` {~/projects/demo:g}    Check project in specific dir
    `lis check` {script.lis:g}         Check single file
    `lis check` {--output:b} {unix}      One diagnostic per line",
        ),

        "test" | "t" => print_help(
            "`lis test` {[path]} {[--flags]:b}

Run tests in a Lisette project.

Arguments:
    {path:g} {(optional):d}                    Path to dir (default: current dir)

Flags:
    {-f:b}, {--filter:b} {<pattern>:g}             Run only tests whose name contains pattern
    {--failed:b}                           Rerun the tests that failed last time
    {--go-flags:b} {\"<flags>\":g}               Pass flags through to `go test`

Examples:
    `lis test`                           Run all tests in current dir
    `lis test` {~/projects/demo:g}           Run tests in specific dir
    `lis test` {-f:b} {parse:g}                  Only tests whose name contains \"parse\"
    `lis test` {--go-flags:b} {\"-failfast\":g}    Stop at the first failing test",
        ),

        "add" => print_help(
            "`lis add` <dependency> {[@version]:b}

Add a third-party Go module as a dependency to your Lisette project.

Arguments:
    {dependency:g} {(required):d}          Go module name

Examples:
    `lis add` {google/uuid:g}            Latest version
    `lis add` {google/uuid:g}{@v1.6.0:b}     Exact version
    `lis add` {google/uuid:g}{@2d3c2a9:b}    Exact commit hash or branch
    `lis add` {go.uber.org/zap:g}        Full path for non-GitHub host",
        ),

        "sync" => print_help(
            "`lis sync`

Tidy `lisette.toml` against the `go:` imports in `src/`, similar to `go mod tidy`.
Will drop dependency entries no longer reached by any import, and generate
typedefs for every imported package. Run this after removing imports, deleting
source files, or pulling new code.",
        ),

        "lsp" => print_help(
            "`lis lsp`

Start the Lisette language server over stdio, for use by editor extensions.",
        ),

        "bindgen" => print_help(
            "`lis bindgen` <package> {[--flags]:b}

Generate `.d.lis` type definition bindings for a Go package.

Arguments:
    {package:g} {(required):d}                    Go package path (e.g., {fmt}, {net/http})

Flags:
    {-o:b}, {--output:b} <path>                   Output file path (default: <package>`.d.lis`)
    {-f:b}, {--force:b}                           Regenerate even if output exists
    {-v:b}, {--verbose:b}                         Show verbose output

Examples:
    `lis bindgen` {fmt:g}                       Generate typedef for {fmt} as `fmt.d.lis`
    `lis bindgen` {net/http:g} {-o:b} {http.d.lis:g}    Generate typedef for {net/http} as {http.d.lis}
    `lis bindgen` {encoding/json:g} {-v:b}          Generate typedef for {encoding/json} with verbose logs",
        ),

        "learn" => print_help(
            "`lis learn`

Create a sample Lisette project in the current directory.

    `learn-lisette`/
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
            "`lis doc` {[symbol]} {[--flags]:b}

Browse documentation on a symbol from the prelude or from the Go stdlib

Arguments:
    {symbol:g} {(optional):d}      Symbol to look up (omit to list all)

Flags:
    {-s:b}, {--search:b} <term>    Search docs for term

Examples:
    `lis doc`                List prelude symbols and Go packages
    `lis doc` {Slice:g}          Docs on Lisette's `Slice` type
    `lis doc` {Slice.map:g}      Docs on `map` method on Lisette's `Slice` type
    `lis doc` {prelude:g}        List all Lisette prelude symbols
    `lis doc` {go::g}            List all Go stdlib packages
    `lis doc` {go:os:g}          Docs on Go stdlib `os` package contents
    `lis doc` {go:os.File:g}     Docs on `File` type in Go stdlib `os` package
    `lis doc` {-s:b} {append:g}      Search docs for `append`",
        ),

        "complete" => print_help(
            "`lis complete` <shell>

Generate shell completion scripts.

Arguments:
    {shell:g} {(required):d}    Shell to generate completions for ({bash}, {zsh}, or {fish})

Examples:
    `lis complete` {bash} > ~/.local/share/bash-completion/completions/lis
    `lis complete` {fish} > ~/.config/fish/completions/lis.fish

    For zsh, add to ~/.zshrc (before compinit):
        fpath=(~/.zfunc $fpath)
    Then generate:
        mkdir -p ~/.zfunc && `lis complete` {zsh} > ~/.zfunc/_lis",
        ),

        "help" => print_help(
            "`lis help` <command>

Show help for a command.

Arguments:
    {command:g} {(required):d}    Command to get help for (e.g., {run:g}, {build:g})",
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
