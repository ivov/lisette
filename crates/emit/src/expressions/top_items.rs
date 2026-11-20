use crate::EmitFacts;
use crate::Planner;
use crate::abi::ReturnSlot;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi};
use crate::ir::bodies::{
    AsyncKind, Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement, expression_statement,
};
use crate::ir::declarations::GoDeclaration;
use crate::ir::go_expression::{BinaryOp, GoParameter};
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::go_name::GeneratedPackage;
use crate::names::go_name::{self, testing_qualifier, testkit_qualifier};
use crate::names::packages::PackageUse;
use crate::types::go_type::GoType;
use syntax::ast::{Binding, Expression, FunctionDefinitionView, Pattern, Visibility};
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn emit_top_item(&mut self, item: &Expression) -> Vec<GoDeclaration> {
        self.focused(item.get_span(), |this| this.emit_focused_top_item(item))
    }

    fn emit_focused_top_item(&mut self, item: &Expression) -> Vec<GoDeclaration> {
        match item {
            Expression::Function {
                doc,
                visibility,
                name,
                ..
            } => self.emit_function_item(item, doc, name, matches!(visibility, Visibility::Public)),
            Expression::Struct {
                doc,
                attributes,
                name,
                name_span,
                generics,
                fields,
                ..
            } => {
                let doc_comment = item_doc_text(doc, name, &go_name::escape_type_name(name));
                let mut declarations =
                    self.emit_struct_definition(name, generics, fields, attributes);
                declarations[0].documentation = doc_comment.into();
                for declaration in &mut declarations {
                    declaration.position = self.facts.source_position(name_span);
                }
                declarations
            }
            Expression::Enum {
                doc,
                attributes,
                name,
                name_span,
                generics,
                ..
            } => {
                let doc_comment = item_doc_text(doc, name, &go_name::escape_type_name(name));
                let mut declarations = self
                    .try_emit_enum(name, generics, attributes, &doc_comment)
                    .unwrap_or_default();
                for declaration in &mut declarations {
                    declaration.position = self.facts.source_position(name_span);
                }
                declarations
            }
            Expression::TypeAlias {
                doc,
                name,
                generics,
                ty,
                ..
            } => {
                if self.facts.alias_emits_nothing(name) {
                    return Vec::new();
                }
                let doc_comment = item_doc_text(doc, name, &go_name::escape_type_name(name));
                let mut declaration = self.emit_type_alias(name, generics, ty);
                declaration.documentation = doc_comment.into();
                vec![declaration]
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
                let doc_comment = item_doc_text(doc, name, &go_name::escape_type_name(name));
                let is_public = matches!(visibility, Visibility::Public);
                let mut declaration =
                    self.emit_interface(name, method_signatures, parents, generics, is_public);
                declaration.documentation = doc_comment.into();
                vec![declaration]
            }
            Expression::ImplBlock {
                receiver_name,
                ty,
                methods,
                generics,
                ..
            } => {
                let receiver_generics = self.facts.checked(item).receiver_generics();
                self.emit_impl_block(receiver_name, ty, methods, generics, receiver_generics)
            }
            Expression::Const {
                doc,
                identifier,
                expression,
                ty,
                ..
            } => {
                let Some(expression) = expression.value() else {
                    return Vec::new();
                };
                vec![self.emit_const(identifier, expression, ty, doc)]
            }
            _ => Vec::new(),
        }
    }

    fn emit_function_item(
        &mut self,
        item: &Expression,
        doc: &Option<String>,
        name: &str,
        is_public: bool,
    ) -> Vec<GoDeclaration> {
        let function = item.function_definition_view();
        let go_name = self.pick_go_function_name(function, false, is_public);
        let doc_comment = item_doc_text(doc, name, &go_name);
        if self.facts.is_test(&self.facts.qualified_current(name)) {
            return self.emit_test_function(name, function, is_public, &doc_comment);
        }
        let mut declarations = self.emit_function(function, None, &go_name, None);
        if let Some(first) = declarations.first_mut() {
            first.documentation = doc_comment.into();
        }
        declarations
    }

    fn emit_test_function(
        &mut self,
        name: &str,
        function: FunctionDefinitionView<'_>,
        is_public: bool,
        doc_comment: &str,
    ) -> Vec<GoDeclaration> {
        let injected = self.facts.synthesized_test_handle_params(function);
        let function = match &injected {
            Some(params) => FunctionDefinitionView { params, ..function },
            None => function,
        };
        let callee = self.pick_go_function_name(function, false, is_public);
        let handle = go_name::fresh_suffixed("t", |name| {
            name == callee || name == testkit_qualifier() || name == testing_qualifier()
        });
        let mut declarations = self.emit_function(function, None, &callee, None);
        if let Some(first) = declarations.first_mut() {
            first.documentation = doc_comment.to_string().into();
        }
        let handle = self.scope.new_local_identifier(handle);
        declarations.push(self.emit_test_wrapper(name, function, callee, handle));
        declarations
    }

    fn emit_test_wrapper(
        &mut self,
        name: &str,
        function: FunctionDefinitionView<'_>,
        callee: String,
        handle: GoIdentifier,
    ) -> GoDeclaration {
        let handle_value = GoExpression::identifier(handle.clone());
        let span = function.name_span;
        let location = || {
            [
                span.file_id,
                span.byte_offset,
                span.byte_offset + span.byte_length,
            ]
            .map(|value| GoExpression::literal(value.to_string()))
        };
        let testkit = |name, arguments| {
            GoExpression::call(
                GoExpression::generated(GeneratedPackage::TestKit, name),
                arguments,
            )
        };
        let mut recover_args = vec![handle_value.clone()];
        recover_args.extend(location());
        let mut statements = vec![
            LoweredStatement::Async {
                kind: AsyncKind::Defer,
                call: testkit("Recover", recover_args),
            }
            .into(),
        ];
        let call = GoExpression::call(
            GoExpression::external_name(callee),
            vec![testkit("New", vec![handle_value.clone()])],
        );
        if matches!(
            self.facts
                .return_abi(function.return_type, ReturnSlot::Function),
            CallableReturnAbi::Lowered(LoweredReturnAbi::BareError)
        ) {
            let error_name = self.scope.new_local_identifier("err".to_string());
            let error = GoExpression::identifier(error_name.clone());
            let mut arguments = vec![handle_value];
            arguments.extend(location());
            arguments.extend([
                GoExpression::literal("\"result_err\"".to_string()),
                GoExpression::literal("\"test returned Err\"".to_string()),
                testkit("ErrOperand", vec![error.clone()]),
            ]);
            let mut check = IfPlan::plain(
                GoExpression::binary(error, BinaryOp::Ne, GoExpression::nil()),
                LoweredBlock {
                    statements: vec![expression_statement(testkit("Fail", arguments))],
                },
                ElseArm::None,
            );
            check.initializer = Some(Definition {
                names: vec![error_name],
                value: call,
            });
            statements.push(LoweredStatement::If(check).into());
        } else {
            statements.push(expression_statement(call));
        }
        GoDeclaration::function(
            None,
            go_name::go_test_function_name(name),
            Vec::new(),
            vec![GoParameter::new(
                handle,
                GoType::pointer_to(&GoType::named(
                    "T",
                    &[],
                    Some(PackageUse::generated(GeneratedPackage::Testing)),
                )),
            )],
            GoType::default(),
            LoweredBlock { statements },
        )
    }
}

