use syntax::ast::{Expression, Span, StructFieldAssignment};

use super::helpers::struct_field_names;
use crate::passes::walk::NodeCtx;

pub fn check_redundant_field_names(expression: &Expression, ctx: &NodeCtx) {
    let Expression::StructCall {
        name,
        field_assignments,
        ty,
        ..
    } = expression
    else {
        return;
    };

    if !field_assignments
        .iter()
        .any(|a| redundant_field(a).is_some())
    {
        return;
    }

    let Some(fields) = struct_field_names(ctx.store, ty, name) else {
        return;
    };

    for assignment in field_assignments {
        if !fields.contains(&assignment.name) {
            continue;
        }
        let Some(value_span) = redundant_field(assignment) else {
            continue;
        };
        let span = assignment.name_span.merge(value_span);
        ctx.sink.push(diagnostics::lint::redundant_field_names(
            &span,
            &assignment.name,
        ));
    }
}

fn redundant_field(assignment: &StructFieldAssignment) -> Option<Span> {
    let Expression::Identifier {
        value,
        span,
        binding_id,
        qualified,
        ..
    } = assignment.value.as_ref()
    else {
        return None;
    };
    if value != &assignment.name {
        return None;
    }
    if binding_id.is_none() && qualified.is_none() {
        return None;
    }
    if span.byte_offset == assignment.name_span.byte_offset {
        return None;
    }
    Some(*span)
}
