use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;
use semantics::checker::{TypeEnv, check_never_comparable};
use semantics::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::{CompoundKind, Type};

use crate::passes::walk::NodeCtx;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Let { binding, value, .. } => {
            if binding.annotation.is_some()
                && let Expression::Call {
                    call_kind: Some(CallKind::NativeConstructor(NativeTypeKind::Map)),
                    span,
                    ..
                } = value.unwrap_parens()
            {
                ctx.claimed_spans.borrow_mut().insert(*span);
            }
        }
        Expression::Call {
            call_kind: Some(CallKind::NativeConstructor(NativeTypeKind::Map)),
            ty,
            span,
            ..
        } => {
            if ctx.claimed_spans.borrow().contains(span) {
                return;
            }
            report_bad_map_key(ctx.store, ty, *span, ctx.sink, &mut HashSet::default());
        }
        Expression::TypeAlias { ty, span, .. } => {
            report_bad_map_key(ctx.store, ty, *span, ctx.sink, &mut HashSet::default());
        }
        _ => {}
    }
}

fn report_bad_map_key(
    store: &Store,
    ty: &Type,
    span: Span,
    sink: &LocalSink,
    visited: &mut HashSet<String>,
) -> bool {
    let resolved = store.deep_resolve_alias(ty);
    if !visited.insert(format!("{resolved:?}")) {
        return false;
    }
    if let Some((CompoundKind::Map, args)) = resolved.as_compound()
        && let Some(key_ty) = args.first()
        && let Some(reason) = check_never_comparable(&TypeEnv::default(), store, key_ty)
    {
        sink.push(diagnostics::infer::non_comparable_map_key(
            key_ty, reason, span,
        ));
        return true;
    }
    resolved
        .children()
        .iter()
        .any(|child| report_bad_map_key(store, child, span, sink, visited))
}
