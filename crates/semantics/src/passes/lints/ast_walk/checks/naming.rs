use diagnostics::LisetteDiagnostic;
use syntax::ast::{Expression, Generic, Pattern, Span};

use crate::passes::lints::ast_walk::casing::{is_snake_case, to_pascal_case, to_snake_case};

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
            visibility,
            ..
        } => {
            if !visibility.is_public() {
                check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);
            }

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
            visibility,
            ..
        } => {
            if !visibility.is_public() {
                check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);
            }

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
            visibility,
            ..
        } => {
            if !visibility.is_public() {
                check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);
            }

            for generic in generics {
                check_type_parameter(generic, diagnostics);
            }
        }

        Expression::Interface {
            name,
            name_span,
            generics,
            visibility,
            ..
        } => {
            if !visibility.is_public() {
                check_pascal_case(name, name_span, "non_pascal_case_type", diagnostics);
            }

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
