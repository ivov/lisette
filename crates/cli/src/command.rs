use diagnostics::render::{Filter, OutputFormat};

#[derive(Debug, Default, PartialEq, Eq)]
pub enum TestSelection {
    #[default]
    All,
    Filter(String),
    Failed,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CheckAction {
    Inspect { deny_warnings: bool },
    Fix,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BindgenTarget {
    Package {
        name: String,
        output: Option<String>,
    },
    Stdlib {
        version: Option<String>,
    },
}

#[derive(Debug)]
pub enum Command {
    New {
        name: String,
    },
    Build {
        path: Option<String>,
        sourcemap: bool,
        go_flags: Vec<String>,
        output: Option<String>,
    },
    Emit {
        path: Option<String>,
        sourcemap: bool,
        output: Option<String>,
    },
    Run {
        target: Option<String>,
        args: Vec<String>,
        sourcemap: bool,
        go_flags: Vec<String>,
    },
    Format {
        path: Option<String>,
        check: bool,
    },
    Check {
        path: Option<String>,
        filter: Filter,
        action: CheckAction,
        format: OutputFormat,
    },
    Test {
        path: Option<String>,
        go_flags: Vec<String>,
        selection: TestSelection,
    },
    Overview,
    Help {
        command: Option<String>,
    },
    Version,
    Add {
        dependency: Option<String>,
        replace: Option<String>,
        path: Option<String>,
    },
    Sync,
    Lsp,
    Bindgen {
        target: BindgenTarget,
        verbose: bool,
    },
    Doc {
        query: Option<String>,
    },
    DocSearch {
        query: String,
    },
    Learn,
    Completions {
        shell: Option<String>,
    },
}

#[derive(Debug)]
pub enum ParseError {
    MissingArgument {
        command: &'static str,
        argument: &'static str,
    },
    UnknownCommand(String),
    UnknownFlag(String),
    UnexpectedArgument {
        message: String,
        reason: String,
        hint: String,
    },
}

fn parse_emit(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut path = None;
    let mut sourcemap = false;
    let mut output = None;

    while let Some(arg) = arguments.next() {
        if arg == "--sourcemap" {
            sourcemap = true;
        } else if let Some(value) = output_value(&arg, &mut arguments, "emit")? {
            output = Some(value);
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            set_target(&mut path, arg, "emit", "Run `lis emit` once per path")?;
        }
    }

    Ok(Command::Emit {
        path,
        sourcemap,
        output,
    })
}

/// An option-looking value almost always means the path was left off, so a
/// real one is spelled `./-name`.
fn output_value(
    arg: &str,
    arguments: &mut impl Iterator<Item = String>,
    command: &'static str,
) -> Result<Option<String>, ParseError> {
    let Some(value) = flag_value(arg, &["-o", "--output"], arguments, command, "-o <path>")? else {
        return Ok(None);
    };

    if value.is_empty() {
        return Err(ParseError::MissingArgument {
            command,
            argument: "-o <path>",
        });
    }

    if value.starts_with('-') {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected value `{}` for `-o`", value),
            reason: "`-o` takes a path, and a value starting with `-` reads as another flag"
                .to_string(),
            hint: format!(
                "Give the path, or write `./{}` if that really is the filename",
                value
            ),
        });
    }

    Ok(Some(value))
}

fn set_target(
    target: &mut Option<String>,
    value: String,
    command: &'static str,
    hint: &'static str,
) -> Result<(), ParseError> {
    if let Some(first) = target {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{}`", value),
            reason: format!(
                "`lis {}` takes one path, and `{}` was already given",
                command, first
            ),
            hint: hint.to_string(),
        });
    }
    *target = Some(value);
    Ok(())
}

/// Matches `arg` against a value-taking flag in both its `--flag value` and
/// `--flag=value` forms. Returns `Ok(None)` when `arg` is not one of the
/// given spellings.
fn flag_value(
    arg: &str,
    spellings: &[&str],
    arguments: &mut impl Iterator<Item = String>,
    command: &'static str,
    argument: &'static str,
) -> Result<Option<String>, ParseError> {
    for spelling in spellings {
        if arg == *spelling {
            return match arguments.next() {
                Some(value) => Ok(Some(value)),
                None => Err(ParseError::MissingArgument { command, argument }),
            };
        }
        if let Some(value) = arg
            .strip_prefix(spelling)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

fn extend_go_flags(go_flags: &mut Vec<String>, raw: &str) -> Result<(), ParseError> {
    match crate::shell_words::split(raw) {
        Ok(tokens) => {
            go_flags.extend(tokens);
            Ok(())
        }
        Err(crate::shell_words::SplitError::UnterminatedQuote(quote)) => {
            Err(ParseError::UnexpectedArgument {
                message: format!("unterminated {} quote in `--go-flags`", quote),
                reason: "the value passed to `--go-flags` has an unbalanced quote".to_string(),
                hint: "Balance the quotes, e.g. `--go-flags \"-ldflags='-s -w'\"`".to_string(),
            })
        }
    }
}

fn parse_output(value: &str) -> Result<OutputFormat, ParseError> {
    match value {
        "unix" => Ok(OutputFormat::Unix),
        other => Err(ParseError::UnexpectedArgument {
            message: format!("unexpected value `{}` for `--output`", other),
            reason: "`--output` accepts `unix`".to_string(),
            hint: "Use `lis check --output unix`".to_string(),
        }),
    }
}

