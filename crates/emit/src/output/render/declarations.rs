use crate::output::render::bodies::render_lowered_block;
use crate::output::render::types::go_type_text;
use crate::output::render::writer::GoWriter;
use crate::target::ir::bodies::{LoweredBlock, for_each_statements_mut};
use crate::target::ir::declarations::{
    ConstantDeclaration, DeclarationKind, Documentation, FieldDeclaration, FunctionSignature,
    GoDeclaration, InterfaceMethod, TypeParameter,
};
use crate::target::ir::go_expression::Position;
use crate::target::types::go_type::{GoType, GoTypeShape};
use std::fmt::Write;

fn write_documentation(output: &mut GoWriter, documentation: &Documentation) {
    for line in &documentation.0 {
        output.push_foreign("//");
        if !line.is_empty() {
            output.push_foreign(" ");
            output.push_foreign(line);
        }
        output.push('\n');
    }
}

fn generics(output: &mut GoWriter, parameters: &[TypeParameter]) {
    if parameters.is_empty() {
        return;
    }
    output.push('[');
    for (index, parameter) in parameters.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        write!(output, "{} ", parameter.name).unwrap();
        match parameter.bounds.as_slice() {
            [] => output.push_str("any"),
            [bound] => output.push_str(&go_type_text(bound)),
            bounds => {
                output.push_str("interface { ");
                for (index, bound) in bounds.iter().enumerate() {
                    if index > 0 {
                        output.push_str("; ");
                    }
                    output.push_str(&go_type_text(bound));
                }
                output.push_str(" }");
            }
        }
    }
    output.push(']');
}

fn write_type_header(output: &mut GoWriter, name: &str, parameters: &[TypeParameter]) {
    output.push_str("type ");
    output.push_str(name);
    generics(output, parameters);
}

fn render_positioned_body(output: &mut GoWriter, body: &LoweredBlock, position: Option<&Position>) {
    let Some(position) = position else {
        render_lowered_block(output, body);
        return;
    };
    let mut body = body.clone();
    for_each_statements_mut(&mut body.statements, &mut |statements| {
        for statement in statements {
            statement.line.get_or_insert_with(|| position.clone());
        }
    });
    render_lowered_block(output, &body);
}

pub(crate) fn render_declaration(declaration: &GoDeclaration) -> String {
    let mut output = GoWriter::with_source_directives();
    write_documentation(&mut output, &declaration.documentation);
    if let Some(position) = &declaration.position {
        output.push_line_directive(position);
    }
    match &declaration.kind {
        DeclarationKind::Type {
            name,
            parameters,
            underlying,
        } => {
            write_type_header(&mut output, name, parameters);
            output.push_str(" ");
            write_underlying(&mut output, underlying);
        }
        DeclarationKind::Alias {
            name,
            parameters,
            underlying,
        } => {
            write_type_header(&mut output, name, parameters);
            output.push_str(" = ");
            write_underlying(&mut output, underlying);
        }
        DeclarationKind::Struct {
            name,
            parameters,
            fields,
        } => {
            write_type_header(&mut output, name, parameters);
            write_struct_body(&mut output, fields, declaration.position.as_ref());
        }
        DeclarationKind::Interface {
            name,
            parameters,
            parents,
            methods,
        } => {
            write_type_header(&mut output, name, parameters);
            write_interface_body(&mut output, parents, methods);
        }
        DeclarationKind::Constants(constants) => write_constants(&mut output, constants),
        DeclarationKind::Binding(binding) => render_lowered_block(
            &mut output,
            &LoweredBlock {
                statements: vec![binding.clone().into()],
            },
        ),
        DeclarationKind::Function { signature, body } => {
            write_function(&mut output, signature, body, declaration.position.as_ref())
        }
    }
    output.into_text()
}

