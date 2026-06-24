use crate::Planner;
use crate::definitions::functions::is_test_context_ty;
use crate::names::go_name::{self, testing_qualifier, testkit_qualifier};
use syntax::ast::{Binding, Expression, FunctionDefinitionView, Pattern, Visibility};
use syntax::types::{Symbol, Type};

impl Planner<'_> {
    pub(crate) fn emit_top_item(&mut self, item: &Expression) -> String {
        match item {
            Expression::Function {
                doc,
                visibility,
                name,
                name_span,
                ..
            } => {
                if self.facts.is_unused_definition(name_span) {
                    return String::new();
                }
                let is_public = matches!(visibility, Visibility::Public);
                let function = item.function_definition_view();
                let doc_comment = emit_doc(doc);

                if self.facts.is_test(&self.facts.qualified_current(name)) {
                    self.emit_test_function(name, function, is_public, &doc_comment)
                } else {
                    let code = self.emit_function(function, None, is_public);
                    format!("{}{}", doc_comment, code)
                }
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
                let code = self.emit_struct_definition(name, generics, fields, kind, attributes);
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
                    .emit_enum(name, generics, attributes)
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
                let code = self.emit_type_alias(name, generics, ty);
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
                    self.emit_interface(name, method_signatures, parents, generics, is_public);
                format!("{}{}", doc_comment, code)
            }
            Expression::ImplBlock {
                receiver_name,
                ty,
                methods,
                generics,
                ..
            } => self.emit_impl_block(receiver_name, ty, methods, generics),
            Expression::Const {
                doc,
                identifier,
                expression,
                ty,
                ..
            } => {
                let doc_comment = emit_doc(doc);
                let code = self.emit_const(identifier, expression, ty);
                format!("{}{}", doc_comment, code)
            }
            _ => String::new(),
        }
    }

    fn emit_test_function(
        &mut self,
        name: &str,
        function: FunctionDefinitionView<'_>,
        is_public: bool,
        doc_comment: &str,
    ) -> String {
        self.require_testing();
        self.require_testkit();
        let test_kit = testkit_qualifier();
        let testing = testing_qualifier();

        let injected = self.synthesized_test_handle_params(function);
        let function = match &injected {
            Some(params) => FunctionDefinitionView { params, ..function },
            None => function,
        };

        let callee = self.pick_go_function_name(function, false, is_public);
        let test_name = go_name::go_test_function_name(name);
        let handle = go_name::TEST_T_PARAM;
        let code = self.emit_function(function, None, is_public);

        let span = function.name_span;
        let recover = format!(
            "defer {test_kit}.Recover({handle}, {}, {}, {})",
            span.file_id,
            span.byte_offset,
            span.byte_offset + span.byte_length,
        );
        let call = format!("{callee}({test_kit}.New({handle}))");
        let body = if function.return_type.is_result() {
            format!(
                "if err := {call}; err != nil {{\n\t\t{test_kit}.Fail({handle}, {}, {}, {}, \"result_err\", \"test returned Err\", {test_kit}.ErrOperand(err))\n\t}}",
                span.file_id,
                span.byte_offset,
                span.byte_offset + span.byte_length,
            )
        } else {
            call
        };
        let wrapper =
            format!("func {test_name}({handle} *{testing}.T) {{\n\t{recover}\n\t{body}\n}}");
        format!("{doc_comment}{code}\n\n{wrapper}")
    }

    fn synthesized_test_handle_params(
        &self,
        function: FunctionDefinitionView<'_>,
    ) -> Option<Vec<Binding>> {
        let has_usable_handle = function.params.iter().any(|param| {
            is_test_context_ty(&param.ty) && self.go_name_for_binding(&param.pattern).is_some()
        });
        if has_usable_handle {
            return None;
        }
        Some(vec![Binding {
            pattern: Pattern::Identifier {
                identifier: go_name::TEST_CTX_PARAM.into(),
                span: function.name_span,
            },
            annotation: None,
            typed_pattern: None,
            ty: Type::Nominal {
                id: Symbol::from_parts(go_name::TEST_PRELUDE_MODULE, "TestContext"),
                params: vec![],
                underlying_ty: None,
            },
            mutable: false,
        }])
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
