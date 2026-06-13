use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal, Span};

const MATCH_FUNCTIONS: &[&str] = &["Match", "MatchString", "MatchReader"];

pub fn check_regexp_in_loop(expression: &Expression, ctx: &NodeCtx) {
    // Scan what re-runs each iteration: body, plus a `while`/`while let` head. A
    // `for` iterable runs once on entry, so an enclosing loop scans it instead.
    match expression {
        Expression::Loop { body, .. } | Expression::For { body, .. } => scan(body, ctx),
        Expression::While {
            condition, body, ..
        } => {
            scan(condition, ctx);
            scan(body, ctx);
        }
        Expression::WhileLet {
            scrutinee, body, ..
        } => {
            scan(scrutinee, ctx);
            scan(body, ctx);
        }
        _ => {}
    }
}

/// Nested functions and loops are their own roots, so stop there; a nested `for`
/// iterable runs per *outer* iteration and no inner root covers it, so descend.
fn scan(expression: &Expression, ctx: &NodeCtx) {
    match expression {
        Expression::Function { .. }
        | Expression::Lambda { .. }
        | Expression::Loop { .. }
        | Expression::While { .. }
        | Expression::WhileLet { .. } => {}
        Expression::For { iterable, .. } => scan(iterable, ctx),
        _ => {
            if let Some(span) = recompiled_pattern_span(expression) {
                ctx.sink.push(diagnostics::lint::regexp_in_loop(span));
            }
            for child in expression.children() {
                scan(child, ctx);
            }
        }
    }
}

/// The string-literal pattern of a `regexp.Match*` call. Only literals are flagged;
/// a non-literal pattern may already be hoisted.
fn recompiled_pattern_span(call: &Expression) -> Option<&Span> {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = call
    else {
        return None;
    };
    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return None;
    };
    if !MATCH_FUNCTIONS.contains(&member.as_str()) {
        return None;
    }
    if namespace.get_type().as_import_namespace() != Some("go:regexp") {
        return None;
    }
    match args.first().map(Expression::unwrap_parens) {
        Some(Expression::Literal {
            literal: Literal::String { .. },
            span,
            ..
        }) => Some(span),
        _ => None,
    }
}
