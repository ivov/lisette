use std::iter::once;

use crate::passes::walk::NodeCtx;
use semantics::store::Store;
use syntax::ast::Expression;
use syntax::program::{CallKind, Definition, resolved_definition};

pub fn check_task_argument_call(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Task {
        expression: operand,
        ..
    } = expression
    else {
        return;
    };
    flag_inputs(operand.unwrap_parens(), ctx);
}

fn flag_inputs(call: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        ..
    } = call
    else {
        return;
    };
    for part in once(callee.as_ref())
        .chain(args.iter())
        .chain(spread.as_deref())
    {
        flag_calls(part, ctx);
    }
}

fn flag_calls(expression: &Expression, ctx: &NodeCtx) {
    let expression = expression.unwrap_parens();
    match expression {
        Expression::Lambda { .. } | Expression::Task { .. } => return,
        Expression::Defer {
            expression: deferred,
            ..
        } => {
            flag_inputs(deferred.unwrap_parens(), ctx);
            return;
        }
        _ => {}
    }
    if let Expression::Call {
        expression: callee,
        call_kind,
        span,
        ..
    } = expression
        && *call_kind != CallKind::Unresolved
        && !constructs_value(callee, *call_kind, ctx.store)
    {
        ctx.sink.push(diagnostics::lint::task_argument_call(span));
        return;
    }
    for child in expression.children() {
        flag_calls(child, ctx);
    }
}

fn constructs_value(callee: &Expression, call_kind: CallKind, store: &Store) -> bool {
    matches!(
        call_kind,
        CallKind::TupleStructConstructor | CallKind::NativeConstructor(_)
    ) || resolved_definition(callee).is_some_and(|qualified| {
        store
            .get_definition(qualified)
            .is_some_and(Definition::is_type_definition)
            || is_variant_constructor(qualified, store)
    })
}

fn is_variant_constructor(qualified: &str, store: &Store) -> bool {
    let Some((prefix, variant)) = qualified.rsplit_once('.') else {
        return false;
    };
    if store.variant_of(prefix, variant).is_some() {
        return true;
    }
    prefix == "prelude"
        && ["prelude.Option", "prelude.Result", "prelude.Partial"]
            .iter()
            .any(|enum_name| store.variant_of(enum_name, variant).is_some())
}
