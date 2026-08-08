use crate::analysis::ScriptUnit;
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
    pub(crate) go_pkg: &'a str,
    pub(crate) name_span: Option<Span>,
    pub(crate) target: Target,
    pub(crate) script: Option<ScriptUnit>,
    /// Set when reached through a replaced module's typedef, so the hint names
    /// the right reconciliation command.
    pub(crate) replace_importer: Option<ReplaceImporter<'a>>,
    pub(crate) transitive_importer: Option<&'a str>,
}

pub(crate) fn emit_for_locator_result(
    result: &TypedefLocatorResult,
    site: &GoImportSite,
    sink: &LocalSink,
) -> bool {
    let GoImportSite {
        go_pkg,
        name_span,
        target,
        script,
        replace_importer,
        transitive_importer,
    } = *site;
    let span = name_span.unwrap_or_else(|| Span::new(0, 0, 0));
    match result {
        TypedefLocatorResult::Found { .. } => return true,
        TypedefLocatorResult::UnknownStdlib => {
            emit_unknown_stdlib(go_pkg, span, target, script, sink);
        }
        TypedefLocatorResult::UndeclaredImport => {
            emit_undeclared(
                go_pkg,
                span,
                script,
                replace_importer,
                transitive_importer,
                sink,
            );
        }
        TypedefLocatorResult::InternalPackage { module } => {
            sink.push(diagnostics::package_graph::internal_go_package(
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
                sink.push(diagnostics::package_graph::missing_local_go_typedef(
                    module, span,
                ));
            } else {
                sink.push(diagnostics::package_graph::missing_go_typedef(
                    go_pkg,
                    module,
                    version,
                    replacement_path.as_deref(),
                    span,
                    script.is_some(),
                ));
            }
        }
        TypedefLocatorResult::UnreadableTypedef { path, error } => {
            sink.push(diagnostics::package_graph::unreadable_go_typedef(
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
                sink.push(diagnostics::package_graph::go_toolchain_missing(
                    go_pkg, span,
                ));
            }
            BindgenFailure::InvocationFailed { stderr } => {
                sink.push(diagnostics::package_graph::bindgen_failed(
                    go_pkg,
                    module,
                    version,
                    stderr,
                    span,
                    script.is_some(),
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
        go_pkg,
        name_span,
        target,
        script,
        transitive_importer,
        ..
    } = *site;
    let span = name_span.unwrap_or_else(|| Span::new(0, 0, 0));
    match status {
        DeclarationStatus::Stdlib
        | DeclarationStatus::DeclaredThirdParty { .. }
        | DeclarationStatus::DeclaredReplacement { .. }
        | DeclarationStatus::DeclaredLocal { .. } => true,
        DeclarationStatus::UnknownStdlib => {
            emit_unknown_stdlib(go_pkg, span, target, script, sink);
            false
        }
        DeclarationStatus::UndeclaredImport => {
            emit_undeclared(go_pkg, span, script, None, transitive_importer, sink);
            false
        }
        DeclarationStatus::InternalPackage { module } => {
            sink.push(diagnostics::package_graph::internal_go_package(
                go_pkg, module, span,
            ));
            false
        }
    }
}

fn emit_unknown_stdlib(
    go_pkg: &str,
    span: Span,
    target: Target,
    script: Option<ScriptUnit>,
    sink: &LocalSink,
) {
    if let Some(targets) = stdlib::get_go_stdlib_package_targets(go_pkg) {
        sink.push(diagnostics::package_graph::go_stdlib_unavailable_on_target(
            go_pkg,
            &target.to_string(),
            &stdlib::format_targets(targets),
            span,
        ));
    } else {
        sink.push(diagnostics::package_graph::unknown_go_stdlib_package(
            go_pkg,
            span,
            script.is_some(),
        ));
    }
}

fn emit_undeclared(
    go_pkg: &str,
    span: Span,
    script: Option<ScriptUnit>,
    replace_importer: Option<ReplaceImporter>,
    transitive_importer: Option<&str>,
    sink: &LocalSink,
) {
    if let (Some(_), Some(importer)) = (script, transitive_importer) {
        sink.push(diagnostics::package_graph::script_transitive_go_dependency(
            go_pkg, importer, span,
        ));
    } else if script.is_some() {
        sink.push(diagnostics::package_graph::script_undeclared_dependency(
            span,
        ));
    } else if let Some(ReplaceImporter::Module(replaced_module)) = replace_importer {
        sink.push(
            diagnostics::package_graph::undeclared_go_import_via_replace(
                go_pkg,
                replaced_module,
                span,
            ),
        );
    } else if let Some(ReplaceImporter::Local(local_package)) = replace_importer {
        sink.push(diagnostics::package_graph::undeclared_go_import_via_local(
            go_pkg,
            local_package,
            span,
        ));
    } else {
        sink.push(diagnostics::package_graph::undeclared_go_import(
            go_pkg, span,
        ));
    }
}