fn parse_deny(value: &str) -> Result<bool, ParseError> {
    match value {
        "warnings" => Ok(true),
        other => Err(ParseError::UnexpectedArgument {
            message: format!("unexpected value `{}` for `--deny`", other),
            reason: "`--deny` accepts `warnings`".to_string(),
            hint: "Use `lis check --deny warnings`".to_string(),
        }),
    }
}

fn check_filter_conflict() -> ParseError {
    ParseError::UnexpectedArgument {
        message: "`--errors-only` and `--warnings-only` cannot be used together".to_string(),
        reason: "they select mutually exclusive sets of diagnostics".to_string(),
        hint: "Use only one of `--errors-only` or `--warnings-only`".to_string(),
    }
}

fn set_test_filter(selection: &mut TestSelection, pattern: String) -> Result<(), ParseError> {
    if pattern.is_empty() {
        return Err(ParseError::UnexpectedArgument {
            message: "`--filter` requires a non-empty pattern".to_string(),
            reason: "an empty pattern matches every test, the same as no filter".to_string(),
            hint: "Pass a pattern, e.g. `lis test --filter parse`".to_string(),
        });
    }
    if matches!(selection, TestSelection::Failed) {
        return Err(test_selection_conflict());
    }
    *selection = TestSelection::Filter(pattern);
    Ok(())
}

fn set_failed_selection(selection: &mut TestSelection) -> Result<(), ParseError> {
    if matches!(selection, TestSelection::Filter(_)) {
        return Err(test_selection_conflict());
    }
    *selection = TestSelection::Failed;
    Ok(())
}

fn test_selection_conflict() -> ParseError {
    ParseError::UnexpectedArgument {
        message: "`--failed` and `--filter` cannot be combined".to_string(),
        reason: "`--failed` reruns the previous run's failures, a fixed set".to_string(),
        hint: "Use one or the other".to_string(),
    }
}

impl Command {
    pub fn parse(args: Vec<String>, is_file: impl Fn(&str) -> bool) -> Result<Command, ParseError> {
        let mut arguments = args.into_iter().skip(1).peekable();

        let Some(command) = arguments.next() else {
            return Ok(Command::Overview);
        };

        if arguments.peek().is_some_and(|s| s == "-h" || s == "--help") {
            return Ok(Command::Help {
                command: Some(command),
            });
        }

        match command.as_str() {
            "new" => parse_new(arguments),
            "build" | "b" => parse_build(arguments),
            "emit" | "e" => parse_emit(arguments),
            "run" | "r" => parse_run(arguments, is_file),
            "format" | "f" => parse_format(arguments),
            "check" | "c" => parse_check(arguments),
            "test" | "t" => parse_test(arguments),
            "help" | "--help" => Ok(Command::Help {
                command: arguments.next(),
            }),
            "version" | "--version" => Ok(Command::Version),
            "add" => parse_add(arguments),
            "sync" => parse_sync(arguments),
            "lsp" => Ok(Command::Lsp),
            "learn" => Ok(Command::Learn),
            "complete" => Ok(Command::Completions {
                shell: arguments.next(),
            }),
            "doc" => parse_doc(arguments),
            "bindgen" => parse_bindgen(arguments),
            _ => Err(ParseError::UnknownCommand(command)),
        }
    }

    pub fn suggest(typo: &str) -> Option<String> {
        const COMMANDS: &[&str] = &[
            "new", "build", "emit", "run", "format", "check", "test", "help", "version", "add",
            "sync", "learn", "doc", "complete", "lsp", "bindgen",
        ];
        let candidates: Vec<String> = COMMANDS.iter().map(|s| s.to_string()).collect();
        diagnostics::infer::find_similar_name(typo, &candidates)
    }
}

fn parse_new(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    match arguments.next() {
        Some(name) => Ok(Command::New { name }),
        None => Err(ParseError::MissingArgument {
            command: "new",
            argument: "name",
        }),
    }
}

fn parse_build(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut path = None;
    let mut sourcemap = false;
    let mut go_flags = Vec::new();
    let mut output = None;

    while let Some(arg) = arguments.next() {
        if arg == "--sourcemap" {
            sourcemap = true;
        } else if let Some(value) = flag_value(
            &arg,
            &["--go-flags"],
            &mut arguments,
            "build",
            "--go-flags <flags>",
        )? {
            extend_go_flags(&mut go_flags, &value)?;
        } else if let Some(value) = output_value(&arg, &mut arguments, "build")? {
            output = Some(value);
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            set_target(&mut path, arg, "build", "Run `lis build` once per path")?;
        }
    }

    Ok(Command::Build {
        path,
        sourcemap,
        go_flags,
        output,
    })
}

