use crate::passes::walk::NodeCtx;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{Expression, Literal};

const CUTSET_FUNCTIONS: &[&str] = &["Trim", "TrimLeft", "TrimRight"];

pub fn check_duplicate_cutset(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return;
    };

    let Expression::DotAccess {
        expression: namespace,
        member,
        ..
    } = callee.unwrap_parens()
    else {
        return;
    };

    if !CUTSET_FUNCTIONS.contains(&member.as_str()) {
        return;
    }

    if namespace.get_type().as_import_namespace() != Some("go:strings") {
        return;
    }

    let Some(Expression::Literal {
        literal: Literal::String { value, .. },
        span,
        ..
    }) = args.get(1).map(Expression::unwrap_parens)
    else {
        return;
    };

    if has_duplicate_char(value) {
        ctx.sink
            .push(diagnostics::lint::trim_charset_misuse(span, member));
    }
}

fn has_duplicate_char(cutset: &str) -> bool {
    let mut seen = HashSet::default();
    !cutset.chars().all(|c| seen.insert(c))
}
