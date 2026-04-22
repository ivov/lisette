use super::casing::{
    is_screaming_snake_case, is_snake_case, to_pascal_case, to_screaming_snake_case, to_snake_case,
};
use crate::is_trivial_expression;
use diagnostics::LisetteDiagnostic;
use syntax::ast::{
    BinaryOperator, Expression, FormatStringPart, Generic, Literal, MatchOrigin, Pattern,
    RestPattern, Span, UnaryOperator,
};

pub fn check_double_negation(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Unary {
        operator,
        expression: operand,
        span: outer_span,
        ..
    } = expression
    else {
        return;
    };

    let Expression::Unary {
        operator: inner_op,
        span: inner_span,
        ..
    } = operand.unwrap_parens()
    else {
        return;
    };

    if operator != inner_op {
        return;
    }

    if !matches!(operator, UnaryOperator::Not | UnaryOperator::Negative) {
        return;
    }

    let operators_span = Span::new(
        outer_span.file_id,
        outer_span.byte_offset,
        inner_span.byte_offset - outer_span.byte_offset + 1,
    );

    let is_bool = *operator == UnaryOperator::Not;
    diagnostics.push(diagnostics::lint::double_negation(&operators_span, is_bool));
}

pub fn check_self_comparison(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Binary {
        operator,
        left,
        right,
        span,
        ..
    } = expression
    else {
        return;
    };

    use BinaryOperator::*;
    if !matches!(
        operator,
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual
    ) {
        return;
    }

    let (
        Expression::Identifier {
            value: left_name, ..
        },
        Expression::Identifier {
            value: right_name, ..
        },
    ) = (left.unwrap_parens(), right.unwrap_parens())
    else {
        return;
    };

    if left_name != right_name {
        return;
    }

    // Don't warn for float types — NaN == NaN is false per IEEE 754
    if left.get_type().is_float() {
        return;
    }

    let always_true = matches!(operator, Equal | LessThanOrEqual | GreaterThanOrEqual);
    diagnostics.push(diagnostics::lint::tautological_comparison(
        span,
        always_true,
    ));
}

pub fn check_self_assignment(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Assignment {
        target,
        value,
        span,
        ..
    } = expression
    else {
        return;
    };

    let (
        Expression::Identifier {
            value: target_name, ..
        },
        Expression::Identifier {
            value: value_name, ..
        },
    ) = (target.unwrap_parens(), value.unwrap_parens())
    else {
        return;
    };

    if target_name != value_name {
        return;
    }

    diagnostics.push(diagnostics::lint::self_assignment(span));
}

pub fn check_empty_match_arm(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
    let Expression::Match { arms, .. } = expression else {
        return;
    };

    for arm in arms {
        if let Expression::Block { items, span, .. } = &*arm.expression
            && items.is_empty()
        {
            diagnostics.push(diagnostics::lint::empty_match_arm(span));
        }
    }
}

pub fn check_excess_parens_on_condition(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let (condition, keyword) = match expression {
        Expression::If { condition, .. } => (condition.as_ref(), "if"),
        Expression::While { condition, .. } => (condition.as_ref(), "while"),
        Expression::Match { subject, .. } => (subject.as_ref(), "match"),
        _ => return,
    };

    if let Expression::Paren { span, .. } = condition {
        diagnostics.push(diagnostics::lint::unnecessary_parens(span, keyword));
    }
}

pub fn check_match_literal_collection(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Match { subject, .. } = expression else {
        return;
    };

    let unwrapped = subject.unwrap_parens();

    if !unwrapped.is_all_literals() {
        return;
    }

    let span = match unwrapped {
        Expression::Literal {
            literal: Literal::Slice(_),
            span,
            ..
        } => Some(span),
        Expression::Tuple { span, .. } => Some(span),
        _ => None,
    };

    if let Some(span) = span {
        diagnostics.push(diagnostics::lint::match_on_literal(span));
    }
}

pub fn check_single_arm_match(expression: &Expression, diagnostics: &mut Vec<LisetteDiagnostic>) {
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

        diagnostics.push(diagnostics::lint::single_arm_match(
            &match_keyword_span,
            &pattern_string,
        ));
    }
}

pub fn check_uninterpolated_fstring(
    expression: &Expression,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    let Expression::Literal {
        literal: Literal::FormatString(parts),
        span,
        ..
    } = expression
    else {
        return;
    };

    let has_interpolation = parts
        .iter()
        .any(|p| matches!(p, FormatStringPart::Expression(_)));

    if !has_interpolation {
        diagnostics.push(diagnostics::lint::uninterpolated_fstring(span));
    }
}