fn parse_run(
    mut arguments: impl Iterator<Item = String>,
    is_file: impl Fn(&str) -> bool,
) -> Result<Command, ParseError> {
    let mut target = None;
    let mut args = Vec::new();
    let mut sourcemap = false;
    let mut go_flags = Vec::new();
    let mut program_arguments = false;

    while let Some(arg) = arguments.next() {
        if program_arguments {
            args.push(arg);
        } else if arg == "--" {
            program_arguments = true;
        } else if arg == "--sourcemap" {
            sourcemap = true;
        } else if let Some(value) = flag_value(
            &arg,
            &["--go-flags"],
            &mut arguments,
            "run",
            "--go-flags <flags>",
        )? {
            extend_go_flags(&mut go_flags, &value)?;
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            let ends_run_arguments = is_file(&arg);
            set_target(
                &mut target,
                arg,
                "run",
                "Pass one target, and arguments for the program after `--`",
            )?;
            program_arguments = ends_run_arguments;
        }
    }

    if let Some(flag) = go_flags
        .iter()
        .find(|f| crate::go_cli::is_go_output_flag(f))
    {
        return Err(ParseError::UnexpectedArgument {
            message: format!("`{}` cannot be passed to `lis run` via `--go-flags`", flag),
            reason: "`run` executes the binary it links at an internal path, so it owns `-o`"
                .to_string(),
            hint: "Use `lis build --go-flags \"-o <path>\"` to choose the output location"
                .to_string(),
        });
    }

    Ok(Command::Run {
        target,
        args,
        sourcemap,
        go_flags,
    })
}

fn parse_format(arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut path = None;
    let mut check = false;

    for arg in arguments {
        match arg.as_str() {
            "--check" => check = true,
            s if s.starts_with('-') => {
                return Err(ParseError::UnknownFlag(s.to_string()));
            }
            s => set_target(
                &mut path,
                s.to_string(),
                "format",
                "Run `lis format` once per path",
            )?,
        }
    }

    Ok(Command::Format { path, check })
}

fn parse_check(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut path = None;
    let mut filter = None;
    let mut deny_warnings = false;
    let mut format = OutputFormat::Graphical;
    let mut fix = false;

    while let Some(arg) = arguments.next() {
        if arg == "--errors-only" {
            match filter {
                Some(Filter::Warnings) => return Err(check_filter_conflict()),
                _ => filter = Some(Filter::Errors),
            }
        } else if arg == "--warnings-only" {
            match filter {
                Some(Filter::Errors) => return Err(check_filter_conflict()),
                _ => filter = Some(Filter::Warnings),
            }
        } else if arg == "--fix" {
            fix = true;
        } else if let Some(value) = flag_value(
            &arg,
            &["--output"],
            &mut arguments,
            "check",
            "--output <value>",
        )? {
            format = parse_output(&value)?;
        } else if let Some(value) =
            flag_value(&arg, &["--deny"], &mut arguments, "check", "--deny <value>")?
        {
            deny_warnings = parse_deny(&value)?;
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            set_target(&mut path, arg, "check", "Run `lis check` once per path")?;
        }
    }

    let filter = filter.unwrap_or_default();
    if filter == Filter::Errors && deny_warnings {
        return Err(ParseError::UnexpectedArgument {
            message: "`--errors-only` and `--deny warnings` cannot be used together".to_string(),
            reason:
                "`--errors-only` hides warnings, so `--deny warnings` would have nothing to act on"
                    .to_string(),
            hint: "Drop `--errors-only` to make warnings fail the check".to_string(),
        });
    }

    if fix && deny_warnings {
        return Err(ParseError::UnexpectedArgument {
            message: "`--fix` and `--deny warnings` cannot be used together".to_string(),
            reason: "`--fix` reports what it rewrote, not which warnings remain, so it cannot fail on leftover warnings"
                .to_string(),
            hint: "Run `lis check --fix` first, then `lis check --deny warnings`".to_string(),
        });
    }

    if fix && format == OutputFormat::Unix {
        return Err(ParseError::UnexpectedArgument {
            message: "`--fix` and `--output unix` cannot be used together".to_string(),
            reason:
                "`--fix` reports what it rewrote, not which diagnostics remain, so it has no per-diagnostic lines to emit"
                    .to_string(),
            hint: "Run `lis check --fix` first, then `lis check --output unix`".to_string(),
        });
    }

    Ok(Command::Check {
        path,
        filter,
        action: if fix {
            CheckAction::Fix
        } else {
            CheckAction::Inspect { deny_warnings }
        },
        format,
    })
}

fn parse_test(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut path = None;
    let mut go_flags = Vec::new();
    let mut selection = TestSelection::All;

    while let Some(arg) = arguments.next() {
        if let Some(value) = flag_value(
            &arg,
            &["-f", "--filter"],
            &mut arguments,
            "test",
            "--filter <pattern>",
        )? {
            set_test_filter(&mut selection, value)?;
        } else if arg == "--failed" {
            set_failed_selection(&mut selection)?;
        } else if let Some(value) = flag_value(
            &arg,
            &["--go-flags"],
            &mut arguments,
            "test",
            "--go-flags <flags>",
        )? {
            extend_go_flags(&mut go_flags, &value)?;
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            set_target(&mut path, arg, "test", "Run `lis test` once per path")?;
        }
    }

    if let Some(flag) = go_flags.iter().find(|f| crate::go_cli::is_go_json_flag(f)) {
        return Err(ParseError::UnexpectedArgument {
            message: format!("`{}` cannot be passed to `lis test` via `--go-flags`", flag),
            reason: "`lis test` runs `go test -json` and parses that stream to render the report"
                .to_string(),
            hint: "Remove `-json`; `lis test` manages it".to_string(),
        });
    }

    if let Some(flag) = go_flags
        .iter()
        .find(|f| crate::go_cli::is_go_selection_flag(f))
    {
        return Err(ParseError::UnexpectedArgument {
            message: format!("`{}` cannot be passed to `lis test` via `--go-flags`", flag),
            reason: "`lis test` selects which tests run and reconciles the report against them"
                .to_string(),
            hint: "Use `lis test --filter <pattern>` to select tests".to_string(),
        });
    }

    Ok(Command::Test {
        path,
        go_flags,
        selection,
    })
}

