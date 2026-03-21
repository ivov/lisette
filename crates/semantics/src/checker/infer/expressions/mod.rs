pub mod bindings;
pub mod control_flow;
pub mod definitions;
pub mod dot_access;
pub mod functions;
pub mod impl_blocks;
pub mod indexed_access;
pub mod literals;
pub mod operators;
pub mod patterns;
pub mod primitives;
pub mod propagate;
pub mod select;
pub mod struct_call;

use syntax::ast::Expression;
use syntax::types::Type;

use super::super::Checker;

impl Checker<'_, '_> {
    pub fn infer_expression(&mut self, expression: Expression, expected_ty: &Type) -> Expression {
        // Track sub-expression depth: `infer_block_items` resets this to false
        // for each top-level statement, so any nested call sees `true`.
        let parent_is_subexpression = std::mem::replace(&mut self.inference.in_subexpression, true);

        let result = self.infer_expression_inner(expression, expected_ty, parent_is_subexpression);

        self.inference.in_subexpression = parent_is_subexpression;
        result
    }

    fn infer_expression_inner(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
        parent_is_subexpression: bool,
    ) -> Expression {
        match expression {
            Expression::Literal { literal, span, .. } => {
                self.infer_literal(literal, expected_ty, span)
            }

            Expression::Block { items, span, .. } => self.infer_block(items, span, expected_ty),

            Expression::Function { .. } => self.infer_function(expression, expected_ty),

            Expression::Lambda {
                params,
                return_annotation,
                body,
                span,
                ..
            } => self.infer_lambda(params, return_annotation, body, span, expected_ty),

            Expression::Unit { span, .. } => self.infer_unit(span, expected_ty),

            Expression::Identifier {
                ref value, span, ..
            } => self.infer_identifier(value.clone(), span, expected_ty),

            Expression::Let {
                binding,
                value,
                mutable,
                mut_span,
                else_block,
                else_span,
                span,
                typed_pattern: _,
                ty: _,
            } => self.infer_let_binding(
                *binding,
                value,
                mutable,
                mut_span,
                else_block,
                else_span,
                span,
                expected_ty,
            ),

            Expression::Call {
                expression,
                args: call_args,
                type_args,
                span,
                ..
            } => {
                let is_panic = matches!(&*expression, Expression::Identifier { value, .. } if value == "panic");
                let result =
                    self.infer_function_call(expression, call_args, type_args, span, expected_ty);
                if parent_is_subexpression && is_panic {
                    self.sink
                        .push(diagnostics::infer::never_call_in_expression(span));
                }
                result
            }

            Expression::If {
                condition,
                consequence,
                alternative,
                span,
                ..
            } => self.infer_if(condition, consequence, alternative, span, expected_ty),

            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before type inference")
            }

            Expression::Match {
                subject,
                arms,
                origin,
                span,
                ..
            } => self.infer_match(subject, arms, origin, span, expected_ty),

            Expression::Tuple { elements, span, .. } => {
                self.infer_tuple(elements, span, expected_ty)
            }

            Expression::StructCall {
                name,
                field_assignments,
                spread,
                span,
                ..
            } => self.infer_struct_call(name, field_assignments, spread, span, expected_ty),

            Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } => self.infer_dot_access_or_qualified_path(expression, member, span, expected_ty),

            Expression::Enum { .. } => self.infer_enum_definition(expression),

            Expression::ValueEnum { .. } => self.infer_value_enum_definition(expression),

            Expression::Struct { .. } => self.infer_struct_definition(expression),

            Expression::TypeAlias { .. } => self.infer_type_alias_definition(expression),

            Expression::VariableDeclaration { .. } => expression,

            Expression::ImplBlock {
                annotation,
                ty: _,
                methods,
                receiver_name,
                generics,
                span,
            } => self.infer_impl_block(annotation, methods, receiver_name, generics, span),

            Expression::Interface { .. } => self.infer_interface(expression),

