use diagnostics::LocalSink;
use syntax::ast::{Expression, Generic, Pattern, Span};

use crate::passes::lints::ast_walk::casing::{is_snake_case, to_pascal_case, to_snake_case};
use crate::passes::walk::NodeCtx;

pub fn check_expression_naming(expression: &Expression, ctx: &NodeCtx) {
    let sink = ctx.sink;
    let is_d_lis = ctx.is_d_lis;
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
                check_pascal_case(name, name_span, "non_pascal_case_type", sink);
            }

            for generic in generics {
                check_type_parameter(generic, sink);
            }

            if !is_d_lis {
                for field in fields.iter().filter(|f| !f.embedded) {
                    check_snake_case(
                        &field.name,
                        &field.name_span,
                        "non_snake_case_struct_field",
                        sink,
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
                check_pascal_case(name, name_span, "non_pascal_case_type", sink);
            }

            for generic in generics {
                check_type_parameter(generic, sink);
            }

            for variant in variants {
                check_pascal_case(
                    &variant.name,
                    &variant.name_span,
                    "non_pascal_case_enum_variant",
                    sink,
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
                check_pascal_case(name, name_span, "non_pascal_case_type", sink);
            }

            for generic in generics {
                check_type_parameter(generic, sink);
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
                check_pascal_case(name, name_span, "non_pascal_case_type", sink);
            }

            for generic in generics {
                check_type_parameter(generic, sink);
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
                    check_snake_case(name, name_span, "non_snake_case_function", sink);
                }
            }

            for generic in generics {
                check_type_parameter(generic, sink);
            }

            if !is_d_lis {
                for param in params {
                    if let Pattern::Identifier { identifier, span } = &param.pattern {
                        check_snake_case(identifier, span, "non_snake_case_parameter", sink);
                    }
                }
            }
        }

        _ => {}
    }
}

pub fn check_pattern_naming(pattern: &Pattern, ctx: &NodeCtx) {
    if ctx.is_d_lis {
        return;
    }
    if let Pattern::Identifier { identifier, span } = pattern {
        check_snake_case(identifier, span, "non_snake_case_variable", ctx.sink);
    }
}

fn check_type_parameter(generic: &Generic, sink: &LocalSink) {
    check_pascal_case(
        &generic.name,
        &generic.span,
        "non_pascal_case_type_parameter",
        sink,
    );
}

fn check_pascal_case(name: &str, span: &Span, code: &str, sink: &LocalSink) {
    if name.starts_with('_') {
        return;
    }

    let first_char = name.chars().next().unwrap_or('A');
    if !first_char.is_uppercase() {
        sink.push(diagnostics::lint::miscased_pascal(
            span,
            code,
            &to_pascal_case(name),
        ));
    }
}

fn check_snake_case(name: &str, span: &Span, code: &str, sink: &LocalSink) {
    if name.starts_with('_') || is_snake_case(name) {
        return;
    }

    sink.push(diagnostics::lint::miscased_snake(
        span,
        code,
        &to_snake_case(name),
    ));
}