fn parse_add(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut dependency = None;
    let mut replace = None;
    let mut path = None;

    while let Some(arg) = arguments.next() {
        if let Some(value) = flag_value(
            &arg,
            &["--replace"],
            &mut arguments,
            "add",
            "--replace <module@version>",
        )? {
            replace = Some(value);
        } else if let Some(value) =
            flag_value(&arg, &["--path"], &mut arguments, "add", "--path <dir>")?
        {
            path = Some(value);
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else if dependency.is_none() {
            dependency = Some(arg);
        } else {
            return Err(ParseError::UnexpectedArgument {
                message: format!("unexpected argument `{}`", arg),
                reason: "`lis add` accepts a single dependency".to_string(),
                hint: "Run `lis add` once per dep".to_string(),
            });
        }
    }

    if path.is_some() && replace.is_some() {
        return Err(ParseError::UnexpectedArgument {
            message: "`--path` cannot be combined with `--replace`".to_string(),
            reason: "`--path` declares a local module, `--replace` substitutes a published one"
                .to_string(),
            hint: "Use one of the two flags".to_string(),
        });
    }
    if let (Some(_), Some(dependency)) = (&path, &dependency) {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{}`", dependency),
            reason: "`--path` takes no module argument, the module path is read from the directory's `go.mod`".to_string(),
            hint: "Run `lis add --path <dir>` with just the directory".to_string(),
        });
    }

    match (dependency, path) {
        (_, Some(path)) => Ok(Command::Add {
            dependency: None,
            replace: None,
            path: Some(path),
        }),
        (Some(dependency), None) => Ok(Command::Add {
            dependency: Some(dependency),
            replace,
            path: None,
        }),
        (None, None) => Err(ParseError::MissingArgument {
            command: "add",
            argument: "dependency",
        }),
    }
}

fn parse_sync(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    if let Some(extra) = arguments.next() {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{}`", extra),
            reason: "`lis sync` takes no arguments".to_string(),
            hint: "Run `lis sync` from the project root".to_string(),
        });
    }
    Ok(Command::Sync)
}

