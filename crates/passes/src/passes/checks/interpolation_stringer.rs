//! Flags f-string interpolation of a first-party struct or enum that has no stringer.

use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, FormatStringPart, Literal};
use syntax::program::{Definition, DefinitionBody, File};
use syntax::types::Type;

use crate::passes::walk::visit_ast;
use semantics::store::Store;

pub(crate) fn run(
    items: &[Expression],
    store: &Store,
    ufcs_methods: &HashSet<(String, String)>,
    sink: &LocalSink,
) {
    visit_ast(
        items,
        &mut |expression| check_expression(expression, store, ufcs_methods, sink),
        &mut |_| {},
    );
}

fn check_expression(
    expression: &Expression,
    store: &Store,
    ufcs_methods: &HashSet<(String, String)>,
    sink: &LocalSink,
) {
    if let Expression::Literal {
        literal: Literal::FormatString(parts),
        ..
    } = expression
    {
        for part in parts {
            if let FormatStringPart::Expression(inner) = part {
                check_interpolation(inner, store, ufcs_methods, sink);
            }
        }
    }
}

fn check_interpolation(
    inner: &Expression,
    store: &Store,
    ufcs_methods: &HashSet<(String, String)>,
    sink: &LocalSink,
) {
    let peeled = store.peel_alias(&inner.get_type());
    let Type::Nominal { id, .. } = &peeled else {
        return;
    };
    let Some(definition) = store.get_definition(id.as_str()) else {
        return;
    };
    if !matches!(
        definition.body,
        DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. }
    ) {
        return;
    }
    if is_foreign(definition, id.as_str(), store)
        || has_stringer(definition, id.as_str(), store, ufcs_methods)
    {
        return;
    }
    sink.push(diagnostics::infer::interpolation_without_stringer(
        id.last_segment(),
        inner.get_span(),
        is_pointer_newtype(definition, store),
    ));
}

fn is_foreign(definition: &Definition, id: &str, store: &Store) -> bool {
    if let Some(module) = store.module_for_qualified_name(id)
        && (module == "prelude" || module.starts_with("go:"))
    {
        return true;
    }
    definition
        .name_span()
        .and_then(|span| store.get_file(span.file_id))
        .is_some_and(File::is_d_lis)
}

fn has_stringer(
    definition: &Definition,
    id: &str,
    store: &Store,
    ufcs_methods: &HashSet<(String, String)>,
) -> bool {
    if is_pointer_newtype(definition, store) {
        return false;
    }
    if definition.is_display() {
        return true;
    }
    let Some(methods) = store.get_own_methods(id) else {
        return false;
    };
    ["string", "String"].iter().any(|name| {
        methods.get(*name).is_some_and(Type::is_stringer_signature)
            && !ufcs_methods.contains(&(id.to_string(), (*name).to_string()))
    })
}

fn is_pointer_newtype(definition: &Definition, store: &Store) -> bool {
    definition.is_pointer_backed_newtype(|id| {
        store
            .get_definition(id)
            .is_some_and(Definition::is_type_alias)
    })
}
