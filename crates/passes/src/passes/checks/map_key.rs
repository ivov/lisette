use diagnostics::LocalSink;
use rustc_hash::FxHashSet;
use semantics::checker::{TypeEnv, check_never_comparable};
use semantics::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::{CallOperation, Definition, NativeTypeKind};
use syntax::types::{CompoundKind, Symbol, Type};

use crate::passes::walk::{ClaimKind, NodeCtx};

pub(crate) fn check(expression: &Expression, ctx: &mut NodeCtx) {
    let constructs_map = |expression: &Expression| {
        matches!(
            ctx.facts.selections.call_operation(expression),
            Some(CallOperation::NativeConstructor {
                kind: NativeTypeKind::Map,
                ..
            })
        )
    };
    match expression {
        Expression::Let { binding, value, .. } => {
            if binding.annotation.is_some()
                && let Expression::Call { span, .. } = value.unwrap_parens()
                && constructs_map(value)
            {
                ctx.claim(ClaimKind::MapKey, *span);
            }
        }
        Expression::Call { ty, span, .. } if constructs_map(expression) => {
            if ctx.is_claimed(ClaimKind::MapKey, span) {
                return;
            }
            report_bad_map_key(ctx.store, ty, *span, ctx.sink, &mut FxHashSet::default());
        }
        Expression::TypeAlias { ty, span, .. } => {
            report_bad_map_key(ctx.store, ty, *span, ctx.sink, &mut FxHashSet::default());
        }
        _ => {}
    }
}

fn report_bad_map_key(
    store: &Store,
    ty: &Type,
    span: Span,
    sink: &LocalSink,
    expanding: &mut FxHashSet<Symbol>,
) -> bool {
    let alias_head = match ty {
        Type::Nominal { id, .. }
            if store
                .get_definition(id.as_str())
                .is_some_and(Definition::is_type_alias) =>
        {
            Some(id.clone())
        }
        _ => None,
    };
    if let Some(id) = &alias_head
        && !expanding.insert(id.clone())
    {
        return ty
            .get_type_params()
            .unwrap_or_default()
            .iter()
            .any(|child| report_bad_map_key(store, child, span, sink, expanding));
    }
    let resolved = store.peel_alias(ty);
    let found = if let Some((CompoundKind::Map, args)) = resolved.as_compound()
        && let Some(key_ty) = args.first()
        && let Some(reason) = check_never_comparable(&TypeEnv::default(), store, key_ty)
    {
        sink.push(diagnostics::infer::non_comparable_map_key(
            key_ty, reason, span,
        ));
        true
    } else {
        resolved
            .children()
            .iter()
            .any(|child| report_bad_map_key(store, child, span, sink, expanding))
    };
    if let Some(id) = alias_head {
        expanding.remove(&id);
    }
    found
}
