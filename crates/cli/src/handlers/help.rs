use crate::cli_error;
use crate::output::print_help;

const VERSION: &str = env!("CARGO_PKG_VERSION");
include!(concat!(env!("OUT_DIR"), "/go_version.rs"));

pub fn print_main_help() {
    print_help(&format!(
        "lisette {} (go {})

Usage: `lis <command>`

Commands:
    `new`      Create a new project
    `build`    Compile a project
    `run`      Compile and run a project
    `format`   Format a file or project
    `check`    Validate a file or project
    `clean`    Remove build artifacts
    `learn`    Generate a sample project
    `doc`      Explore prelude or Go stdlib
    `bindgen`  Generate `.d.lis` bindings
    `lsp`      Start the LSP server
    `help`     Print this message

Hint: Run `lis help <command>` to learn more about a command.
      New to Lisette? See https://lisette.run/quickstart",
        VERSION, GO_VERSION
    ));
}

pub fn print_command_help(command: &str) {
    match command {
        "new" => print_help(
            "`lis new` <name>

Create a new Lisette project.

Arguments:
    <name>    Name of the project to create (e.g., `myproject`)",
        ),

        "build" | "b" => print_help(
            "`lis build` [path] [options]

Compile a Lisette project.

Arguments:
    [path]    Path to project directory (default: current directory)

Options:
    `--debug`    Include `//line` directives in generated Go code for stack traces

Abbreviation: `b`

Examples:
    `lis build`                          Build project in current directory
    `lis build` {path/to/project/dir:g}      Build project in specified directory
    `lis build` {--debug:g}                  Build with source mapping directives",
        ),

        "run" | "r" => print_help(
            "`lis run` [target:g] [options] [-- args...]

Compile and execute a Lisette file or project.

Arguments:
    [target:g]         Project directory or `.lis` file (default: current directory)
    [args]           Arguments to pass to the program (after --)

Options:
    `--debug`    Include `//line` directives in generated Go code for stack traces

Abbreviation: `r`

Examples:
    `lis run`                            Run project in current directory
    `lis run` {path/to/project/dir:g}        Run project in specified directory
    `lis run` {script.lis:g}                 Run a standalone script
    `lis run` {script.lis:g} {-- arg}          Pass argument to script",
        ),

        "format" | "f" => print_help(
            "`lis format` [path] [--check]

Format Lisette source files.

Arguments:
    [path]      Path to file or directory (default: current directory)

Options:
    `--check`     Check if files are formatted without modifying them

Abbreviation: `f`

Examples:
    `lis format`                   Format project in current directory
    `lis format` {src/main.lis:g}      Format a single file
    `lis format` {--check}           Check formatting in current directory",
        ),

        "check" | "c" => print_help(
            "`lis check` [path]

Type-check and lint Lisette source files, without emitting code.

Arguments:
    [path]    Path to file or directory (default: current directory)

Abbreviation: `c`

Examples:
    `lis check`                          Check project in current directory
    `lis check` {path/to/project/dir:g}      Check project in specified directory
    `lis check` {script.lis:g}               Check a single file",
        ),

        "clean" | "x" => print_help(
            "`lis clean` [path]

Remove build artifacts, i.e. `target` directory.

Arguments:
    [path]    Project path (default: current directory)

Abbreviation: `x`",
        ),

        "add" => print_help(
            "`lis add` <dependency>

Add a dependency to the project.

Arguments:
    <dependency>    Dependency to add (e.g., `github.com/user/repo`)",
        ),

        "remove" | "rm" => print_help(
            "`lis remove` <dependency>

Remove a dependency from the project.

Arguments:
    <dependency>    Dependency to remove (e.g., `github.com/user/repo`)",
        ),

        "list" => print_help(
            "`lis list`

List all dependencies in the project.",
        ),

        "lsp" => print_help(
            "`lis lsp`

Start the Lisette language server, for use by IDEs.",
        ),

        "bindgen" => print_help(
            "`lis bindgen` <package> [options]

Generate `.d.lis` type definition bindings for a Go package.

Arguments:
    <package>    Go package path to generate bindings for (e.g., `fmt`, `net/http`)

Options:
    `-o`, `--output`=<path>    Output file path (default: <package>`.d.lis`)
    `-f`, `--force`            Regenerate even if output exists
    `-v`, `--verbose`          Show verbose output

Examples:
    `lis bindgen` {fmt:g}                      Generate typedef for `fmt` as `fmt.d.lis`
    `lis bindgen` {net/http:g} {-o http.d.lis}   Generate typedef for `net/http` as `http.d.lis`
    `lis bindgen` {encoding/json:g} {-v}         Generate typedef for `encoding/json` with verbose logs",
        ),

        "learn" => print_help(
            "`lis learn`

Generate a sample project to explore Lisette's features.

Creates a `learn-lisette` directory with a CLI task manager that demonstrates
enums, structs, pattern matching, error handling, closures, Go interop, and concurrency.",
        ),

        "doc" => print_help(
            "`lis doc` [query]

Explore the prelude — types, methods, and builtin functions available without imports.

Arguments:
    [query]              Type or type.method to look up (omit to list all)
    `-s`, `--search` <term>  Search across prelude and Go stdlib

Examples:
    `lis doc`                          List all prelude types and functions
    `lis doc` {Option:g}                   Show `Option` definition and its methods
    `lis doc` {Option.map:g}               Show the `map` method on `Option`
    `lis doc` {Slice:g}                    Show `Slice` definition and its methods
    `lis doc` {go:strings:g}               Browse the `strings` Go package
    `lis doc` `-s split`                 Search for `split` across prelude and Go stdlib
    `lis doc` `-s contains`              Find all `contains` methods and functions",
        ),

        "help" => print_help(
            "`lis help` <command>

Print help information.

Arguments:
    <command>    Command to get help for (e.g., `run`, `build`)",
        ),

        unknown => {
            cli_error!(
                "Unknown command",
                format!("`{}` is not a lis command", unknown),
                "Run `lis help` for available commands"
            );
        }
    }
}

pub fn print_version() {
    println!("lisette {} (go {})", VERSION, GO_VERSION);
}
