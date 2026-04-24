//! Reject patterns that don't work in irrefutable contexts.
//!
//! Irrefutable contexts are: let bindings (without `else`), for loops,
//! function/lambda parameters, and select-arm receive bindings. In these
//! positions:
//!
//! - `as` bindings (`pat as name`) are meaningless because there is no fall-
//!   through arm to branch to.
//! - Literal patterns would never match for non-literal scrutinees — the
//!   compiler cannot prove they always match, so they are rejected.
//! - Or-patterns are rejected because they imply branching; an irrefutable
//!   single arm cannot choose among alternatives.
//!
//! `let pat else { ... }` relaxes the or-pattern rule (the `else` arm handles
//! the refutable case) but still rejects bare literal patterns.

use diagnostics::LocalSink;
use syntax::ast::{Binding, Expression, Pattern, SelectArm, SelectArmPattern};

pub(super) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    match expression {
        Expression::Let {
            binding,
            else_block,
            ..
        } => {
            reject_as_binding(&binding.pattern, sink);
            if else_block.is_none() {
                check_binding_pattern(&binding.pattern, sink);
            } else {
                check_literal_only(&binding.pattern, sink);
            }
        }
        Expression::For { binding, .. } => {
            reject_as_binding(&binding.pattern, sink);
            check_binding_pattern(&binding.pattern, sink);
        }
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                visit_param(param, sink);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                visit_select_arm(arm, sink);
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, sink);
    }
}

fn visit_param(param: &Binding, sink: &LocalSink) {
    reject_as_binding(&param.pattern, sink);
    check_binding_pattern(&param.pattern, sink);
}

fn visit_select_arm(arm: &SelectArm, sink: &LocalSink) {
    if let SelectArmPattern::Receive { binding, .. } = &arm.pattern {
        check_binding_pattern(binding, sink);
    }
}

fn reject_as_binding(pattern: &Pattern, sink: &LocalSink) {
    match pattern {
        Pattern::AsBinding { span, .. } => {
            sink.push(diagnostics::infer::as_binding_in_irrefutable_context(*span));
        }
        Pattern::Tuple { elements, .. } => {
            for elem in elements {
                reject_as_binding(elem, sink);
            }
        }
        Pattern::Struct { fields, .. } => {
            for field in fields {
                reject_as_binding(&field.value, sink);
            }
        }
        Pattern::Slice { prefix, .. } => {
            for elem in prefix {
                reject_as_binding(elem, sink);
            }
        }
        Pattern::EnumVariant { fields, .. } => {
            for field in fields {
                reject_as_binding(field, sink);
            }
        }
        _ => {}
    }
}

fn check_binding_pattern(pattern: &Pattern, sink: &LocalSink) {
    if let Pattern::AsBinding { pattern, .. } = pattern {
        check_binding_pattern(pattern, sink);
        return;
    }

    if matches!(pattern, Pattern::Literal { .. }) {
        sink.push(diagnostics::infer::literal_pattern_in_binding(
            pattern.get_span(),
        ));
    }

    if matches!(pattern, Pattern::Or { .. }) {
        sink.push(diagnostics::infer::or_pattern_in_irrefutable_context(
            pattern.get_span(),
        ));
    }
}

fn check_literal_only(pattern: &Pattern, sink: &LocalSink) {
    let mut innermost = pattern;
    while let Pattern::AsBinding { pattern, .. } = innermost {
        innermost = pattern;
    }
    if matches!(innermost, Pattern::Literal { .. }) {
        sink.push(diagnostics::infer::literal_pattern_in_binding(
            pattern.get_span(),
        ));
    }
}
