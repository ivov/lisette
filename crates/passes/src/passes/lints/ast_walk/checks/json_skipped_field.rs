//! Only the call's own argument is checked. A record reached through a field or
//! a container, or one that embeds another, is a deliberate miss.

use diagnostics::lint::JsonDirection;
use syntax::ast::{Expression, Span, StructFieldDefinition, StructFields};
use syntax::go_names::struct_field_is_exported;
use syntax::program::{DefinitionBody, TypeAttribute, is_internal_package_id};
use syntax::types::{CompoundKind, Symbol, Type, unqualified_name};

use crate::passes::walk::NodeCtx;
use semantics::store::Store;

const JSON_PACKAGE: &str = "go:encoding/json";

/// A record that declares one of these writes its own wire shape.
const MARSHALERS: &[&str] = &["MarshalJSON", "MarshalText"];
const UNMARSHALERS: &[&str] = &["UnmarshalJSON", "UnmarshalText"];

pub fn check_json_skipped_field(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return;
    };
    let Some((index, direction)) = entry_point(callee) else {
        return;
    };
    let Some(argument) = args.get(index) else {
        return;
    };
    let Some(id) = record_id(ctx.store, &argument.get_type()) else {
        return;
    };
    // A Go or prelude type has no Lisette declaration to attribute.
    if ctx
        .store
        .package_for_qualified_name(&id)
        .is_none_or(is_internal_package_id)
    {
        return;
    }
    if declares_hook(ctx.store, &id, direction) {
        return;
    }
    let Some(definition) = ctx.store.get_definition(&id) else {
        return;
    };
    let DefinitionBody::Struct {
        fields: StructFields::Record(fields),
        attributes,
        ..
    } = &definition.body
    else {
        return;
    };
    // Go promotes an embedded struct's fields under its own rules.
    if fields.iter().any(StructFieldDefinition::is_embedded) {
        return;
    }

    let exports_all = attributes.contains(&TypeAttribute::Serialized);
    let hidden: Vec<(&str, Span)> = fields
        .iter()
        .filter(|field| !struct_field_is_exported(field, exports_all))
        .map(|field| (field.name.as_str(), field.name_span))
        .collect();
    if hidden.is_empty() {
        return;
    }

    ctx.sink.push(diagnostics::lint::json_skipped_field(
        &argument.get_span(),
        unqualified_name(&id),
        &hidden,
        direction,
    ));
}

fn entry_point(callee: &Expression) -> Option<(usize, JsonDirection)> {
    let Expression::DotAccess {
        expression: base,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    let entry = match member.as_str() {
        "Marshal" | "MarshalIndent" => (0, JsonDirection::Reads),
        "Unmarshal" => (1, JsonDirection::Fills),
        _ => return None,
    };
    (base.get_type().as_import_namespace() == Some(JSON_PACKAGE)).then_some(entry)
}

fn record_id(store: &Store, ty: &Type) -> Option<Symbol> {
    match store.peel_alias(ty) {
        Type::Nominal { id, .. } => Some(id),
        Type::Compound {
            kind: CompoundKind::Ref,
            args,
            ..
        } => record_id(store, args.first()?),
        _ => None,
    }
}

fn declares_hook(store: &Store, id: &str, direction: JsonDirection) -> bool {
    let hooks = match direction {
        JsonDirection::Reads => MARSHALERS,
        JsonDirection::Fills => UNMARSHALERS,
    };
    hooks
        .iter()
        .any(|method| store.get_method(id, method).is_some())
}
