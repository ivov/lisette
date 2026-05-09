use deps::{BindgenFailure, DeclarationStatus, TypedefLocatorResult};
use diagnostics::LocalSink;
use stdlib::Target;
use syntax::ast::Span;

pub fn emit_for_locator_result(
    result: &TypedefLocatorResult,
    import_name: &str,
    go_pkg: &str,
    name_span: Option<Span>,
    target: Target,
    standalone_mode: bool,
    sink: &LocalSink,
) -> bool {
    let span = name_span.unwrap_or_else(|| Span::new(0, 0, 0));
    match result {
        TypedefLocatorResult::Found { .. } => return true,
        TypedefLocatorResult::UnknownStdlib => {
            emit_unknown_stdlib(import_name, go_pkg, span, target, standalone_mode, sink);
        }
        TypedefLocatorResult::UndeclaredImport => {
            emit_undeclared(import_name, go_pkg, span, standalone_mode, sink);
        }
        TypedefLocatorResult::MissingTypedef { module, version } => {
            sink.push(diagnostics::module_graph::missing_go_typedef(
                go_pkg, module, version, span,
            ));
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
pub fn emit_for_declaration_status(
    status: &DeclarationStatus,
    import_name: &str,
    go_pkg: &str,
    name_span: Span,
    target: Target,
    standalone_mode: bool,
    sink: &LocalSink,
) -> bool {
    match status {
        DeclarationStatus::Stdlib | DeclarationStatus::DeclaredThirdParty { .. } => true,
        DeclarationStatus::UnknownStdlib => {
            emit_unknown_stdlib(
                import_name,
                go_pkg,
                name_span,
                target,
                standalone_mode,
                sink,
            );
            false
        }
        DeclarationStatus::UndeclaredImport => {
            emit_undeclared(import_name, go_pkg, name_span, standalone_mode, sink);
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
            false,
            standalone_mode,
            None,
        ));
    }
}

fn emit_undeclared(
    import_name: &str,
    go_pkg: &str,
    span: Span,
    standalone_mode: bool,
    sink: &LocalSink,
) {
    if standalone_mode {
        sink.push(diagnostics::module_graph::module_not_found(
            import_name,
            span,
            false,
            true,
            None,
        ));
    } else {
        sink.push(diagnostics::module_graph::undeclared_go_import(
            go_pkg, span,
        ));
    }
}