fn pattern_to_suggestion(pattern: &Pattern) -> String {
    match pattern {
        Pattern::EnumVariant {
            identifier, fields, ..
        } => {
            let variant = identifier.split('.').next_back().unwrap_or(identifier);
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

pub fn check_rest_only_slice_pattern(pattern: &Pattern, diagnostics: &mut Vec<LisetteDiagnostic>) {
    if let Pattern::Or { patterns, .. } = pattern {
        for p in patterns {
            check_rest_only_slice_pattern(p, diagnostics);
        }
        return;
    }

    if let Pattern::Slice {
        prefix, rest, span, ..
    } = pattern
        && prefix.is_empty()
        && !matches!(rest, RestPattern::Absent)
    {
        let help = match rest {
            RestPattern::Bind { name, .. } => {
                format!("Use `let {}` instead", name)
            }
            _ => "Use `let _` instead".to_string(),
        };

        diagnostics.push(diagnostics::lint::rest_only_slice_pattern(span, help));
    }
}

pub fn check_expression_naming(
    expression: &Expression,
    is_d_lis: bool,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Struct {
            name,
            name_span,
            generics,
            fields,
            ..
        } => {
            check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }

            if !is_d_lis {
                for field in fields {
                    check_snake_case(
                        &field.name,
                        &field.name_span,
                        "non_snake_case_struct_field",
                        diagnostics,
                    );
                }
            }
        }

        Expression::Enum {
            name,
            name_span,
            generics,
            variants,
            ..
        } => {
            check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }

            for variant in variants {
                check_pascal_case(
                    &variant.name,
                    &variant.name_span,
                    "non_pascal_case_enum_variant",
                    diagnostics,
                );
            }
        }

        Expression::TypeAlias {
            name,
            name_span,
            generics,
            ..
        } => {
            check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }
        }

        Expression::Interface {
            name,
            name_span,
            generics,
            ..
        } => {
            check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }
        }

        Expression::Function {
            name,
            name_span,
            generics,
            params,
            ..
        } => {
            if !is_d_lis {
                let is_method = params.first().is_some_and(|p| {
                    matches!(&p.pattern, Pattern::Identifier { identifier, .. } if identifier == "self")
                });
                if !is_method {
                    check_snake_case(name, name_span, "non_snake_case_function", diagnostics);
                }
            }

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }

            if !is_d_lis {
                for param in params {
                    if let Pattern::Identifier { identifier, span } = &param.pattern {
                        check_snake_case(identifier, span, "non_snake_case_parameter", diagnostics);
                    }
                }
            }
        }

        Expression::Const {
            identifier,
            identifier_span,
            ..
        } => {
            if !is_d_lis {
                check_screaming_snake_case(identifier, identifier_span, diagnostics);
            }
        }

        _ => {}
    }
}

pub fn check_pattern_naming(
    pattern: &Pattern,
    is_d_lis: bool,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    if is_d_lis {
        return;
    }
    if let Pattern::Identifier { identifier, span } = pattern {
        check_snake_case(identifier, span, "non_snake_case_variable", diagnostics);
    }
}

fn check_type_parameter(generic: &Generic, diagnostics: &mut Vec<LisetteDiagnostic>) {
    check_pascal_case(
        &generic.name,
        &generic.span,
        "non_pascal_case_type_parameter",
        diagnostics,
    );
}

fn check_pascal_case(
    name: &str,
    span: &Span,
    code: &str,
    diagnostics: &mut Vec<LisetteDiagnostic>,
) {
    if name.starts_with('_') {
        return;
    }

    let first_char = name.chars().next().unwrap_or('A');
    if !first_char.is_uppercase() {
        diagnostics.push(diagnostics::lint::miscased_pascal(
            span,
            code,
            &to_pascal_case(name),
        ));
    }
}

fn check_snake_case(name: &str, span: &Span, code: &str, diagnostics: &mut Vec<LisetteDiagnostic>) {
    if name.starts_with('_') || is_snake_case(name) {
        return;
    }

    diagnostics.push(diagnostics::lint::miscased_snake(
        span,
        code,
        &to_snake_case(name),
    ));
}

fn check_screaming_snake_case(name: &str, span: &Span, diagnostics: &mut Vec<LisetteDiagnostic>) {
    if name.starts_with('_') || is_screaming_snake_case(name) {
        return;
    }

    diagnostics.push(diagnostics::lint::miscased_screaming_snake(
        span,
        &to_screaming_snake_case(name),
    ));
}
