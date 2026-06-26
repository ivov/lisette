use diagnostics::LocalSink;
use syntax::ast::{Expression, Generic, Pattern, RestPattern, Span};

use crate::passes::lints::ast_walk::casing::{is_snake_case, to_pascal_case, to_snake_case};
use crate::passes::walk::{FunctionRole, NodeCtx, PatternRole};

use super::helpers::first_param_is_self;

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

                if !is_d_lis && variant.fields.is_struct() {
                    for field in variant.fields.iter() {
                        check_snake_case(
                            &field.name,
                            &field.name_span,
                            "non_snake_case_enum_field",
                            sink,
                        );
                    }
                }
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
            let is_go_method = match ctx.function_role.get() {
                FunctionRole::InterfaceMethod => true,
                FunctionRole::ImplMethod => first_param_is_self(params),
                FunctionRole::Free => false,
            };
            let exempt = is_go_method && name.chars().next().is_some_and(char::is_uppercase);
            if !is_d_lis && !exempt {
                check_snake_case(name, name_span, "non_snake_case_function", sink);
            }

            for generic in generics {
                check_type_parameter(generic, sink);
            }
        }

        Expression::ImplBlock { generics, .. } => {
            for generic in generics {
                check_type_parameter(generic, sink);
            }
        }

        _ => {}
    }
}

pub fn check_pattern_naming(pattern: &Pattern, ctx: &NodeCtx) {
    if ctx.is_d_lis {
        return;
    }
    let (name, span) = match pattern {
        Pattern::Identifier { identifier, span } => (identifier, span),
        Pattern::AsBinding {
            name, name_span, ..
        } => (name, name_span),
        Pattern::Slice {
            rest: RestPattern::Bind { name, span },
            ..
        } => (name, span),
        _ => return,
    };
    let code = match ctx.pattern_role.get() {
        PatternRole::Parameter => "non_snake_case_parameter",
        PatternRole::Binding => "non_snake_case_variable",
    };
    check_snake_case(name, span, code, ctx.sink);
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
