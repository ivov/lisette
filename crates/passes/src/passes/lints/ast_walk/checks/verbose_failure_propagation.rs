use crate::passes::walk::NodeCtx;
use semantics::store::Store;
use syntax::ast::{Expression, MatchArm, Pattern, Span};
use syntax::types::{Type, unqualified_name};

use super::helpers::enum_variant_binding;

type Arm<'a> = (&'a Pattern, &'a Expression);

pub fn check_verbose_failure_propagation(expression: &Expression, ctx: &NodeCtx) {
    let (fires, span, keyword_len) = match expression {
        Expression::Match {
            subject,
            arms,
            span,
            ..
        } => {
            if arms.len() != 2 || arms.iter().any(MatchArm::has_guard) {
                return;
            }
            let a = (&arms[0].pattern, arms[0].expression.as_ref());
            let b = (&arms[1].pattern, arms[1].expression.as_ref());
            (propagation_fires(subject, a, b, ctx.store), span, 5)
        }
        Expression::IfLet {
            scrutinee,
            pattern,
            consequence,
            alternative,
            span,
            ..
        } => {
            let Some(alternative) = alternative.expression() else {
                return;
            };
            let wildcard = Pattern::WildCard {
                span: alternative.get_span(),
            };
            let a = (pattern, consequence.as_ref());
            let b = (&wildcard, alternative);
            (propagation_fires(scrutinee, a, b, ctx.store), span, 2)
        }
        _ => return,
    };

    if fires {
        let keyword_span = Span::new(span.file_id, span.byte_offset, keyword_len);
        ctx.sink
            .push(diagnostics::lint::verbose_failure_propagation(
                &keyword_span,
            ));
    }
}

fn propagation_fires(subject: &Expression, a: Arm, b: Arm, store: &Store) -> bool {
    let subject_ty = subject.get_type();
    if subject_ty.is_option() {
        check_option_propagation(a, b)
    } else if subject_ty.is_result() {
        check_result_propagation(&subject_ty, a, b, store)
    } else {
        false
    }
}

fn check_option_propagation(arm_a: Arm, arm_b: Arm) -> bool {
    let try_pair = |some_arm: Arm, fail_arm: Arm| {
        let Some(name) = enum_variant_binding(some_arm.0, "Some") else {
            return false;
        };
        is_none_or_wildcard(fail_arm.0)
            && body_is_identifier(some_arm.1, name)
            && body_is_return_none(fail_arm.1)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn check_result_propagation(subject_ty: &Type, arm_a: Arm, arm_b: Arm, store: &Store) -> bool {
    let try_pair = |ok_arm: Arm, err_arm: Arm| {
        let Some(ok_name) = enum_variant_binding(ok_arm.0, "Ok") else {
            return false;
        };
        let Some(err_name) = enum_variant_binding(err_arm.0, "Err") else {
            return false;
        };
        if !body_is_identifier(ok_arm.1, ok_name) {
            return false;
        }
        let Some(returned) = returned_err_of_binding(err_arm.1, err_name) else {
            return false;
        };
        propagation_type_checks(subject_ty, &returned.get_type(), store)
    };
    try_pair(arm_a, arm_b) || try_pair(arm_b, arm_a)
}

fn propagation_type_checks(subject_ty: &Type, returned_ty: &Type, store: &Store) -> bool {
    returned_ty.is_result()
        && store.peel_alias(&subject_ty.err_type()) == store.peel_alias(&returned_ty.err_type())
}

fn is_none_or_wildcard(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::WildCard { .. } => true,
        Pattern::EnumVariant {
            identifier,
            fields,
            rest,
            ..
        } => unqualified_name(identifier) == "None" && fields.is_empty() && !*rest,
        _ => false,
    }
}

fn body_is_identifier(expression: &Expression, name: &str) -> bool {
    match expression.unwrap_parens() {
        Expression::Identifier { value, .. } => value.as_str() == name,
        Expression::Block { items, .. } => items.len() == 1 && body_is_identifier(&items[0], name),
        _ => false,
    }
}

fn body_is_return_none(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Return {
            expression: inner, ..
        } => matches!(inner.unwrap_parens(), Expression::Identifier { value, .. }
            if value.as_str() == "None"),
        Expression::Block { items, .. } => items.len() == 1 && body_is_return_none(&items[0]),
        _ => false,
    }
}

fn returned_err_of_binding<'a>(
    expression: &'a Expression,
    binding: &str,
) -> Option<&'a Expression> {
    match expression.unwrap_parens() {
        Expression::Return {
            expression: inner, ..
        } => {
            let inner = inner.unwrap_parens();
            is_err_of_binding(inner, binding).then_some(inner)
        }
        Expression::Block { items, .. } if items.len() == 1 => {
            returned_err_of_binding(&items[0], binding)
        }
        _ => None,
    }
}

fn is_err_of_binding(expression: &Expression, binding: &str) -> bool {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression.unwrap_parens()
    else {
        return false;
    };
    if args.len() != 1 {
        return false;
    }
    let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
        return false;
    };
    if unqualified_name(value) != "Err" {
        return false;
    }
    matches!(args[0].unwrap_parens(), Expression::Identifier { value, .. }
        if value.as_str() == binding)
}
