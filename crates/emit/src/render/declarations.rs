use crate::ir::bodies::{LoweredBlock, for_each_statements_mut};
use crate::ir::declarations::{DeclarationKind, Documentation, GoDeclaration, TypeParameter};
use crate::ir::go_expression::Position;
use crate::render::{Renderer, writer::GoWriter};
use crate::types::go_type::{GoType, GoTypeShape};
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
            [bound] => output.push_str(&bound.code),
            bounds => {
                output.push_str("interface { ");
                for (index, bound) in bounds.iter().enumerate() {
                    if index > 0 {
                        output.push_str("; ");
                    }
                    output.push_str(&bound.code);
                }
                output.push_str(" }");
            }
        }
    }
    output.push(']');
}
impl Renderer {
    fn render_positioned_body(
        &self,
        output: &mut GoWriter,
        body: &LoweredBlock,
        position: Option<&Position>,
    ) {
        let Some(position) = position else {
            self.render_lowered_block(output, body);
            return;
        };
        let mut body = body.clone();
        for_each_statements_mut(&mut body.statements, &mut |statements| {
            for statement in statements {
                statement.line.get_or_insert_with(|| position.clone());
            }
        });
        self.render_lowered_block(output, &body);
    }

    pub(crate) fn render_declaration(&self, declaration: &GoDeclaration) -> String {
        let mut output = GoWriter::new(true);
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
                write!(output, "type {name}").unwrap();
                generics(&mut output, parameters);
                output.push_str(" ");
                write_underlying(&mut output, underlying);
            }
            DeclarationKind::Alias {
                name,
                parameters,
                underlying,
            } => {
                write!(output, "type {name}").unwrap();
                generics(&mut output, parameters);
                output.push_str(" = ");
                write_underlying(&mut output, underlying);
            }
            DeclarationKind::Struct {
                name,
                parameters,
                fields,
            } => {
                write!(output, "type {name}").unwrap();
                generics(&mut output, parameters);
                if fields.is_empty() {
                    output.push_str(" struct{}");
                } else {
                    output.push_str(" struct {\n");
                    for field in fields {
                        write_documentation(&mut output, &field.documentation);
                        if let Some(position) = &declaration.position {
                            output.push_line_directive(position);
                        }
                        if let Some(name) = &field.name {
                            write!(output, "{name} ").unwrap();
                        }
                        output.push_str(&field.ty.code);
                        if let Some(tag) = &field.tag {
                            output.push(' ');
                            output.push_foreign(tag);
                        }
                        output.push('\n');
                    }
                    output.push('}');
                }
            }
            DeclarationKind::Interface {
                name,
                parameters,
                parents,
                methods,
            } => {
                write!(output, "type {name}").unwrap();
                generics(&mut output, parameters);
                output.push_str(" interface {\n");
                for parent in parents {
                    writeln!(output, "{}", parent.code).unwrap();
                }
                for method in methods {
                    write!(output, "{}(", method.name).unwrap();
                    for (index, parameter) in method.parameters.iter().enumerate() {
                        if index > 0 {
                            output.push_str(", ");
                        }
                        output.push_str(&parameter.code);
                    }
                    output.push(')');
                    if !method.result.is_empty() {
                        write!(output, " {}", method.result.code).unwrap();
                    }
                    output.push('\n');
                }
                output.push('}');
            }
            DeclarationKind::Constants(constants) => {
                output.push_str("const (\n");
                for constant in constants {
                    write_documentation(&mut output, &constant.documentation);
                    output.push_str(&constant.name);
                    if let Some(ty) = &constant.ty {
                        write!(output, " {}", ty.code).unwrap();
                    }
                    if let Some(value) = &constant.value {
                        output.push_str(" = ");
                        value.write(&mut output);
                    }
                    output.push('\n');
                }
                output.push(')');
            }
            DeclarationKind::Binding(binding) => self.render_lowered_block(
                &mut output,
                &LoweredBlock {
                    statements: vec![binding.clone().into()],
                },
            ),
            DeclarationKind::Function { signature, body } => {
                output.push_str("func ");
                if let Some(receiver) = &signature.receiver {
                    write!(output, "({} {}) ", receiver.name, receiver.go_type.code).unwrap();
                }
                output.push_str(&signature.name);
                generics(&mut output, &signature.generics);
                output.push('(');
                let parameters = signature
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.name.to_string(), parameter.go_type.code.clone()))
                    .collect::<Vec<_>>();
                output.push_str(&group_params(&parameters));
                output.push(')');
                if !signature.result.is_empty() {
                    write!(output, " {}", signature.result.code).unwrap();
                }
                if body.renders_empty() {
                    output.push_str(" {}");
                } else {
                    output.push_str(" {\n");
                    self.render_positioned_body(&mut output, body, declaration.position.as_ref());
                    output.push('}');
                }
            }
        }
        output.into_text()
    }
}

fn write_underlying(output: &mut GoWriter, underlying: &GoType) {
    if let GoTypeShape::Struct(fields) = underlying.shape.as_ref()
        && !fields.is_empty()
    {
        output.push_str("struct {\n");
        for (name, ty) in fields {
            writeln!(output, "{name} {}", ty.code).unwrap();
        }
        output.push('}');
    } else {
        output.push_str(&underlying.code);
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