pub(crate) fn doc_text(doc: &Option<String>) -> String {
    doc.as_deref().map_or_else(String::new, doc_lines)
}

pub(crate) fn item_doc_text(doc: &Option<String>, lisette_name: &str, go_name: &str) -> String {
    let Some(text) = doc else {
        return String::new();
    };
    let first_word = text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default();
    if first_word == lisette_name && lisette_name != go_name {
        doc_lines(&format!("{go_name}{}", &text[first_word.len()..]))
    } else {
        doc_lines(text)
    }
}

fn doc_lines(text: &str) -> String {
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                "//".to_string()
            } else {
                format!("// {}", line)
            }
        })
        .collect();
    format!("{}\n", lines.join("\n"))
}

impl EmitFacts<'_> {
    fn synthesized_test_handle_params(
        &self,
        function: FunctionDefinitionView<'_>,
    ) -> Option<Vec<Binding>> {
        let has_usable_handle = function.params.iter().any(|param| {
            param.ty.is_test_context() && self.go_name_for_binding(&param.pattern).is_some()
        });
        if has_usable_handle {
            return None;
        }
        Some(vec![Binding {
            pattern: Pattern::Identifier {
                identifier: go_name::TEST_CTX_PARAM.into(),
                span: function.name_span,
                binding: None,
            },
            annotation: None,
            ty: Type::test_context(),
            mut_span: None,
        }])
    }
}
