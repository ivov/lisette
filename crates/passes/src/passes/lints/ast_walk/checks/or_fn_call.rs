use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;
use syntax::program::CallKind;

use super::helpers::has_escaping_control_flow;

pub fn check_or_fn_call(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return;
    };
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    let eager = member.as_str();
    let (lazy, eager_argument, allows_result, allows_partial) = match eager {
        "unwrap_or" => {
            let [default] = args.as_slice() else {
                return;
            };
            ("unwrap_or_else", default, true, true)
        }
        "ok_or" => {
            let [err] = args.as_slice() else {
                return;
            };
            ("ok_or_else", err, false, false)
        }
        "map_or" => {
            let [default, _mapper] = args.as_slice() else {
                return;
            };
            ("map_or_else", default, true, false)
        }
        _ => return,
    };

    // `?`/`return`/`break`/`continue` target the enclosing function, so moving
    // the argument into a synthesized closure would retarget or break it.
    if has_escaping_control_flow(eager_argument) {
        return;
    }

    if !does_real_work(eager_argument) {
        return;
    }

    let receiver_ty = receiver.get_type();
    let supported = receiver_ty.is_option()
        || (allows_result && receiver_ty.is_result())
        || (allows_partial && receiver_ty.is_partial());
    if !supported {
        return;
    }

    ctx.sink.push(diagnostics::lint::or_fn_call(
        &eager_argument.get_span(),
        eager,
        lazy,
    ));
}

// Descends through cheap wrappers (arithmetic, field reads, value constructors),
// but stops at a closure, whose body does not run until invoked.
fn does_real_work(argument: &Expression) -> bool {
    match argument.unwrap_parens() {
        Expression::Lambda { .. } => false,
        Expression::Call {
            expression: callee,
            args,
            call_kind,
            ..
        } => {
            if is_value_constructor(callee, call_kind.as_ref()) {
                args.iter().any(does_real_work)
            } else {
                true
            }
        }
        other => other.children().iter().any(does_real_work),
    }
}

fn is_value_constructor(callee: &Expression, call_kind: Option<&CallKind>) -> bool {
    match call_kind {
        Some(CallKind::TupleStructConstructor) => true,
        // Enum-variant constructors resolve to `Regular`; an unresolved callee
        // may still be one, so fall back to the leaf-name shape in both cases.
        Some(CallKind::Regular) | None => is_pascal_case_constructor(callee),
        _ => false,
    }
}

fn is_pascal_case_constructor(callee: &Expression) -> bool {
    match callee.unwrap_parens() {
        Expression::Identifier { value, .. } => starts_uppercase(value),
        Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } => starts_uppercase(member) && receiver.get_type().as_import_namespace().is_none(),
        _ => false,
    }
}

fn starts_uppercase(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}
