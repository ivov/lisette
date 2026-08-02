use crate::passes::walk::NodeCtx;
use syntax::ast::{Binding, Expression, Pattern};

use super::helpers::bool_literal;

const TEST_CONTEXT_QUALIFIED_ID: &str = "**test_prelude.TestContext";

pub fn check_infallible_assertion(expression: &Expression, ctx: &NodeCtx) {
    if !ctx.is_test() || !establishes_test_context(expression) {
        return;
    }
    for child in expression.children() {
        scan(child, ctx);
    }
}

fn scan(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Assert {
        expression: operand,
        span,
        ..
    } = expression
        && bool_literal(operand.unwrap_parens()) == Some(true)
    {
        ctx.sink.push(diagnostics::lint::infallible_assertion(span));
    }
    if matches!(
        expression,
        Expression::Function { .. } | Expression::Lambda { .. }
    ) && establishes_test_context(expression)
    {
        return;
    }
    for child in expression.children() {
        scan(child, ctx);
    }
}

fn establishes_test_context(expression: &Expression) -> bool {
    match expression {
        Expression::Function {
            attributes, params, ..
        } => {
            attributes.iter().any(|attribute| attribute.name == "test")
                || params.iter().any(provides_test_handle)
        }
        Expression::Lambda { params, .. } => params.iter().any(provides_test_handle),
        _ => false,
    }
}

fn provides_test_handle(param: &Binding) -> bool {
    matches!(&param.pattern, Pattern::Identifier { identifier, .. } if identifier != "_")
        && param.ty.strip_refs().get_qualified_id() == Some(TEST_CONTEXT_QUALIFIED_ID)
}
