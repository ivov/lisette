use super::*;

impl TaskState {
    /// Register a Go module (stdlib or third-party). Unlike regular modules,
    /// Go modules export everything as public and do not put their own module
    /// in scope (no self-references like `MyModule.Type`). `cache_path` is the
    /// on-disk typedef location, or `None` for embedded stdlib typedefs.
    pub fn parse_and_register_go_module(
        &mut self,
        store: &mut Store,
        module_id: &str,
        source: &str,
        cache_path: Option<PathBuf>,
        locator: &TypedefLocator,
    ) {
        if store.has(module_id) {
            return;
        }

        store.add_module(module_id);

        if let Some(pkg_name) = extract_package_directive(source) {
            store
                .go_package_names
                .insert(module_id.to_string(), pkg_name);
        }

        let file_id = store.new_file_id();
        let filename = format!("{}.d.lis", module_id.replace('/', "_"));

        let build_result = syntax::build_ast(source, file_id);
        if build_result.failed() {
            for error in &build_result.errors {
                eprintln!("bindgen: error parsing {}: {:?}", filename, error);
            }
        }

        let file = File {
            id: file_id,
            module_id: module_id.to_string(),
            name: filename.clone(),
            display_path: filename,
            source_path: cache_path,
            source: source.to_string(),
            items: build_result.ast,
            file_comment: build_result.file_comment,
        };

        let imports = file.imports();

        let replace_importer =
            module_id
                .strip_prefix("go:")
                .and_then(|pkg| match locator.validate_declaration(pkg) {
                    deps::DeclarationStatus::DeclaredReplacement { .. } => {
                        Some(crate::diagnostics::ReplaceImporter::Module(pkg))
                    }
                    deps::DeclarationStatus::DeclaredLocal { .. } => {
                        Some(crate::diagnostics::ReplaceImporter::Local(pkg))
                    }
                    _ => None,
                });

        for import in &imports {
            if let Some(go_pkg) = import.name.strip_prefix("go:") {
                if matches!(import.alias, Some(syntax::ast::ImportAlias::Blank(_))) {
                    continue;
                }

                let import_module_id = format!("go:{}", go_pkg);

                if store.has(&import_module_id) {
                    continue;
                }

                match locator.find_typedef_content(go_pkg) {
                    deps::TypedefLocatorResult::Found { content, origin } => {
                        self.parse_and_register_go_module(
                            store,
                            &import_module_id,
                            content.as_ref(),
                            origin.into_cache_path(),
                            locator,
                        );
                    }
                    other => {
                        emit_for_locator_result(
                            &other,
                            &GoImportSite {
                                import_name: &import.name,
                                go_pkg,
                                name_span: Some(import.name_span),
                                target: locator.target(),
                                standalone_mode: false,
                                replace_importer,
                            },
                            &self.sink,
                        );
                    }
                }
            }
        }

        store.store_file(file);

        self.with_file_context_mut(
            store,
            FileContext::ImportedTypedef {
                module_id,
                file_id,
                imports: &imports,
            },
            |this, store| {
                let items = std::mem::take(
                    &mut store
                        .get_file_mut(file_id)
                        .expect("file must exist after store_file")
                        .items,
                );
                this.register_types_and_values(store, &items, &Visibility::Public);
            },
        );
    }
}
