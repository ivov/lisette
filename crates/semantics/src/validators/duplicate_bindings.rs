//! Reject patterns that bind the same name twice, e.g. `(x, x)` or
//! `Some(x) | Ok(x)` inside the same variant.
//!
//! Walks every `Pattern` site in the typed AST. Mirrors the behaviour of the
//! previous inline check in `checker/infer/checks.rs::check_duplicate_bindings`.

use diagnostics::LocalSink;
use rustc_hash::FxHashMap as HashMap;
use syntax::ast::{
    Binding, Expression, MatchArm, Pattern, RestPattern, SelectArm, SelectArmPattern, Span,
};

pub(super) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            visit_binding(binding, sink);
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            check(pattern, sink);
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

    for child in expression.children() {
        visit_expression(child, sink);
    }
}

fn visit_binding(binding: &Binding, sink: &LocalSink) {
    check(&binding.pattern, sink);
}

fn visit_match_arm(arm: &MatchArm, sink: &LocalSink) {
    check(&arm.pattern, sink);
}

fn visit_select_arm(arm: &SelectArm, sink: &LocalSink) {
    if let SelectArmPattern::Receive { binding, .. } = &arm.pattern {
        check(binding, sink);
    } else if let SelectArmPattern::MatchReceive { arms, .. } = &arm.pattern {
        for arm in arms {
            visit_match_arm(arm, sink);
        }
    }
}

fn check(pattern: &Pattern, sink: &LocalSink) {
    if let Pattern::Or { patterns, .. } = pattern {
        for alternative in patterns {
            check(alternative, sink);
        }
        return;
    }

    if matches!(
        pattern,
        Pattern::Identifier { .. }
            | Pattern::WildCard { .. }
            | Pattern::Literal { .. }
            | Pattern::Unit { .. }
    ) {
        return;
    }

    let bindings = collect_pattern_bindings(pattern);
    let mut seen: HashMap<&str, &Span> = HashMap::default();
    for (name, span) in &bindings {
        if let Some(first_span) = seen.get(name.as_str()) {
            sink.push(diagnostics::infer::duplicate_binding_in_pattern(
                name,
                **first_span,
                *span,
            ));
        } else {
            seen.insert(name.as_str(), span);
        }
    }
}

/// Flatten a pattern into a list of `(name, definition-span)` pairs, one per
/// binding introduced by the pattern. Or-patterns contribute their first arm
/// only (Or arms must bind the same names; duplicates across them are handled
/// separately in `checker/infer/expressions/patterns.rs`).
fn collect_pattern_bindings(pattern: &Pattern) -> Vec<(String, Span)> {
    match pattern {
        Pattern::Identifier { identifier, span } => vec![(identifier.to_string(), *span)],
        Pattern::Tuple { elements, .. } => {
            elements.iter().flat_map(collect_pattern_bindings).collect()
        }
        Pattern::EnumVariant { fields, .. } => {
            fields.iter().flat_map(collect_pattern_bindings).collect()
        }
        Pattern::Struct { fields, .. } => fields
            .iter()
            .flat_map(|f| collect_pattern_bindings(&f.value))
            .collect(),
        Pattern::Slice { prefix, rest, .. } => {
            let mut bindings: Vec<_> = prefix.iter().flat_map(collect_pattern_bindings).collect();
            if let RestPattern::Bind { name, span } = rest {
                bindings.push((name.to_string(), *span));
            }
            bindings
        }
        Pattern::Or { patterns, .. } => patterns
            .first()
            .map(collect_pattern_bindings)
            .unwrap_or_default(),
        Pattern::AsBinding {
            pattern,
            name,
            span,
        } => {
            let mut bindings = collect_pattern_bindings(pattern);
            bindings.push((name.to_string(), *span));
            bindings
        }
        Pattern::WildCard { .. } | Pattern::Literal { .. } | Pattern::Unit { .. } => vec![],
    }
}
