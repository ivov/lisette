use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::return_abi;
use crate::output::render::output::doc_lines;
use crate::target::ir::bodies::async_statement;
use crate::target::ir::bodies::effect_statement;
use crate::target::ir::bodies::{
    AsyncKind, Definition, ElseArm, IfPlan, LoweredBlock, LoweredStatement,
};
use crate::target::ir::declarations::GoDeclaration;
use crate::target::ir::go_expression::{BinaryOp, GoParameter};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::{self, testing_qualifier, testkit_qualifier};
use crate::target::names::go_name::{GeneratedPackage, GoExport};
use crate::target::names::packages::PackageUse;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi};
use syntax::ast::{Binding, Expression, FunctionDefinitionView, Pattern};
use syntax::types::Type;

impl Planner<'_> {
    pub(in crate::lowering) fn emit_top_item(&mut self, item: &Expression) -> Vec<GoDeclaration> {
        self.focused(item.get_span(), |this| this.emit_focused_top_item(item))
    }

    fn emit_focused_top_item(&mut self, item: &Expression) -> Vec<GoDeclaration> {
        match item {
            Expression::Function {
                doc,
                visibility,
                name,
                ..
            } => self.emit_function_item(item, doc, name, GoExport::from_visibility(*visibility)),
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
                let export = GoExport::from_visibility(*visibility);
                let mut declaration =
                    self.emit_interface(name, method_signatures, parents, generics, export);
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
        export: GoExport,
    ) -> Vec<GoDeclaration> {
        let function = item.function_definition_view();
        let go_name = self.free_function_go_name(function.name, export);
        let doc_comment = item_doc_text(doc, name, &go_name);
        if self.facts.is_test(&self.facts.qualified_current(name)) {
            return self.emit_test_function(name, function, export, &doc_comment);
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
        export: GoExport,
        doc_comment: &str,
    ) -> Vec<GoDeclaration> {
        let injected = synthesized_test_handle_params(&self.facts, function);
        let function = match &injected {
            Some(params) => FunctionDefinitionView { params, ..function },
            None => function,
        };
        let callee = self.free_function_go_name(function.name, export);
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
        let mut statements = vec![async_statement(
            AsyncKind::Defer,
            testkit("Recover", recover_args),
        )];
        let call = GoExpression::call(
            GoExpression::external_name(callee),
            vec![testkit("New", vec![handle_value.clone()])],
        );
        if matches!(
            return_abi(&self.facts, function.return_type, ReturnSlot::Function),
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
                    statements: vec![effect_statement(testkit("Fail", arguments))],
                },
                ElseArm::None,
            );

            check.initializer = Some(Definition {
                names: vec![error_name],
                value: call,
            });
            statements.push(LoweredStatement::If(check).into());
        } else {
            statements.push(effect_statement(call));
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

pub(in crate::lowering) fn item_doc_text(
    doc: &Option<String>,
    lisette_name: &str,
    go_name: &str,
) -> String {
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

fn synthesized_test_handle_params(
    facts: &EmitFacts<'_>,
    function: FunctionDefinitionView<'_>,
) -> Option<Vec<Binding>> {
    let has_usable_handle = function.params.iter().any(|param| {
        param.ty.is_test_context() && facts.go_name_for_binding(&param.pattern).is_some()
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
