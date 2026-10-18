use crate::passes::walk::NodeCtx;
use syntax::ast::{ConstructorPatternResolution, Expression, Pattern};
use syntax::program::{CallKind, resolved_definition};
use syntax::types::{GO_IMPORT_PREFIX, unqualified_name};

pub fn check_unreachable_partial_err(expression: &Expression, ctx: &NodeCtx<'_>) {
    let Expression::IfLet {
        scrutinee,
        pattern:
            Pattern::EnumVariant {
                resolution:
                    ConstructorPatternResolution::EnumVariant {
                        enum_name,
                        variant_name,
                    },
                span,
                ..
            },
        ..
    } = expression
    else {
        return;
    };
    if enum_name != "prelude.Partial" || unqualified_name(variant_name) != "Err" {
        return;
    }
    let Expression::Call {
        expression: callee,
        call_kind: CallKind::Regular,
        ty,
        ..
    } = scrutinee.unwrap_parens()
    else {
        return;
    };
    // Lisette functions can construct Err even with a non-nilable value type.
    let Some(callee_id) = resolved_definition(callee).filter(|id| id.starts_with(GO_IMPORT_PREFIX))
    else {
        return;
    };
    let ty = ctx.store.peel_alias(ty);
    if !ty.is_partial() {
        return;
    }
    let ok_ty = ty.ok_type();
    let underlying = ctx.store.peel_underlying(&ok_ty);
    if underlying.contains_type_parameter()
        || underlying.is_variable()
        || underlying.is_placeholder()
        || ctx.store.contains_unknown(&ok_ty)
        || ctx.store.partial_ok_is_nilable(&ok_ty)
    {
        return;
    }
    ctx.sink.push(diagnostics::lint::unreachable_partial_err(
        span,
        unqualified_name(callee_id),
    ));
}
