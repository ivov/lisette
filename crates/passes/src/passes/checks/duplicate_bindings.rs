//! Reject patterns that bind the same name twice, e.g. `(x, x)` or
//! `Some(x) | Ok(x)` inside the same variant.
//!
//! Walks every `Pattern` site in the typed AST. Mirrors the behaviour of the
//! previous inline check in `checker/infer/checks.rs::check_duplicate_bindings`.

use crate::passes::walk::NodeCtx;
use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{Binding, Expression, MatchArm, Pattern, SelectArm, Span};

use syntax::ast::{collect_pattern_bindings, expand_or_patterns};

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let sink = ctx.sink;
    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            visit_binding(binding, sink);
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            check_pattern(pattern, sink);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                visit_match_arm(arm, sink);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                visit_select_arm(arm, sink);
            }
        }
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                visit_binding(param, sink);
            }
        }
        _ => {}
    }
}

fn visit_binding(binding: &Binding, sink: &LocalSink) {
    check_pattern(&binding.pattern, sink);
}

fn visit_match_arm(arm: &MatchArm, sink: &LocalSink) {
    check_pattern(&arm.pattern, sink);
}

fn visit_select_arm(arm: &SelectArm, sink: &LocalSink) {
    if let SelectArm::Receive { binding, .. } = arm {
        check_pattern(binding, sink);
    } else if let SelectArm::MatchReceive { arms, .. } = arm {
        for arm in arms {
            visit_match_arm(arm, sink);
        }
    }
}

fn check_pattern(pattern: &Pattern, sink: &LocalSink) {
    let mut reported: Vec<(Span, Span)> = vec![];
    for expansion in expand_or_patterns(pattern, &|_| true) {
        let bindings = collect_pattern_bindings(&expansion.pattern);
        let mut seen: HashMap<&str, Span> = HashMap::default();
        for (name, span) in &bindings {
            if let Some(first_span) = seen.get(name.as_str()) {
                if !reported.contains(&(*first_span, *span)) {
                    reported.push((*first_span, *span));
                    sink.push(diagnostics::infer::duplicate_binding_in_pattern(
                        name,
                        *first_span,
                        *span,
                    ));
                }
            } else {
                seen.insert(name.as_str(), *span);
            }
        }
    }
}
