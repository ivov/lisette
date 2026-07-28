use deps::{BindgenFailure, DeclarationStatus, TypedefLocatorResult};
use diagnostics::LocalSink;
use stdlib::Target;
use syntax::ast::Span;

/// The replaced module whose typedef the current import was reached through.
#[derive(Clone, Copy)]
pub(crate) enum ReplaceImporter<'a> {
    Module(&'a str),
    Local(&'a str),
}

/// The import site a typedef-resolution diagnostic refers to.
pub(crate) struct GoImportSite<'a> {
    pub(crate) import_name: &'a str,
    pub(crate) go_pkg: &'a str,
    pub(crate) name_span: Option<Span>,
    pub(crate) target: Target,
    pub(crate) standalone_mode: bool,
    /// Set when reached through a replaced module's typedef, so the hint names
    /// the right reconciliation command.
    pub(crate) replace_importer: Option<ReplaceImporter<'a>>,
}

pub(crate) fn emit_for_locator_result(
    result: &TypedefLocatorResult,
    site: &GoImportSite,
    sink: &LocalSink,
) -> bool {
    let GoImportSite {
        import_name,
        go_pkg,
        name_span,
        target,
        standalone_mode,
        replace_importer,
    } = *site;
    let span = name_span.unwrap_or_else(|| Span::new(0, 0, 0));
    match result {
        TypedefLocatorResult::Found { .. } => return true,
        TypedefLocatorResult::UnknownStdlib => {
            emit_unknown_stdlib(import_name, go_pkg, span, target, standalone_mode, sink);
        }
        TypedefLocatorResult::UndeclaredImport => {
            emit_undeclared(
                import_name,
                go_pkg,
                span,
                standalone_mode,
                replace_importer,
                sink,
            );
        }
        TypedefLocatorResult::InternalPackage { module } => {
            sink.push(diagnostics::module_graph::internal_go_package(
                go_pkg, module, span,
            ));
        }
        TypedefLocatorResult::MissingTypedef {
            module,
            version,
            replacement_path,
            local,
        } => {
            if *local {
                sink.push(diagnostics::module_graph::missing_local_go_typedef(
                    module, span,
                ));
            } else {
                sink.push(diagnostics::module_graph::missing_go_typedef(
                    go_pkg,
                    module,
                    version,
                    replacement_path.as_deref(),
                    span,
                ));
            }
        }
        TypedefLocatorResult::UnreadableTypedef { path, error } => {
            sink.push(diagnostics::module_graph::unreadable_go_typedef(
                path, error, span,
            ));
        }
        TypedefLocatorResult::BindgenFailed {
            module,
            version,
            kind,
            ..
        } => match kind {
            BindgenFailure::GoToolchainMissing => {
                sink.push(diagnostics::module_graph::go_toolchain_missing(
                    go_pkg, span,
                ));
            }
            BindgenFailure::InvocationFailed { stderr } => {
                sink.push(diagnostics::module_graph::bindgen_failed(
                    go_pkg, module, version, stderr, span,
                ));
            }
        },
    }
    false
}

/// Emit a diagnostic for a non-OK `DeclarationStatus`; returns `true` if OK.
pub(crate) fn emit_for_declaration_status(
    status: &DeclarationStatus,
    site: &GoImportSite,
    sink: &LocalSink,
) -> bool {
    let GoImportSite {
        import_name,
        go_pkg,
        name_span,
        target,
        standalone_mode,
        ..
    } = *site;
    let span = name_span.unwrap_or_else(|| Span::new(0, 0, 0));
    match status {
        DeclarationStatus::Stdlib
        | DeclarationStatus::DeclaredThirdParty { .. }
        | DeclarationStatus::DeclaredReplacement { .. }
        | DeclarationStatus::DeclaredLocal { .. } => true,
        DeclarationStatus::UnknownStdlib => {
            emit_unknown_stdlib(import_name, go_pkg, span, target, standalone_mode, sink);
            false
        }
        DeclarationStatus::UndeclaredImport => {
            emit_undeclared(import_name, go_pkg, span, standalone_mode, None, sink);
            false
        }
        DeclarationStatus::InternalPackage { module } => {
            sink.push(diagnostics::module_graph::internal_go_package(
                go_pkg, module, span,
            ));
            false
        }
    }
}

fn emit_unknown_stdlib(
    import_name: &str,
    go_pkg: &str,
    span: Span,
    target: Target,
    standalone_mode: bool,
    sink: &LocalSink,
) {
    if let Some(targets) = stdlib::get_go_stdlib_package_targets(go_pkg) {
        sink.push(diagnostics::module_graph::go_stdlib_unavailable_on_target(
            go_pkg,
            &target.to_string(),
            &stdlib::format_targets(targets),
            span,
        ));
    } else {
        sink.push(diagnostics::module_graph::module_not_found(
            import_name,
            span,
            if standalone_mode {
                diagnostics::module_graph::MissingModuleReason::Standalone
            } else {
                diagnostics::module_graph::MissingModuleReason::NotFound
            },
        ));
    }
}

fn emit_undeclared(
    import_name: &str,
    go_pkg: &str,
    span: Span,
    standalone_mode: bool,
    replace_importer: Option<ReplaceImporter>,
    sink: &LocalSink,
) {
    if standalone_mode {
        sink.push(diagnostics::module_graph::module_not_found(
            import_name,
            span,
            diagnostics::module_graph::MissingModuleReason::Standalone,
        ));
    } else if let Some(ReplaceImporter::Module(replaced_module)) = replace_importer {
        sink.push(diagnostics::module_graph::undeclared_go_import_via_replace(
            go_pkg,
            replaced_module,
            span,
        ));
    } else if let Some(ReplaceImporter::Local(local_module)) = replace_importer {
        sink.push(diagnostics::module_graph::undeclared_go_import_via_local(
            go_pkg,
            local_module,
            span,
        ));
    } else {
        sink.push(diagnostics::module_graph::undeclared_go_import(
            go_pkg, span,
        ));
    }
}
