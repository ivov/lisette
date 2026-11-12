use super::*;
use syntax::ParseError;

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

    let result = entry.parse_mode.parse(&entry.source, ENTRY_FILE_ID);

    if result.status != FileParseStatus::Failed {
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

    let (file, errors) = File::from_parse(
        ENTRY_FILE_ID,
        ENTRY_PACKAGE_ID,
        entry.filename.clone(),
        entry.display_path,
        entry.source,
        result,
    );
    store.store_file(file);

    EntryRegistration {
        filename: Some(entry.filename),
        errors,
    }
}

/// Loads every other `.lis` file in the entry package's folder as a sibling file.
pub(super) fn load_sibling_files(
    store: &mut Store,
    sink: &LocalSink,
    loader: &dyn Loader,
    entry_filename: Option<&str>,
    include_tests: bool,
    mode: ParseMode,
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
        let result = mode.parse(&content.source, file_id);
        let (file, errors) = File::from_parse(
            file_id,
            ENTRY_PACKAGE_ID,
            filename,
            content.display_path,
            content.source,
            result,
        );
        sink.extend_parse_errors(errors);
        store.store_file(file);
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
                CompilePhase::Check => discovered
                    .production_packages()
                    .map(|package_id| package_id.as_str().into())
                    .collect(),
                CompilePhase::Emit | CompilePhase::Test => Vec::new(),
            };
            if include_test_roots {
                additional.extend(
                    discovered
                        .test_roots()
                        .map(|package_id| package_id.as_str().into()),
                );
            }
            Roots {
                primary: vec![entry_package.into()],
                additional,
            }
        }
        ProjectKind::Library => {
            let mut additional = Vec::new();
            if include_test_roots {
                additional.extend(
                    discovered
                        .test_roots()
                        .map(|package_id| package_id.as_str().into()),
                );
            }
            additional.push(entry_package.into());
            Roots {
                primary: discovered
                    .production_packages()
                    .map(|package_id| package_id.as_str().into())
                    .collect(),
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
        let parsed = ParseMode::Strict.parse("fn valid() {}\nfn incomplete(", ENTRY_FILE_ID);

        assert!(parsed.ast.is_empty() && parsed.status == FileParseStatus::Failed);
    }

    #[test]
    fn recovering_entry_parsing_keeps_partial_ast() {
        let parsed = ParseMode::Recover.parse("fn valid() {}\nfn incomplete(", ENTRY_FILE_ID);

        assert!(!parsed.ast.is_empty() && parsed.status == FileParseStatus::Recovered);
    }

    #[test]
    fn recovering_entry_parsing_still_rejects_lex_errors() {
        let parsed = ParseMode::Recover.parse("fn main() { \"unterminated }", ENTRY_FILE_ID);

        assert!(
            parsed.status == FileParseStatus::Failed
                && parsed
                    .errors
                    .first()
                    .is_some_and(|error| error.code.starts_with("lex."))
        );
    }
}
