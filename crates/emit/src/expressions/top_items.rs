use crate::EmitEffects;
use crate::Planner;
use syntax::ast::{Expression, Visibility};

impl Planner<'_> {
    pub(crate) fn emit_top_item(&mut self, item: &Expression, fx: &mut EmitEffects) -> String {
        match item {
            Expression::Function {
                doc,
                visibility,
                name_span,
                ..
            } => {
                if self.facts.is_unused_definition(name_span) {
                    return String::new();
                }
                let is_public = matches!(visibility, Visibility::Public);
                let function = item.function_definition_view();
                let doc_comment = emit_doc(doc);

                let code = self.emit_function(function, None, is_public, fx);
                format!("{}{}", doc_comment, code)
            }
            Expression::Struct {
                doc,
                attributes,
                name,
                generics,
                fields,
                kind,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let code =
                    self.emit_struct_definition(name, generics, fields, kind, attributes, fx);
                format!("{}{}", doc_comment, code)
            }
            Expression::Enum {
                doc,
                attributes,
                name,
                generics,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let code = self
                    .emit_enum(name, generics, attributes, fx)
                    .unwrap_or_default();
                format!("{}{}", doc_comment, code)
            }
            Expression::TypeAlias {
                doc,
                name,
                generics,
                ty,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let code = self.emit_type_alias(name, generics, ty, fx);
                format!("{}{}", doc_comment, code)
            }
            Expression::Interface {
                doc,
                name,
                method_signatures,
                parents,
                generics,
                visibility,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let is_public = matches!(visibility, Visibility::Public);
                let code =
                    self.emit_interface(name, method_signatures, parents, generics, is_public, fx);
                format!("{}{}", doc_comment, code)
            }
            Expression::ImplBlock {
                receiver_name,
                ty,
                methods,
                generics,
                ..
            } => self.emit_impl_block(receiver_name, ty, methods, generics, fx),
            Expression::Const {
                doc,
                identifier,
                expression,
                ty,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let code = self.emit_const(identifier, expression, ty, fx);
                format!("{}{}", doc_comment, code)
            }
            _ => String::new(),
        }
    }
}

pub(crate) fn emit_doc(doc: &Option<String>) -> String {
    match doc {
        Some(text) => {
            let lines: Vec<String> = text
                .lines()
                .map(|line| {
                    if line.is_empty() {
                        "//".to_string()
                    } else {
                        format!("// {}", line)
                    }
                })
                .collect();
            if lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", lines.join("\n"))
            }
        }
        None => String::new(),
    }
}
