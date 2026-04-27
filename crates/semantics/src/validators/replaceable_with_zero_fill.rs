use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, Literal, Span, StructFieldAssignment, StructSpread};
use syntax::program::Definition;
use syntax::types::{SubstitutionMap, Type, substitute};

use crate::checker::infer::expressions::struct_call::has_zero;
use crate::store::Store;

const ZERO_FIELD_THRESHOLD: usize = 3;

pub(super) fn run(
    typed_ast: &[Expression],
    source: &str,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    for expression in typed_ast {
        walk(expression, source, module_id, store, sink);
    }
}

fn walk(expression: &Expression, source: &str, module_id: &str, store: &Store, sink: &LocalSink) {
    if let Expression::StructCall {
        name,
        field_assignments,
        spread,
        ty,
        span,
        ..
    } = expression
        && matches!(spread, StructSpread::None)
    {
        let zero_count = field_assignments
            .iter()
            .filter(|f| is_obvious_zero(&f.value))
            .count();
        if zero_count >= ZERO_FIELD_THRESHOLD
            && let Some(unspecified) = unspecified_fields(store, ty, name, field_assignments)
            && unspecified.is_empty()
            && rewrite_would_typecheck(store, ty, name, field_assignments, module_id)
        {
            let kept = render_kept_fields(source, field_assignments);
            let owner_span = Span::new(span.file_id, span.byte_offset, name.len() as u32);
            sink.push(diagnostics::lint::replaceable_with_zero_fill(
                &owner_span,
                &kept,
                name,
            ));
        }
    }

    for child in expression.children() {
        walk(child, source, module_id, store, sink);
    }
}

fn render_kept_fields(source: &str, fields: &[StructFieldAssignment]) -> String {
    fields
        .iter()
        .filter(|f| !is_obvious_zero(&f.value))
        .map(|f| {
            let value_span = f.value.get_span();
            let start = f.name_span.byte_offset as usize;
            let end = (value_span.byte_offset + value_span.byte_length) as usize;
            source
                .get(start..end)
                .map(|s| s.to_string())
                .unwrap_or_else(|| f.name.to_string())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_obvious_zero(value: &Expression) -> bool {
    match value {
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { value, .. } => *value == 0,
            Literal::Float { value, .. } => *value == 0.0,
            Literal::Boolean(b) => !*b,
            Literal::String { value, .. } => value.is_empty(),
            _ => false,
        },
        Expression::Identifier { value, .. } => value.as_str() == "None",
        _ => false,
    }
}

fn is_go_imported(ty: &Type) -> bool {
    let Type::Nominal { id, .. } = ty.strip_refs() else {
        return false;
    };
    id.as_str().starts_with("go:")
}

fn struct_module(ty: &Type) -> Option<String> {
    let Type::Nominal { id, .. } = ty.strip_refs() else {
        return None;
    };
    id.as_str().split_once('.').map(|(m, _)| m.to_string())
}

fn rewrite_would_typecheck(
    store: &Store,
    ty: &Type,
    name: &str,
    field_assignments: &[StructFieldAssignment],
    from_module: &str,
) -> bool {
    if is_go_imported(ty) {
        return true;
    }
    let Some(omitted) = post_rewrite_unspecified_fields(store, ty, name, field_assignments) else {
        return false;
    };
    let is_cross_module = struct_module(ty).is_some_and(|m| m.as_str() != from_module);
    omitted
        .iter()
        .all(|f| (!is_cross_module || f.is_public) && has_zero(store, &f.ty, from_module).is_ok())
}

struct OmittedField {
    ty: Type,
    is_public: bool,
}

fn unspecified_fields(
    store: &Store,
    ty: &Type,
    name: &str,
    field_assignments: &[StructFieldAssignment],
) -> Option<Vec<OmittedField>> {
    let assigned: HashSet<&str> = field_assignments.iter().map(|f| f.name.as_str()).collect();
    fields_filtered(store, ty, name, &assigned)
}

fn post_rewrite_unspecified_fields(
    store: &Store,
    ty: &Type,
    name: &str,
    field_assignments: &[StructFieldAssignment],
) -> Option<Vec<OmittedField>> {
    let kept: HashSet<&str> = field_assignments
        .iter()
        .filter(|f| !is_obvious_zero(&f.value))
        .map(|f| f.name.as_str())
        .collect();
    fields_filtered(store, ty, name, &kept)
}

fn fields_filtered(
    store: &Store,
    ty: &Type,
    name: &str,
    keep_specified: &HashSet<&str>,
) -> Option<Vec<OmittedField>> {
    let stripped = ty.strip_refs();
    let Type::Nominal { id, params, .. } = &stripped else {
        return None;
    };

    let def = store.get_definition(id.as_str())?;
    match def {
        Definition::Struct {
            fields, ty: def_ty, ..
        } => {
            let map = build_substitution(def_ty, params);
            Some(
                fields
                    .iter()
                    .filter(|f| !keep_specified.contains(f.name.as_str()))
                    .map(|f| OmittedField {
                        ty: substitute_or_clone(&f.ty, &map),
                        is_public: f.visibility.is_public(),
                    })
                    .collect(),
            )
        }
        Definition::Enum {
            variants, generics, ..
        } => {
            let variant_name = name.rsplit('.').next()?;
            let variant = variants.iter().find(|v| v.name == variant_name)?;
            let mut map = SubstitutionMap::default();
            if generics.len() == params.len() {
                for (g, p) in generics.iter().zip(params.iter()) {
                    map.insert(g.name.clone(), p.clone());
                }
            }
            Some(
                variant
                    .fields
                    .iter()
                    .filter(|f| !keep_specified.contains(f.name.as_str()))
                    .map(|f| OmittedField {
                        ty: substitute_or_clone(&f.ty, &map),
                        is_public: true,
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

fn build_substitution(def_ty: &Type, params: &[Type]) -> SubstitutionMap {
    let mut map = SubstitutionMap::default();
    if let Type::Forall { vars, .. } = def_ty
        && vars.len() == params.len()
    {
        for (var, param) in vars.iter().zip(params.iter()) {
            map.insert(var.clone(), param.clone());
        }
    }
    map
}

fn substitute_or_clone(ty: &Type, map: &SubstitutionMap) -> Type {
    if map.is_empty() {
        ty.clone()
    } else {
        substitute(ty, map)
    }
}
