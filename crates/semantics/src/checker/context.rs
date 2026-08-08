use super::resolution::{ImportState, PrefixedImport};
use super::state::Cursor;
use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileContext<'a> {
    Standard {
        package_id: &'a str,
        file_id: u32,
        imports: &'a [FileImport],
    },
    ImportedTypedef {
        package_id: &'a str,
        file_id: u32,
        imports: &'a [FileImport],
    },
    Prelude,
    TestPrelude {
        file_id: u32,
    },
}

impl<'a> FileContext<'a> {
    fn parts(self) -> (&'a str, u32, &'a [FileImport]) {
        match self {
            Self::Standard {
                package_id,
                file_id,
                imports,
            }
            | Self::ImportedTypedef {
                package_id,
                file_id,
                imports,
            } => (package_id, file_id, imports),
            Self::Prelude => (
                crate::prelude::PRELUDE_PACKAGE_ID,
                crate::prelude::PRELUDE_FILE_ID,
                &[],
            ),
            Self::TestPrelude { file_id } => {
                (crate::prelude::TEST_PRELUDE_PACKAGE_ID, file_id, &[])
            }
        }
    }
}

pub(super) struct SavedFileContext {
    cursor: Cursor,
    scopes: Scopes,
    imports: ImportState,
}

impl TaskState {
    pub(crate) fn with_file_context_mut<T>(
        &mut self,
        store: &mut Store,
        context: FileContext<'_>,
        f: impl FnOnce(&mut Self, &mut Store) -> T,
    ) -> T {
        let saved = self.enter_file_context(&*store, context);
        let result = f(self, store);
        self.exit_file_context(saved);
        result
    }

    pub(super) fn enter_file_context(
        &mut self,
        store: &Store,
        context: FileContext<'_>,
    ) -> SavedFileContext {
        let (package_id, file_id, imports) = context.parts();
        let saved = SavedFileContext {
            cursor: std::mem::replace(&mut self.cursor, Cursor::file(package_id, file_id)),
            scopes: std::mem::take(&mut self.scopes),
            imports: std::mem::take(&mut self.imports),
        };

        match context {
            FileContext::Standard { .. } => {
                self.put_prelude_in_scope(store);
                if self.current_file_is_test(store) {
                    self.put_unprefixed_package_in_scope(
                        store,
                        crate::prelude::TEST_PRELUDE_PACKAGE_ID,
                    );
                }
                self.put_unprefixed_package_in_scope(store, package_id);
            }
            FileContext::ImportedTypedef { .. } => {
                self.put_prelude_in_scope(store);
                let self_alias = store
                    .go_package_names
                    .get(package_id)
                    .cloned()
                    .unwrap_or_else(|| go_import_default_name(package_id).to_string());
                self.imports.prefixed.insert(
                    self_alias,
                    PrefixedImport::LookupOnly {
                        package_id: package_id.into(),
                    },
                );
            }
            FileContext::Prelude => {
                self.put_unprefixed_package_in_scope(store, package_id);
            }
            FileContext::TestPrelude { .. } => {
                self.put_prelude_in_scope(store);
                self.put_unprefixed_package_in_scope(store, package_id);
            }
        }
        self.put_imported_packages_in_scope(store, imports);

        saved
    }

    pub(super) fn exit_file_context(&mut self, saved: SavedFileContext) {
        self.cursor = saved.cursor;
        self.scopes = saved.scopes;
        self.imports = saved.imports;
    }

    pub(super) fn without_diagnostics<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let diagnostics_before = self.sink.checkpoint();
        let result = f(self);
        self.sink.rollback(diagnostics_before);
        result
    }
}
