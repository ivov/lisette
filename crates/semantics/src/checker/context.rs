use super::resolution::{ImportState, PrefixedImport};
use super::state::Cursor;
use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileContext<'a> {
    Standard {
        module_id: &'a str,
        file_id: u32,
        imports: &'a [FileImport],
    },
    ImportedTypedef {
        module_id: &'a str,
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
                module_id,
                file_id,
                imports,
            }
            | Self::ImportedTypedef {
                module_id,
                file_id,
                imports,
            } => (module_id, file_id, imports),
            Self::Prelude => (
                crate::prelude::PRELUDE_MODULE_ID,
                crate::prelude::PRELUDE_FILE_ID,
                &[],
            ),
            Self::TestPrelude { file_id } => (crate::prelude::TEST_PRELUDE_MODULE_ID, file_id, &[]),
        }
    }
}

struct SavedFileContext {
    cursor: Cursor,
    scopes: Scopes,
    imports: ImportState,
}

impl TaskState {
    pub(super) fn with_module_cursor<T>(
        &mut self,
        module_id: &str,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        if self.cursor.module_id == module_id {
            return f(self);
        }

        let previous_module_id = std::mem::replace(&mut self.cursor.module_id, module_id.into());
        let result = f(self);
        self.cursor.module_id = previous_module_id;
        result
    }

    pub(super) fn with_file_context<T>(
        &mut self,
        store: &Store,
        context: FileContext<'_>,
        f: impl FnOnce(&mut Self, &Store) -> T,
    ) -> T {
        let saved = self.enter_file_context(store, context);
        let result = f(self, store);
        self.exit_file_context(saved);
        result
    }

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

    fn enter_file_context(&mut self, store: &Store, context: FileContext<'_>) -> SavedFileContext {
        let (module_id, file_id, imports) = context.parts();
        let saved = SavedFileContext {
            cursor: std::mem::replace(
                &mut self.cursor,
                Cursor {
                    module_id: module_id.into(),
                    file_id: Some(file_id),
                },
            ),
            scopes: std::mem::take(&mut self.scopes),
            imports: std::mem::take(&mut self.imports),
        };

        match context {
            FileContext::Standard { .. } => {
                self.put_prelude_in_scope(store);
                if self.current_file_is_test(store) {
                    self.put_unprefixed_module_in_scope(
                        store,
                        crate::prelude::TEST_PRELUDE_MODULE_ID,
                    );
                }
                self.put_unprefixed_module_in_scope(store, module_id);
            }
            FileContext::ImportedTypedef { .. } => {
                self.put_prelude_in_scope(store);
                let self_alias = store
                    .go_package_names
                    .get(module_id)
                    .cloned()
                    .unwrap_or_else(|| go_import_default_name(module_id).to_string());
                self.imports.prefixed.insert(
                    self_alias,
                    PrefixedImport::LookupOnly {
                        module_id: module_id.into(),
                    },
                );
            }
            FileContext::Prelude => {
                self.put_unprefixed_module_in_scope(store, module_id);
            }
            FileContext::TestPrelude { .. } => {
                self.put_prelude_in_scope(store);
                self.put_unprefixed_module_in_scope(store, module_id);
            }
        }
        self.put_imported_modules_in_scope(store, imports);

        saved
    }

    fn exit_file_context(&mut self, saved: SavedFileContext) {
        self.cursor = saved.cursor;
        self.scopes = saved.scopes;
        self.imports = saved.imports;
    }

    /// Run a closure speculatively: if it returns `Err`, type variable bindings
    /// and diagnostics produced during the closure are rolled back together.
    pub(super) fn speculatively<T, E>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, E>,
    ) -> Result<T, E> {
        let diagnostics_before = self.sink.checkpoint();
        self.env.begin_speculation();
        let result = f(self);
        let rollback = result.is_err();
        self.env.end_speculation(rollback);
        if rollback {
            self.sink.rollback(diagnostics_before);
        }
        result
    }

    pub(super) fn without_diagnostics<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let diagnostics_before = self.sink.checkpoint();
        let result = f(self);
        self.sink.rollback(diagnostics_before);
        result
    }

    pub(super) fn tracking_diagnostics<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, bool) {
        let checkpoint = self.sink.checkpoint();
        let result = f(self);
        (result, self.sink.has_changed_since(checkpoint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_speculation_rolls_back_diagnostics() {
        let mut task = TaskState::with_fresh_allocator();
        task.sink
            .push(diagnostics::LisetteDiagnostic::error("before"));

        let result: Result<(), ()> = task.speculatively(|task| {
            task.sink
                .push(diagnostics::LisetteDiagnostic::error("speculative"));
            Err(())
        });

        assert!(result.is_err());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "before");
    }

    #[test]
    fn successful_speculation_keeps_diagnostics() {
        let mut task = TaskState::with_fresh_allocator();

        let result: Result<(), ()> = task.speculatively(|task| {
            task.sink
                .push(diagnostics::LisetteDiagnostic::error("reported"));
            Ok(())
        });

        assert!(result.is_ok());
        let diagnostics = task.sink.into_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].plain_message(), "reported");
    }
}