            Expression::Assignment {
                target,
                value,
                compound_operator,
                span,
            } => self.infer_assignment(target, value, compound_operator, span),

            Expression::Return {
                expression, span, ..
            } => self.infer_return_statement(expression, span, parent_is_subexpression),

            Expression::Propagate {
                expression, span, ..
            } => {
                if parent_is_subexpression {
                    self.check_failure_propagation_in_subexpression(&expression, span);
                }
                self.infer_propagate(expression, span, expected_ty)
            }

            Expression::TryBlock {
                items,
                try_keyword_span,
                span,
                ..
            } => self.infer_try_block(items, try_keyword_span, span, expected_ty),

            Expression::RecoverBlock {
                items,
                recover_keyword_span,
                span,
                ..
            } => self.infer_recover_block(items, recover_keyword_span, span, expected_ty),

            Expression::Binary {
                operator,
                left,
                right,
                span,
                ..
            } => self.infer_binary(operator, left, right, expected_ty, span),

            Expression::Paren {
                expression, span, ..
            } => self.infer_paren(expression, span, expected_ty, parent_is_subexpression),

            Expression::Unary {
                operator,
                expression,
                span,
                ..
            } => self.infer_unary(operator, expression, expected_ty, span),

            Expression::Const {
                doc,
                annotation,
                ty: _,
                expression,
                span,
                identifier,
                identifier_span,
                visibility,
            } => self.infer_const_binding(
                doc,
                annotation,
                expression,
                identifier,
                identifier_span,
                visibility,
                span,
            ),

            Expression::Loop { body, span, .. } => self.infer_loop(body, span, expected_ty),

            Expression::While {
                condition,
                body,
                span,
                ..
            } => self.infer_while(condition, body, span, expected_ty),

            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                span,
                ..
            } => self.infer_while_let(pattern, scrutinee, body, span, expected_ty),

            Expression::For {
                binding,
                iterable,
                body,
                span,
                ..
            } => self.infer_for(*binding, iterable, body, span, expected_ty),

            Expression::Reference {
                expression, span, ..
            } => self.infer_reference(expression, span, expected_ty),

            Expression::IndexedAccess {
                expression,
                index,
                span,
                ..
            } => self.infer_indexed_access(expression, index, span, expected_ty),

            Expression::Task {
                expression, span, ..
            } => {
                // Only fire the generic ban when the dedicated
                // `task_in_expression_position` check won't — avoids duplicates.
                if parent_is_subexpression && !self.scopes.is_value_context() {
                    self.sink
                        .push(diagnostics::infer::control_flow_in_expression("task", span));
                }
                self.infer_task(expression, span, expected_ty)
            }

            Expression::Defer {
                expression, span, ..
            } => {
                if parent_is_subexpression && !self.scopes.is_value_context() {
                    self.sink
                        .push(diagnostics::infer::control_flow_in_expression(
                            "defer", span,
                        ));
                }
                self.infer_defer(expression, span, expected_ty)
            }

            Expression::Select { arms, span, .. } => self.infer_select(arms, span, expected_ty),

            Expression::ModuleImport {
                name,
                name_span,
                alias,
                span,
            } => Expression::ModuleImport {
                name,
                name_span,
                alias,
                span,
            },

            Expression::Range {
                start,
                end,
                inclusive,
                span,
                ..
            } => self.infer_range(start, end, inclusive, span, expected_ty),

            Expression::Cast {
                expression,
                target_type,
                span,
                ..
            } => self.infer_cast(expression, target_type, span, expected_ty),

            Expression::Break { value, span } => {
                self.infer_break(value, span, parent_is_subexpression)
            }
            Expression::Continue { span } => self.infer_continue(span, parent_is_subexpression),
            Expression::RawGo { text } => Expression::RawGo { text },
            Expression::NoOp => Expression::NoOp,
        }
    }

    pub(super) fn with_value_context<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let prev_ctx = self.scopes.set_value_context();
        let result = f(self);
        self.scopes.restore_use_context(prev_ctx);
        result
    }
}
