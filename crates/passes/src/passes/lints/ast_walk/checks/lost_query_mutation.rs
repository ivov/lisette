use syntax::ast::Expression;

use crate::passes::walk::NodeCtx;

const MUTATING_METHODS: &[&str] = &["Set", "Add", "Del"];

pub fn check_lost_query_mutation(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        span,
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

    if !MUTATING_METHODS.contains(&member.as_str()) {
        return;
    }

    let Expression::Call {
        expression: query_callee,
        ..
    } = receiver.unwrap_parens()
    else {
        return;
    };

    let Expression::DotAccess {
        expression: url,
        member: query_member,
        ..
    } = query_callee.unwrap_parens()
    else {
        return;
    };

    if query_member != "Query" {
        return;
    }

    let receiver_ty = ctx.store.peel_alias(&url.get_type().strip_refs());
    if receiver_ty.strip_refs().get_qualified_id() != Some("go:net/url.URL") {
        return;
    }

    ctx.sink
        .push(diagnostics::lint::lost_query_mutation(span, member));
}
