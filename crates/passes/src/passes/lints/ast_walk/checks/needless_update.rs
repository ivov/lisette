use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, Span, StructSpread};
use syntax::types::Type;

use super::helpers::{is_side_effect_free, struct_field_names};
use crate::passes::walk::NodeCtx;

pub fn check_needless_update(expression: &Expression, ctx: &NodeCtx) {
    let Expression::StructCall {
        name,
        field_assignments,
        spread,
        ty,
        ..
    } = expression
    else {
        return;
    };

    let StructSpread::From(base) = spread else {
        return;
    };
    if !is_side_effect_free(base) {
        return;
    }
    if is_go_imported(ty) {
        return;
    }
    if !same_named_type(&base.get_type(), ty) {
        return;
    }

    let Some(fields) = struct_field_names(ctx.store, ty, name) else {
        return;
    };
    let assigned: HashSet<&str> = field_assignments.iter().map(|f| f.name.as_str()).collect();
    if !fields.iter().all(|f| assigned.contains(f.as_str())) {
        return;
    }

    let base_span = base.get_span();
    let span = spread_span(ctx.source, base_span);
    let base_text = ctx
        .source
        .get(base_span.byte_offset as usize..base_span.end() as usize)
        .unwrap_or("");
    ctx.sink
        .push(diagnostics::lint::needless_update(&span, base_text));
}

fn is_go_imported(ty: &Type) -> bool {
    matches!(ty.strip_refs(), Type::Nominal { id, .. } if id.as_str().starts_with("go:"))
}

fn same_named_type(a: &Type, b: &Type) -> bool {
    matches!(
        (a.strip_refs(), b.strip_refs()),
        (
            Type::Nominal { id: ai, params: ap, .. },
            Type::Nominal { id: bi, params: bp, .. },
        ) if ai == bi && ap == bp
    )
}

fn spread_span(source: &str, base_span: Span) -> Span {
    let start = source
        .get(..base_span.byte_offset as usize)
        .and_then(|prefix| prefix.rfind(".."))
        .map(|pos| pos as u32)
        .unwrap_or(base_span.byte_offset);
    Span::new(base_span.file_id, start, base_span.end() - start)
}
