use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, MatchOrigin, Pattern, Span};
use syntax::types::unqualified_name;

use crate::is_trivial_expression;

pub fn check_single_arm_match(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Match {
        arms, origin, span, ..
    } = expression
    else {
        return;
    };

    if matches!(origin, MatchOrigin::IfLet { .. }) {
        return;
    }

    if arms.len() != 2 {
        return;
    }

    let (first, second) = (&arms[0], &arms[1]);

    if first.has_guard() || second.has_guard() {
        return;
    }

    let second_is_catchall = matches!(
        &second.pattern,
        Pattern::WildCard { .. } | Pattern::Identifier { .. }
    );
    let second_is_trivial = is_trivial_expression(&second.expression);

    if !second_is_catchall || !second_is_trivial {
        return;
    }

    if matches!(&first.pattern, Pattern::EnumVariant { .. }) {
        let pattern_string = pattern_to_suggestion(&first.pattern);
        let match_keyword_span = Span::new(span.file_id, span.byte_offset, 5);

        ctx.sink.push(diagnostics::lint::single_arm_match(
            &match_keyword_span,
            &pattern_string,
        ));
    }
}

fn pattern_to_suggestion(pattern: &Pattern) -> String {
    match pattern {
        Pattern::EnumVariant {
            identifier, fields, ..
        } => {
            let variant = unqualified_name(identifier);
            if fields.is_empty() {
                variant.to_string()
            } else if fields.len() == 1 {
                format!("{}(x)", variant)
            } else {
                let bindings: Vec<_> = (0..fields.len()).map(|i| format!("x{}", i)).collect();
                format!("{}({})", variant, bindings.join(", "))
            }
        }
        Pattern::Literal { literal, .. } => format!("{:?}", literal),
        _ => "_".to_string(),
    }
}
