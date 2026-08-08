use super::*;

pub(super) enum EntryRegistration {
    Absent,
    Present {
        filename: String,
        parse: EntryParseOutcome,
    },
}

impl EntryRegistration {
    pub(super) fn filename(&self) -> Option<&str> {
        match self {
            Self::Absent => None,
            Self::Present { filename, .. } => Some(filename),
        }
    }

    pub(super) fn parse_failed(&self) -> bool {
        match self {
            Self::Absent => false,
            Self::Present { parse, .. } => parse.is_failed(),
        }
    }

    pub(super) fn into_parse(self) -> EntryParseOutcome {
        match self {
            Self::Absent => EntryParseOutcome::Clean,
            Self::Present { parse, .. } => parse,
        }
    }
}

struct ParsedEntry {
    ast: Vec<syntax::ast::Expression>,
    file_comment: Option<String>,
    outcome: EntryParseOutcome,
}

/// Parses and registers the entry file (`main.lis`, or the library root).
pub(super) fn register_entry_file(
    store: &mut Store,
    sink: &LocalSink,
    entry: Option<EntryFile>,
    include_tests: bool,
) -> EntryRegistration {
    let Some(entry) = entry else {
        return EntryRegistration::Absent;
    };

    let ParsedEntry {
        ast,
        file_comment,
        outcome,
    } = parse_entry_file(&entry.source, entry.parse_mode);

    if !outcome.is_failed() {
        if entry.filename.ends_with("_test.lis") {
            sink.push(diagnostics::package_graph::wrong_test_file_suffix(
                &entry.display_path,
            ));
        } else if entry.filename.ends_with(".test.lis") && !include_tests {
            sink.push(diagnostics::package_graph::cannot_emit_test_file(
                &entry.display_path,
            ));
        }
    }
    store.store_entry_file(
        &entry.filename,
        &entry.display_path,
        &entry.source,
        ast,
        file_comment,
    );

    EntryRegistration::Present {
        filename: entry.filename,
        parse: outcome,
    }
}

fn parse_entry_file(source: &str, mode: EntryParseMode) -> ParsedEntry {
    match mode {
        EntryParseMode::Strict => {
            let ParseResult {
                ast,
                errors,
                file_comment,
                ..
            } = syntax::build_ast(source, ENTRY_FILE_ID);
            let outcome = if errors.is_empty() {
                EntryParseOutcome::Clean
            } else {
                EntryParseOutcome::Failed(errors)
            };
            ParsedEntry {
                ast,
                file_comment,
                outcome,
            }
        }
        EntryParseMode::Recover => {
            let lex_result = Lexer::new(source, ENTRY_FILE_ID).lex();
            if lex_result.failed() {
                return ParsedEntry {
                    ast: Vec::new(),
                    file_comment: None,
                    outcome: EntryParseOutcome::Failed(lex_result.errors),
                };
            }

            let ParseResult {
                ast,
                errors,
                file_comment,
                ..
            } = Parser::new(lex_result.tokens, source).parse();
            let outcome = if errors.is_empty() {
                EntryParseOutcome::Clean
            } else {
                EntryParseOutcome::Recovered(errors)
            };
            ParsedEntry {
                ast,
                file_comment,
                outcome,
            }
        }
    }
}

/// Loads every other `.lis` file in the entry package's folder as a sibling file.
pub(super) fn load_sibling_files(
    store: &mut Store,
    sink: &LocalSink,
    loader: &dyn Loader,
    entry_filename: Option<&str>,
    include_tests: bool,
) {
    for (filename, content) in loader.scan_folder(ENTRY_PACKAGE_ID) {
        if Some(filename.as_str()) == entry_filename {
            continue;
        }
        if filename.ends_with("_test.lis") {
            sink.push(diagnostics::package_graph::wrong_test_file_suffix(
                &content.display_path,
            ));
            continue;
        }
        if !filename.ends_with(".lis")
            || filename.ends_with(".d.lis")
            || (filename.ends_with(".test.lis") && !include_tests)
        {
            continue;
        }
        let file_id = store.new_file_id();
        let result = syntax::build_ast(&content.source, file_id);
        sink.extend_parse_errors(result.errors);
        store.store_file(File {
            id: file_id,
            package_id: ENTRY_PACKAGE_ID.to_string(),
            name: filename,
            display_path: content.display_path,
            source_path: None,
            source: content.source,
            items: result.ast,
            file_comment: result.file_comment,
        });
    }
}

pub(super) fn compute_roots(
    project_kind: ProjectKind,
    compile_phase: CompilePhase,
    discovered: &DiscoveredPackages,
    entry_package: String,
) -> Roots {
    let include_test_roots = compile_phase.includes_tests();
    match project_kind {
        ProjectKind::Binary => {
            let mut additional = match compile_phase {
                CompilePhase::Check => discovered.production_packages().cloned().collect(),
                CompilePhase::Emit | CompilePhase::Test => Vec::new(),
            };
            if include_test_roots {
                additional.extend(discovered.test_roots().cloned());
            }
            Roots {
                primary: vec![entry_package],
                additional,
            }
        }
        ProjectKind::Library => {
            let mut additional = Vec::new();
            if include_test_roots {
                additional.extend(discovered.test_roots().cloned());
            }
            additional.push(entry_package);
            Roots {
                primary: discovered.production_packages().cloned().collect(),
                additional,
            }
        }
    }
}

/// Production packages the primary roots never reached.
pub(super) fn find_unreachable_packages(
    discovered: &DiscoveredPackages,
    graph_result: &crate::package_graph::PackageGraphResult,
) -> Vec<String> {
    let mut unreachable: Vec<String> = discovered
        .production_packages()
        .filter(|m| !graph_result.primary_reachable.contains(m.as_str()))
        .cloned()
        .collect();
    unreachable.sort();
    unreachable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_entry_parsing_rejects_partial_ast() {
        let parsed = parse_entry_file("fn valid() {}\nfn incomplete(", EntryParseMode::Strict);

        assert!(parsed.ast.is_empty() && matches!(parsed.outcome, EntryParseOutcome::Failed(_)));
    }

    #[test]
    fn recovering_entry_parsing_keeps_partial_ast() {
        let parsed = parse_entry_file("fn valid() {}\nfn incomplete(", EntryParseMode::Recover);

        assert!(
            !parsed.ast.is_empty() && matches!(parsed.outcome, EntryParseOutcome::Recovered(_))
        );
    }

    #[test]
    fn recovering_entry_parsing_still_rejects_lex_errors() {
        let parsed = parse_entry_file("fn main() { \"unterminated }", EntryParseMode::Recover);

        assert!(matches!(
            parsed.outcome,
            EntryParseOutcome::Failed(errors)
                if errors.first().is_some_and(|error| error.code.starts_with("lex."))
        ));
    }
}
