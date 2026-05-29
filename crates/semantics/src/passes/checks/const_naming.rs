use diagnostics::LocalSink;
use syntax::ast::Expression;

use crate::passes::lints::ast_walk::casing::{is_screaming_snake_case, to_screaming_snake_case};

pub(crate) fn check(expression: &Expression, sink: &LocalSink) {
    let Expression::Const {
        identifier,
        identifier_span,
        ..
    } = expression
    else {
        return;
    };
    if identifier.starts_with('_') || is_screaming_snake_case(identifier) {
        return;
    }
    sink.push(diagnostics::lint::miscased_screaming_snake(
        identifier_span,
        &to_screaming_snake_case(identifier),
    ));
}
