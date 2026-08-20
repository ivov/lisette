use super::*;
use syntax::ParseError;
use syntax::ast::Expression;

pub(super) struct EntryRegistration {
    filename: Option<String>,
    errors: Vec<ParseError>,
}

impl EntryRegistration {
    pub(super) fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    pub(super) fn into_errors(self) -> Vec<ParseError> {
        self.errors
    }
}

struct ParsedEntry {
    ast: Vec<Expression>,
    file_comment: Option<String>,
    status: FileParseStatus,
    errors: Vec<ParseError>,
}

/// Parses and registers the entry file (`main.lis`, or the library root).
pub(super) fn register_entry_file(
    store: &mut Store,
    sink: &LocalSink,
    entry: Option<EntryFile>,
    include_tests: bool,
) -> EntryRegistration {
    let Some(entry) = entry else {
        return EntryRegistration {
            filename: None,
            errors: Vec::new(),
        };
    };

    let ParsedEntry {
        ast,
        file_comment,
        status,
        errors,
    } = parse_entry_file(&entry.source, entry.parse_mode);

    if status != FileParseStatus::Failed {
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

    store.store_file(File {
        id: ENTRY_FILE_ID,
        package_id: ENTRY_PACKAGE_ID.to_string(),
        parse_status: status,
        name: entry.filename.clone(),
        display_path: entry.display_path,
        source_path: None,
        source: entry.source,
        items: ast,
        file_comment,
    });

    EntryRegistration {
        filename: Some(entry.filename),
        errors,
    }
}

fn parse_entry_file(source: &str, mode: EntryParseMode) -> ParsedEntry {
    let result = match mode {
        EntryParseMode::Strict => syntax::build_ast(source, ENTRY_FILE_ID),
        EntryParseMode::Recover => syntax::build_ast_recovering(source, ENTRY_FILE_ID),
    };
    ParsedEntry {
        ast: result.ast,
        file_comment: result.file_comment,
        status: result.status,
        errors: result.errors,
    }
}

/// Loads every other `.lis` file in the entry package's folder as a sibling file.
pub(super) fn load_sibling_files(
    store: &mut Store,
    sink: &LocalSink,
    loader: &dyn Loader,
    entry_filename: Option<&str>,
    include_tests: bool,
    recover: bool,
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
        let result = if recover {
            syntax::build_ast_recovering(&content.source, file_id)
        } else {
            syntax::build_ast(&content.source, file_id)
        };
        sink.extend_parse_errors(result.errors);
        store.store_file(File {
            id: file_id,
            package_id: ENTRY_PACKAGE_ID.to_string(),
            parse_status: result.status,
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
    graph_result: &PackageGraphResult,
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

        assert!(parsed.ast.is_empty() && parsed.status == FileParseStatus::Failed);
    }

    #[test]
    fn recovering_entry_parsing_keeps_partial_ast() {
        let parsed = parse_entry_file("fn valid() {}\nfn incomplete(", EntryParseMode::Recover);

        assert!(!parsed.ast.is_empty() && parsed.status == FileParseStatus::Recovered);
    }

    #[test]
    fn recovering_entry_parsing_still_rejects_lex_errors() {
        let parsed = parse_entry_file("fn main() { \"unterminated }", EntryParseMode::Recover);

        assert!(
            parsed.status == FileParseStatus::Failed
                && parsed
                    .errors
                    .first()
                    .is_some_and(|error| error.code.starts_with("lex."))
        );
    }
}