fn write_function(
    output: &mut GoWriter,
    signature: &FunctionSignature,
    body: &LoweredBlock,
    position: Option<&Position>,
) {
    output.push_str("func ");
    if let Some(receiver) = &signature.receiver {
        write!(
            output,
            "({} {}) ",
            receiver.name,
            go_type_text(&receiver.go_type)
        )
        .unwrap();
    }
    output.push_str(&signature.name);
    generics(output, &signature.generics);
    output.push('(');
    let parameters = signature
        .parameters
        .iter()
        .map(|parameter| (parameter.name.to_string(), go_type_text(&parameter.go_type)))
        .collect::<Vec<_>>();
    output.push_str(&group_params(&parameters));
    output.push(')');
    if let Some(result) = &signature.result {
        write!(output, " {}", go_type_text(result)).unwrap();
    }
    if body.renders_empty() {
        output.push_str(" {}");
    } else {
        output.push_str(" {\n");
        render_positioned_body(output, body, position);
        output.push('}');
    }
}

fn write_struct_body(
    output: &mut GoWriter,
    fields: &[FieldDeclaration],
    position: Option<&Position>,
) {
    if fields.is_empty() {
        output.push_str(" struct{}");
        return;
    }
    output.push_str(" struct {\n");
    for field in fields {
        write_documentation(output, &field.documentation);
        if let Some(position) = position {
            output.push_line_directive(position);
        }
        if let Some(name) = &field.name {
            write!(output, "{name} ").unwrap();
        }
        output.push_str(&go_type_text(&field.ty));
        if let Some(tag) = &field.tag {
            output.push(' ');
            output.push_foreign(tag);
        }
        output.push('\n');
    }
    output.push('}');
}

fn write_interface_body(output: &mut GoWriter, parents: &[GoType], methods: &[InterfaceMethod]) {
    output.push_str(" interface {\n");
    for parent in parents {
        writeln!(output, "{}", go_type_text(parent)).unwrap();
    }
    for method in methods {
        write!(output, "{}(", method.name).unwrap();
        for (index, parameter) in method.parameters.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            output.push_str(&go_type_text(parameter));
        }
        output.push(')');
        if let Some(result) = &method.result {
            write!(output, " {}", go_type_text(result)).unwrap();
        }
        output.push('\n');
    }
    output.push('}');
}

fn write_constants(output: &mut GoWriter, constants: &[ConstantDeclaration]) {
    output.push_str("const (\n");
    for constant in constants {
        write_documentation(output, &constant.documentation);
        output.push_str(&constant.name);
        if let Some(ty) = &constant.ty {
            write!(output, " {}", go_type_text(ty)).unwrap();
        }
        if let Some(value) = &constant.value {
            output.push_str(" = ");
            value.write(output);
        }
        output.push('\n');
    }
    output.push(')');
}

fn write_underlying(output: &mut GoWriter, underlying: &GoType) {
    if let GoTypeShape::Struct(fields) = underlying.shape.as_ref()
        && !fields.is_empty()
    {
        output.push_str("struct {\n");
        for (name, ty) in fields {
            writeln!(output, "{name} {}", go_type_text(ty)).unwrap();
        }
        output.push('}');
    } else {
        output.push_str(&go_type_text(underlying));
    }
}

pub(crate) fn group_params(params: &[(String, String)]) -> String {
    if params.is_empty() {
        return String::new();
    }
    if params.len() == 1 {
        return format!("{} {}", params[0].0, params[0].1);
    }
    let mut parts: Vec<String> = Vec::new();
    let mut names: Vec<&str> = vec![&params[0].0];
    let mut current_ty = &params[0].1;

    for param in &params[1..] {
        if param.1 == *current_ty {
            names.push(&param.0);
        } else {
            parts.push(format!("{} {}", names.join(", "), current_ty));
            names.clear();
            names.push(&param.0);
            current_ty = &param.1;
        }
    }
    parts.push(format!("{} {}", names.join(", "), current_ty));
    parts.join(", ")
}