fn parse_doc(arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut search = false;
    let mut query = None;
    let mut extra = None;

    for arg in arguments {
        match arg.as_str() {
            "-s" | "--search" => search = true,
            s if s.starts_with('-') => {
                return Err(ParseError::UnknownFlag(s.to_string()));
            }
            _ if query.is_none() => query = Some(arg),
            _ if extra.is_none() => extra = Some(arg),
            _ => {}
        }
    }

    if search {
        return match query {
            Some(q) => Ok(Command::DocSearch { query: q }),
            None => Err(ParseError::MissingArgument {
                command: "doc",
                argument: "search query",
            }),
        };
    }

    if let (Some(q), Some(item)) = (&query, &extra) {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{}`", item),
            reason: "The `doc` command takes a single query argument".to_string(),
            hint: format!("Did you mean `lis doc {}.{}`?", q, item),
        });
    }
    Ok(Command::Doc { query })
}

fn parse_bindgen(mut arguments: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut positional = Vec::new();
    let mut output = None;
    let mut verbose = false;

    while let Some(arg) = arguments.next() {
        if arg == "-v" || arg == "--verbose" {
            verbose = true;
        } else if let Some(value) = flag_value(
            &arg,
            &["-o", "--output"],
            &mut arguments,
            "bindgen",
            "--output <path>",
        )? {
            if value.is_empty() {
                return Err(ParseError::MissingArgument {
                    command: "bindgen",
                    argument: "--output <path>",
                });
            }
            output = Some(value);
        } else if arg.starts_with('-') {
            return Err(ParseError::UnknownFlag(arg));
        } else {
            positional.push(arg);
        }
    }

    let target = bindgen_target(&positional, output)?;
    Ok(Command::Bindgen { target, verbose })
}

fn bindgen_target(
    positional: &[String],
    output: Option<String>,
) -> Result<BindgenTarget, ParseError> {
    let Some(package) = positional.first() else {
        return Err(ParseError::MissingArgument {
            command: "bindgen",
            argument: "package",
        });
    };
    let extra = positional.get(1);
    if let Some(trailing) = positional.get(2) {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{trailing}`"),
            reason: "`lis bindgen` accepts at most a target and stdlib version".to_string(),
            hint: "Remove the extra argument".to_string(),
        });
    }

    if package == "stdlib" {
        if output.is_some() {
            return Err(ParseError::UnexpectedArgument {
                message: "`--output` cannot be used when generating stdlib bindings".to_string(),
                reason: "stdlib bindings are written to the repository typedef directory"
                    .to_string(),
                hint: "Remove `--output`".to_string(),
            });
        }
        return Ok(BindgenTarget::Stdlib {
            version: extra.cloned(),
        });
    }

    if let Some(extra) = extra {
        return Err(ParseError::UnexpectedArgument {
            message: format!("unexpected argument `{extra}`"),
            reason: "package bindgen accepts a single package target".to_string(),
            hint: "Remove the extra argument".to_string(),
        });
    }
    Ok(BindgenTarget::Package {
        name: package.clone(),
        output,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(parts: &[&str]) -> Result<Command, ParseError> {
        parse_with_files(parts, &[])
    }

    fn parse_with_files(parts: &[&str], files: &[&str]) -> Result<Command, ParseError> {
        Command::parse(parts.iter().map(|s| s.to_string()).collect(), |path| {
            files.contains(&path)
        })
    }

    #[test]
    fn test_failed_flag_parses() {
        let Ok(Command::Test { selection, .. }) = parse(&["lis", "test", "--failed"]) else {
            panic!("expected Test command");
        };
        assert_eq!(selection, TestSelection::Failed);
    }

    #[test]
    fn test_failed_and_filter_conflict() {
        assert!(parse(&["lis", "test", "--failed", "-f", "parse"]).is_err());
    }

    #[test]
    fn add_without_replace_has_no_replace() {
        let Ok(Command::Add {
            dependency,
            replace,
            path,
        }) = parse(&["lis", "add", "github.com/gorilla/mux"])
        else {
            panic!("expected Add command");
        };
        assert_eq!(dependency.as_deref(), Some("github.com/gorilla/mux"));
        assert_eq!(replace, None);
        assert_eq!(path, None);
    }

    #[test]
    fn add_replace_flag_after_dependency() {
        let Ok(Command::Add {
            dependency,
            replace,
            ..
        }) = parse(&[
            "lis",
            "add",
            "github.com/df-mc/dragonfly",
            "--replace",
            "github.com/you/dragonfly@v1.2.0",
        ])
        else {
            panic!("expected Add command");
        };
        assert_eq!(dependency.as_deref(), Some("github.com/df-mc/dragonfly"));
        assert_eq!(replace.as_deref(), Some("github.com/you/dragonfly@v1.2.0"));
    }

    #[test]
    fn add_replace_flag_before_dependency_and_equals_form() {
        let Ok(Command::Add {
            dependency,
            replace,
            ..
        }) = parse(&[
            "lis",
            "add",
            "--replace=github.com/you/dragonfly@v1.2.0",
            "github.com/df-mc/dragonfly",
        ])
        else {
            panic!("expected Add command");
        };
        assert_eq!(dependency.as_deref(), Some("github.com/df-mc/dragonfly"));
        assert_eq!(replace.as_deref(), Some("github.com/you/dragonfly@v1.2.0"));
    }

    #[test]
    fn add_replace_without_value_errors() {
        assert!(parse(&["lis", "add", "dep", "--replace"]).is_err());
    }

    #[test]
    fn add_path_flag_takes_a_directory_and_no_module() {
        let Ok(Command::Add {
            dependency,
            replace,
            path,
        }) = parse(&["lis", "add", "--path", "../foo"])
        else {
            panic!("expected Add command");
        };
        assert_eq!(dependency, None);
        assert_eq!(replace, None);
        assert_eq!(path.as_deref(), Some("../foo"));
    }

    #[test]
    fn add_path_rejects_module_argument_and_replace_combination() {
        assert!(parse(&["lis", "add", "example.com/foo", "--path", "../foo"]).is_err());
        assert!(
            parse(&[
                "lis",
                "add",
                "--path",
                "../foo",
                "--replace",
                "github.com/you/foo@v1.0.0"
            ])
            .is_err()
        );
        assert!(parse(&["lis", "add", "--path"]).is_err());
    }

    #[test]
    fn check_defaults_to_graphical_format() {
        let Ok(Command::Check { format, .. }) = parse(&["lis", "check"]) else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Graphical);
    }

    #[test]
    fn check_output_unix_space_form() {
        let Ok(Command::Check { format, .. }) = parse(&["lis", "check", "--output", "unix"]) else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
    }

    #[test]
    fn check_output_unix_equals_form() {
        let Ok(Command::Check { format, .. }) = parse(&["lis", "check", "--output=unix"]) else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
    }

    #[test]
    fn check_output_missing_value() {
        assert!(matches!(
            parse(&["lis", "check", "--output"]),
            Err(ParseError::MissingArgument {
                command: "check",
                argument: "--output <value>",
            })
        ));
    }

    #[test]
    fn check_output_invalid_value() {
        assert!(matches!(
            parse(&["lis", "check", "--output", "json"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_rejects_both_filter_flags() {
        assert!(matches!(
            parse(&["lis", "check", "--errors-only", "--warnings-only"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_deny_defaults_off() {
        let Ok(Command::Check { action, .. }) = parse(&["lis", "check"]) else {
            panic!("expected Check command");
        };
        assert_eq!(
            action,
            CheckAction::Inspect {
                deny_warnings: false
            }
        );
    }

    #[test]
    fn check_deny_warnings_space_form() {
        let Ok(Command::Check { action, .. }) = parse(&["lis", "check", "--deny", "warnings"])
        else {
            panic!("expected Check command");
        };
        assert_eq!(
            action,
            CheckAction::Inspect {
                deny_warnings: true
            }
        );
    }

    #[test]
    fn check_deny_warnings_equals_form() {
        let Ok(Command::Check { action, .. }) = parse(&["lis", "check", "--deny=warnings"]) else {
            panic!("expected Check command");
        };
        assert_eq!(
            action,
            CheckAction::Inspect {
                deny_warnings: true
            }
        );
    }

    #[test]
    fn check_deny_missing_value() {
        assert!(matches!(
            parse(&["lis", "check", "--deny"]),
            Err(ParseError::MissingArgument {
                command: "check",
                argument: "--deny <value>",
            })
        ));
    }

    #[test]
    fn check_deny_invalid_value() {
        assert!(matches!(
            parse(&["lis", "check", "--deny", "errors"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_deny_warnings_composes_with_warnings_only() {
        let Ok(Command::Check { action, filter, .. }) =
            parse(&["lis", "check", "--deny", "warnings", "--warnings-only"])
        else {
            panic!("expected Check command");
        };
        assert_eq!(
            action,
            CheckAction::Inspect {
                deny_warnings: true
            }
        );
        assert_eq!(filter, Filter::Warnings);
    }

    #[test]
    fn check_rejects_deny_warnings_with_errors_only() {
        assert!(matches!(
            parse(&["lis", "check", "--errors-only", "--deny", "warnings"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_rejects_deny_warnings_with_fix() {
        assert!(matches!(
            parse(&["lis", "check", "--fix", "--deny", "warnings"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_fix_parses_as_a_distinct_action() {
        let Ok(Command::Check { action, .. }) = parse(&["lis", "check", "--fix"]) else {
            panic!("expected Check command");
        };

        assert_eq!(action, CheckAction::Fix);
    }

    fn run_parts(parts: &[&str]) -> (Option<String>, Vec<String>, Vec<String>) {
        let Ok(Command::Run {
            target,
            args,
            go_flags,
            ..
        }) = parse(parts)
        else {
            panic!("expected Run command");
        };
        (target, args, go_flags)
    }

    #[test]
    fn run_target_only() {
        let (target, args, go_flags) = run_parts(&["lis", "run", "."]);
        assert_eq!(target.as_deref(), Some("."));
        assert!(args.is_empty());
        assert!(go_flags.is_empty());
    }

    #[test]
    fn run_go_flags_before_target() {
        let (target, _, go_flags) = run_parts(&["lis", "run", "--go-flags", "-race", "."]);
        assert_eq!(target.as_deref(), Some("."));
        assert_eq!(go_flags, vec!["-race"]);
    }

    #[test]
    fn run_go_flags_after_target() {
        let (target, _, go_flags) = run_parts(&["lis", "run", ".", "--go-flags", "-race"]);
        assert_eq!(target.as_deref(), Some("."));
        assert_eq!(go_flags, vec!["-race"]);
    }

    #[test]
    fn run_go_flags_equals_form() {
        let (_, _, go_flags) = run_parts(&["lis", "run", "--go-flags=-trimpath"]);
        assert_eq!(go_flags, vec!["-trimpath"]);
    }

    #[test]
    fn run_go_flags_inner_quoted_value_stays_one_token() {
        let (_, _, go_flags) =
            run_parts(&["lis", "run", "--go-flags", "-trimpath -ldflags='-s -w'"]);
        assert_eq!(go_flags, vec!["-trimpath", "-ldflags=-s -w"]);
    }

    #[test]
    fn run_separator_routes_remaining_tokens_to_program_args() {
        let (target, args, go_flags) = run_parts(&["lis", "run", ".", "--", "--go-flags", "-race"]);
        assert_eq!(target.as_deref(), Some("."));
        assert_eq!(args, vec!["--go-flags", "-race"]);
        assert!(go_flags.is_empty());
    }

    #[test]
    fn run_rejects_output_flag_separated_form() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags", "-o /tmp/x"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn run_rejects_output_flag_joined_form() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags", "-o=/tmp/x"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn run_rejects_output_flag_double_dash_separated_form() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags", "--o /tmp/x"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn run_rejects_output_flag_double_dash_joined_form() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags", "--o=/tmp/x"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn test_rejects_json_flag_in_go_flags() {
        assert!(matches!(
            parse(&["lis", "test", "--go-flags", "-json=false"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
        assert!(matches!(
            parse(&["lis", "test", "--go-flags", "-json"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn test_accepts_other_go_flags() {
        let Ok(Command::Test { go_flags, .. }) =
            parse(&["lis", "test", "--go-flags", "-failfast -tags run"])
        else {
            panic!("expected Test command");
        };
        assert_eq!(go_flags, vec!["-failfast", "-tags", "run"]);
    }

    #[test]
    fn test_rejects_selection_flags_in_go_flags() {
        for flag in ["-run", "-skip", "-list"] {
            assert!(
                matches!(
                    parse(&["lis", "test", "--go-flags", flag]),
                    Err(ParseError::UnexpectedArgument { .. })
                ),
                "expected `{flag}` to be rejected"
            );
        }
    }

    #[test]
    fn test_rejects_empty_filter() {
        for args in [
            vec!["lis", "test", "-f", ""],
            vec!["lis", "test", "--filter="],
            vec!["lis", "test", "-f="],
        ] {
            assert!(
                matches!(parse(&args), Err(ParseError::UnexpectedArgument { .. })),
                "expected {args:?} to be rejected"
            );
        }
    }

    #[test]
    fn a_second_positional_target_names_both_paths() {
        for args in [
            vec!["lis", "run", "a.lis", "b.lis"],
            vec!["lis", "check", "a.lis", "b.lis"],
            vec!["lis", "build", "one", "two"],
            vec!["lis", "emit", "one", "two"],
            vec!["lis", "format", "one", "two"],
            vec!["lis", "test", "one", "two"],
        ] {
            let Err(ParseError::UnexpectedArgument {
                message, reason, ..
            }) = parse(&args)
            else {
                panic!("expected {args:?} to be rejected");
            };
            assert!(message.contains(args[3]), "{message}");
            assert!(reason.contains(args[2]), "{reason}");
        }
    }

    #[test]
    fn program_arguments_after_a_separator_are_not_targets() {
        let (target, args, _) = run_parts(&["lis", "run", "a.lis", "--", "b.lis"]);
        assert_eq!(target.as_deref(), Some("a.lis"));
        assert_eq!(args, vec!["b.lis"]);
    }

    fn run_file_parts(
        parts: &[&str],
        files: &[&str],
    ) -> (Option<String>, Vec<String>, Vec<String>) {
        let Ok(Command::Run {
            target,
            args,
            go_flags,
            ..
        }) = parse_with_files(parts, files)
        else {
            panic!("expected Run command");
        };
        (target, args, go_flags)
    }

    #[test]
    fn everything_after_a_file_target_goes_to_the_program() {
        let (target, args, _) =
            run_file_parts(&["lis", "run", "tool.lis", "alpha", "beta"], &["tool.lis"]);
        assert_eq!(target.as_deref(), Some("tool.lis"));
        assert_eq!(args, vec!["alpha", "beta"]);
    }

    #[test]
    fn a_flag_the_compiler_knows_belongs_to_the_program_after_a_file_target() {
        let (target, args, go_flags) = run_file_parts(
            &[
                "lis",
                "run",
                "--sourcemap",
                "tool.lis",
                "--sourcemap",
                "--go-flags",
                "input.txt",
            ],
            &["tool.lis"],
        );
        assert_eq!(target.as_deref(), Some("tool.lis"));
        assert_eq!(args, vec!["--sourcemap", "--go-flags", "input.txt"]);
        assert!(go_flags.is_empty());
    }

    #[test]
    fn a_separator_after_a_file_target_belongs_to_the_program() {
        let (target, args, _) =
            run_file_parts(&["lis", "run", "tool.lis", "--", "-o", "--"], &["tool.lis"]);
        assert_eq!(target.as_deref(), Some("tool.lis"));
        assert_eq!(args, vec!["--", "-o", "--"]);
    }

    #[test]
    fn a_separator_before_a_directory_target_still_separates() {
        let (target, args, _) = run_file_parts(&["lis", "run", ".", "--", "-o"], &["tool.lis"]);
        assert_eq!(target.as_deref(), Some("."));
        assert_eq!(args, vec!["-o"]);
    }

    #[test]
    fn an_extensionless_file_target_ends_the_compiler_arguments() {
        let (target, args, _) = run_file_parts(&["lis", "run", "backup", "--verbose"], &["backup"]);
        assert_eq!(target.as_deref(), Some("backup"));
        assert_eq!(args, vec!["--verbose"]);
    }

    #[test]
    fn a_second_target_after_a_directory_still_points_at_the_separator() {
        let Err(ParseError::UnexpectedArgument { hint, .. }) =
            parse_with_files(&["lis", "run", ".", "arg"], &["tool.lis"])
        else {
            panic!("expected a rejection");
        };
        assert!(hint.contains("`--`"), "{hint}");
    }

    #[test]
    fn a_directory_target_keeps_the_compiler_grammar() {
        let (target, args, go_flags) =
            run_file_parts(&["lis", "run", ".", "--go-flags", "-race"], &["tool.lis"]);
        assert_eq!(target.as_deref(), Some("."));
        assert!(args.is_empty());
        assert_eq!(go_flags, vec!["-race"]);
    }

    #[test]
    fn run_rejects_unknown_flag() {
        assert!(matches!(
            parse(&["lis", "run", "--bogus"]),
            Err(ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn run_go_flags_requires_value() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags"]),
            Err(ParseError::MissingArgument {
                command: "run",
                argument: "--go-flags <flags>",
            })
        ));
    }

    #[test]
    fn run_go_flags_rejects_unterminated_quote() {
        assert!(matches!(
            parse(&["lis", "run", "--go-flags", "-ldflags='-s"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn emit_parses_path_and_sourcemap() {
        let Ok(Command::Emit {
            path, sourcemap, ..
        }) = parse(&["lis", "emit", "src", "--sourcemap"])
        else {
            panic!("expected Emit command");
        };
        assert_eq!(path.as_deref(), Some("src"));
        assert!(sourcemap);
    }

    #[test]
    fn emit_rejects_unknown_flag() {
        assert!(matches!(
            parse(&["lis", "emit", "--bogus"]),
            Err(ParseError::UnknownFlag(_))
        ));
    }

    #[test]
    fn emit_parses_both_output_spellings() {
        for argument in [
            vec!["lis", "emit", "tool.lis", "-o", "out.go"],
            vec!["lis", "emit", "tool.lis", "--output", "out.go"],
            vec!["lis", "emit", "tool.lis", "--output=out.go"],
        ] {
            let Ok(Command::Emit { path, output, .. }) = parse(&argument) else {
                panic!("expected Emit command for {argument:?}");
            };
            assert_eq!(path.as_deref(), Some("tool.lis"));
            assert_eq!(output.as_deref(), Some("out.go"), "for {argument:?}");
        }
    }

    #[test]
    fn build_parses_output_beside_go_flags() {
        let Ok(Command::Build {
            path,
            go_flags,
            output,
            ..
        }) = parse(&[
            "lis",
            "build",
            "tool.lis",
            "-o",
            "dist/tool",
            "--go-flags",
            "-trimpath",
        ])
        else {
            panic!("expected Build command");
        };
        assert_eq!(path.as_deref(), Some("tool.lis"));
        assert_eq!(output.as_deref(), Some("dist/tool"));
        assert_eq!(go_flags, vec!["-trimpath"]);
    }

    #[test]
    fn output_flag_without_a_path_is_a_missing_argument() {
        for argument in [vec!["lis", "emit", "-o"], vec!["lis", "build", "-o"]] {
            assert!(
                matches!(
                    parse(&argument),
                    Err(ParseError::MissingArgument {
                        argument: "-o <path>",
                        ..
                    })
                ),
                "expected a missing-argument error for {argument:?}"
            );
        }
    }

    #[test]
    fn output_flag_rejects_a_value_that_looks_like_an_option() {
        for argument in [
            vec!["lis", "build", "-o", "--sourcemap"],
            vec!["lis", "build", "-o", "--go-flags"],
            vec!["lis", "emit", "-o", "--output"],
            vec!["lis", "emit", "-o", "--bogus"],
            vec!["lis", "emit", "--output=-o"],
        ] {
            assert!(
                matches!(parse(&argument), Err(ParseError::UnexpectedArgument { .. })),
                "expected a rejection for {argument:?}"
            );
        }
    }

    #[test]
    fn output_flag_accepts_a_dash_path_spelled_relatively() {
        let Ok(Command::Emit { output, .. }) =
            parse(&["lis", "emit", "f.lis", "-o", "./-weird.go"])
        else {
            panic!("expected Emit command");
        };
        assert_eq!(output.as_deref(), Some("./-weird.go"));
    }

    #[test]
    fn build_parses_go_flags_before_target() {
        let Ok(Command::Build { path, go_flags, .. }) =
            parse(&["lis", "build", "--go-flags", "-trimpath", "."])
        else {
            panic!("expected Build command");
        };
        assert_eq!(path.as_deref(), Some("."));
        assert_eq!(go_flags, vec!["-trimpath"]);
    }

    #[test]
    fn build_parses_go_flags_equals_form() {
        let Ok(Command::Build { go_flags, .. }) = parse(&["lis", "build", "--go-flags=-race"])
        else {
            panic!("expected Build command");
        };
        assert_eq!(go_flags, vec!["-race"]);
    }

    #[test]
    fn build_allows_output_flag() {
        let Ok(Command::Build { go_flags, .. }) =
            parse(&["lis", "build", "--go-flags", "-o dist/app"])
        else {
            panic!("expected Build command");
        };
        assert_eq!(go_flags, vec!["-o", "dist/app"]);
    }

    #[test]
    fn build_parses_sourcemap_and_go_flags() {
        let Ok(Command::Build {
            sourcemap,
            go_flags,
            ..
        }) = parse(&["lis", "build", "--sourcemap", "--go-flags", "-trimpath"])
        else {
            panic!("expected Build command");
        };
        assert!(sourcemap);
        assert_eq!(go_flags, vec!["-trimpath"]);
    }

    #[test]
    fn build_go_flags_requires_value() {
        assert!(matches!(
            parse(&["lis", "build", "--go-flags"]),
            Err(ParseError::MissingArgument {
                command: "build",
                argument: "--go-flags <flags>",
            })
        ));
    }

    #[test]
    fn check_output_composes_with_errors_only() {
        let Ok(Command::Check { format, filter, .. }) =
            parse(&["lis", "check", "--output", "unix", "--errors-only"])
        else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
        assert_eq!(filter, Filter::Errors);
    }

    #[test]
    fn bindgen_package_carries_only_package_options() {
        let Ok(Command::Bindgen { target, verbose }) = parse(&[
            "lis",
            "bindgen",
            "github.com/acme/pkg",
            "-o",
            "pkg.d.lis",
            "-v",
        ]) else {
            panic!("expected Bindgen command");
        };

        assert_eq!(
            (target, verbose),
            (
                BindgenTarget::Package {
                    name: "github.com/acme/pkg".to_string(),
                    output: Some("pkg.d.lis".to_string()),
                },
                true,
            )
        );
    }

    #[test]
    fn bindgen_package_rejects_ignored_version_argument() {
        let result = parse(&["lis", "bindgen", "github.com/acme/pkg", "v1.2.3"]);

        assert!(matches!(result, Err(ParseError::UnexpectedArgument { .. })));
    }

    #[test]
    fn bindgen_stdlib_rejects_ignored_output_argument() {
        let result = parse(&["lis", "bindgen", "stdlib", "--output", "ignored"]);

        assert!(matches!(result, Err(ParseError::UnexpectedArgument { .. })));
    }

    #[test]
    fn bindgen_output_requires_a_path() {
        let result = parse(&["lis", "bindgen", "github.com/acme/pkg", "--output"]);

        assert!(matches!(result, Err(ParseError::MissingArgument { .. })));
    }
}
