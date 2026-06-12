use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub fn check_exit_after_defer(expression: &Expression, ctx: &NodeCtx) {
    let body = match expression {
        Expression::Function { body, .. } | Expression::Lambda { body, .. } => body,
        _ => return,
    };

    scan(body, false, ctx);
}

fn scan(expression: &Expression, pending: bool, ctx: &NodeCtx) {
    match expression {
        Expression::Function { .. } | Expression::Lambda { .. } => {}
        Expression::Block { items, .. } => {
            let mut pending = pending;
            for item in items {
                scan(item, pending, ctx);
                if matches!(item, Expression::Defer { .. }) {
                    pending = true;
                }
            }
        }
        Expression::Call {
            expression: callee,
            span,
            ..
        } => {
            if pending && is_os_exit(callee) {
                ctx.sink.push(diagnostics::lint::exit_after_defer(span));
            }
            for child in expression.children() {
                scan(child, pending, ctx);
            }
        }
        _ => {
            for child in expression.children() {
                scan(child, pending, ctx);
            }
        }
    }
}

fn is_os_exit(callee: &Expression) -> bool {
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return false;
    };
    member.as_str() == "Exit" && receiver.get_type().as_import_namespace() == Some("go:os")
}
