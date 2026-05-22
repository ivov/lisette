use diagnostics::render::OutputFormat;

#[derive(Debug)]
pub enum Command {
    New {
        name: String,
    },
    Build {
        path: Option<String>,
        debug: bool,
    },
    Run {
        target: Option<String>,
        args: Vec<String>,
        debug: bool,
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

fn parse_format(value: &str) -> Result<OutputFormat, ParseError> {
    match value {
        "unix" => Ok(OutputFormat::Unix),
        other => Err(ParseError::UnexpectedArgument {
            message: format!("unexpected value `{}` for `--format`", other),
            reason: "`--format` accepts `unix`".to_string(),
            hint: "Use `lis check --format unix`".to_string(),
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
                let mut path = None;
                let mut debug = false;

                for arg in arguments {
                    match arg.as_str() {
                        "--debug" => debug = true,
                        s if s.starts_with('-') => {
                            return Err(ParseError::UnknownFlag(s.to_string()));
                        }
                        s => path = Some(s.to_string()),
                    }
                }

                Ok(Command::Build { path, debug })
            }

            "run" | "r" => {
                let mut target = None;
                let mut args = Vec::new();
                let mut debug = false;
                let mut found_separator = false;

                for arg in arguments {
                    if found_separator {
                        args.push(arg);
                    } else if arg == "--" {
                        found_separator = true;
                    } else if arg == "--debug" {
                        debug = true;
                    } else {
                        target = Some(arg);
                    }
                }

                Ok(Command::Run {
                    target,
                    args,
                    debug,
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
                        "--format" => {
                            let Some(value) = arguments.next() else {
                                return Err(ParseError::MissingArgument {
                                    command: "check",
                                    argument: "--format <value>",
                                });
                            };
                            format = parse_format(&value)?;
                        }
                        s if s.starts_with("--format=") => {
                            format = parse_format(s.split_once('=').unwrap().1)?;
                        }
                        s if s.starts_with('-') => {
                            return Err(ParseError::UnknownFlag(s.to_string()));
                        }
                        s => path = Some(s.to_string()),
                    }
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
            "new", "build", "run", "format", "check", "help", "version", "add", "sync", "learn",
            "doc", "complete", "lsp", "bindgen",
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
    fn check_format_unix_space_form() {
        let Ok(Command::Check { format, .. }) = parse(&["lis", "check", "--format", "unix"]) else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
    }

    #[test]
    fn check_format_unix_equals_form() {
        let Ok(Command::Check { format, .. }) = parse(&["lis", "check", "--format=unix"]) else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
    }

    #[test]
    fn check_format_missing_value() {
        assert!(matches!(
            parse(&["lis", "check", "--format"]),
            Err(ParseError::MissingArgument {
                command: "check",
                argument: "--format <value>",
            })
        ));
    }

    #[test]
    fn check_format_invalid_value() {
        assert!(matches!(
            parse(&["lis", "check", "--format", "json"]),
            Err(ParseError::UnexpectedArgument { .. })
        ));
    }

    #[test]
    fn check_format_composes_with_errors_only() {
        let Ok(Command::Check {
            format,
            errors_only,
            warnings_only,
            ..
        }) = parse(&["lis", "check", "--format", "unix", "--errors-only"])
        else {
            panic!("expected Check command");
        };
        assert_eq!(format, OutputFormat::Unix);
        assert!(errors_only);
        assert!(!warnings_only);
    }
}
