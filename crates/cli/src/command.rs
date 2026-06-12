use diagnostics::render::OutputFormat;

#[derive(Debug)]
pub enum Command {
    New {
        name: String,
    },
    Build {
        path: Option<String>,
        sourcemap: bool,
    },
    Emit {
        path: Option<String>,
        sourcemap: bool,
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
        errors_only: bool,
        warnings_only: bool,
        format: OutputFormat,
    },
    Overview,
    Help {
        command: Option<String>,
    },
    Version,
    Add {
        dependency: String,
    },
    Sync,
    Lsp,
    Bindgen {
        package: String,
        output: Option<String>,
        version: Option<String>,
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

fn parse_path_and_sourcemap(
    arguments: impl Iterator<Item = String>,
) -> Result<(Option<String>, bool), ParseError> {
    let mut path = None;
    let mut sourcemap = false;
    for arg in arguments {
        match arg.as_str() {
            "--sourcemap" => sourcemap = true,
            s if s.starts_with('-') => return Err(ParseError::UnknownFlag(s.to_string())),
            s => path = Some(s.to_string()),
        }
    }
    Ok((path, sourcemap))
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

fn is_go_output_flag(token: &str) -> bool {
    token == "-o" || token.starts_with("-o=")
}

fn parse_format(value: &str) -> Result<OutputFormat, ParseError> {
    match value {
        "unix" => Ok(OutputFormat::Unix),
        other => Err(ParseError::UnexpectedArgument {
            message: format!("unexpected value `{}` for `--output`", other),
            reason: "`--output` accepts `unix`".to_string(),
            hint: "Use `lis check --output unix`".to_string(),
        }),
    }
}

impl Command {
    pub fn parse(args: Vec<String>) -> Result<Command, ParseError> {
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
            "new" => match arguments.next() {
                Some(name) => Ok(Command::New { name }),
                None => Err(ParseError::MissingArgument {
                    command: "new",
                    argument: "name",
                }),
            },

            "build" | "b" => {
                let (path, sourcemap) = parse_path_and_sourcemap(arguments)?;
                Ok(Command::Build { path, sourcemap })
            }

            "emit" | "e" => {
                let (path, sourcemap) = parse_path_and_sourcemap(arguments)?;
                Ok(Command::Emit { path, sourcemap })
            }

            "run" | "r" => {
                let mut target = None;
                let mut args = Vec::new();
                let mut sourcemap = false;
                let mut go_flags = Vec::new();
                let mut found_separator = false;

                while let Some(arg) = arguments.next() {
                    if found_separator {
                        args.push(arg);
                    } else if arg == "--" {
                        found_separator = true;
                    } else if arg == "--sourcemap" {
                        sourcemap = true;
                    } else if arg == "--go-flags" {
                        let Some(value) = arguments.next() else {
                            return Err(ParseError::MissingArgument {
                                command: "run",
                                argument: "--go-flags <flags>",
                            });
                        };
                        extend_go_flags(&mut go_flags, &value)?;
                    } else if let Some(value) = arg.strip_prefix("--go-flags=") {
                        extend_go_flags(&mut go_flags, value)?;
                    } else if arg.starts_with('-') {
                        return Err(ParseError::UnknownFlag(arg));
                    } else {
                        target = Some(arg);
                    }
                }

                if let Some(flag) = go_flags.iter().find(|f| is_go_output_flag(f)) {
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

            "format" | "f" => {
                let mut path = None;
                let mut check = false;

                for arg in arguments {
                    match arg.as_str() {
                        "--check" => check = true,
                        s if s.starts_with('-') => {
                            return Err(ParseError::UnknownFlag(s.to_string()));
                        }
                        s => path = Some(s.to_string()),
                    }
                }

                Ok(Command::Format { path, check })
            }

            "check" | "c" => {
                let mut path = None;
                let mut errors_only = false;
                let mut warnings_only = false;
                let mut format = OutputFormat::Graphical;

                while let Some(arg) = arguments.next() {
                    match arg.as_str() {
                        "--errors-only" => errors_only = true,
                        "--warnings-only" => warnings_only = true,
                        "--output" => {
                            let Some(value) = arguments.next() else {
                                return Err(ParseError::MissingArgument {
                                    command: "check",
                                    argument: "--output <value>",
                                });
                            };
                            format = parse_format(&value)?;
                        }
                        s if s.starts_with("--output=") => {
                            format = parse_format(s.split_once('=').unwrap().1)?;
                        }
                        s if s.starts_with('-') => {
                            return Err(ParseError::UnknownFlag(s.to_string()));
                        }
                        s => path = Some(s.to_string()),
                    }
                }

                if errors_only && warnings_only {
                    return Err(ParseError::UnexpectedArgument {
                        message: "`--errors-only` and `--warnings-only` cannot be used together"
                            .to_string(),
                        reason: "they select mutually exclusive sets of diagnostics".to_string(),
                        hint: "Use only one of `--errors-only` or `--warnings-only`".to_string(),
                    });
                }

                Ok(Command::Check {
                    path,
                    errors_only,
                    warnings_only,
                    format,
                })
            }

            "help" | "--help" => Ok(Command::Help {
                command: arguments.next(),
            }),

            "version" | "--version" => Ok(Command::Version),

            "add" => match arguments.next() {
                Some(dependency) => {
                    if let Some(extra) = arguments.next() {
                        return Err(ParseError::UnexpectedArgument {
                            message: format!("unexpected argument `{}`", extra),
                            reason: "`lis add` accepts a single dependency".to_string(),
                            hint: "Run `lis add` once per dep".to_string(),
                        });
                    }
                    Ok(Command::Add { dependency })
                }
                None => Err(ParseError::MissingArgument {
                    command: "add",
                    argument: "dependency",
                }),
            },

            "sync" => {
                if let Some(extra) = arguments.next() {
                    return Err(ParseError::UnexpectedArgument {
                        message: format!("unexpected argument `{}`", extra),
                        reason: "`lis sync` takes no arguments".to_string(),
                        hint: "Run `lis sync` from the project root".to_string(),
                    });
                }
                Ok(Command::Sync)
            }

            "lsp" => Ok(Command::Lsp),

            "learn" => Ok(Command::Learn),

            "complete" => Ok(Command::Completions {
                shell: arguments.next(),
            }),

            "doc" => {
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
                    match query {
                        Some(q) => Ok(Command::DocSearch { query: q }),
                        None => Err(ParseError::MissingArgument {
                            command: "doc",
                            argument: "search query",
                        }),
                    }
                } else {
                    if let (Some(q), Some(item)) = (&query, &extra) {
                        return Err(ParseError::UnexpectedArgument {
                            message: format!("unexpected argument `{}`", item),
                            reason: "The `doc` command takes a single query argument".to_string(),
                            hint: format!("Did you mean `lis doc {}.{}`?", q, item),
                        });
                    }
                    Ok(Command::Doc { query })
                }
            }

            "bindgen" => {
                let mut package = None;
                let mut output = None;
                let mut version = None;
                let mut verbose = false;

                while let Some(arg) = arguments.next() {
                    match arg.as_str() {
                        "-v" | "--verbose" => verbose = true,
                        "-o" | "--output" => {
                            output = arguments.next();
                        }
                        s if s.starts_with("-o=") || s.starts_with("--output=") => {
                            output = Some(s.split('=').nth(1).unwrap_or("").to_string());
                        }
                        s if s.starts_with('-') => {
                            return Err(ParseError::UnknownFlag(s.to_string()));
                        }
                        s if package.is_none() => package = Some(s.to_string()),
                        s if version.is_none() => version = Some(s.to_string()),
                        _ => {}
                    }
                }

                match package {
                    Some(package) => Ok(Command::Bindgen {
                        package,
                        output,
                        version,
                        verbose,
                    }),
                    None => Err(ParseError::MissingArgument {
                        command: "bindgen",
                        argument: "package",
                    }),
                }
            }

            _ => Err(ParseError::UnknownCommand(command)),
        }
    }

    pub fn suggest(typo: &str) -> Option<String> {
        const COMMANDS: &[&str] = &[
            "new", "build", "emit", "run", "format", "check", "help", "version", "add", "sync",
            "learn", "doc", "complete", "lsp", "bindgen",
        ];
        let candidates: Vec<String> = COMMANDS.iter().map(|s| s.to_string()).collect();
        diagnostics::infer::find_similar_name(typo, &candidates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(parts: &[&str]) -> Result<Command, ParseError> {
        Command::parse(parts.iter().map(|s| s.to_string()).collect())
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
        let Ok(Command::Emit { path, sourcemap }) = parse(&["lis", "emit", "src", "--sourcemap"])
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
    fn check_output_composes_with_errors_only() {
        let Ok(Command::Check {
            format,
            errors_only,
            warnings_only,
            ..
        }) = parse(&["lis", "check", "--output", "unix", "--errors-only"])
        else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
        assert!(errors_only);
        assert!(!warnings_only);
    }
}
