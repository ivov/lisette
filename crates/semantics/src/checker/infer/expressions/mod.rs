pub mod aliasing;
pub mod bindings;
pub mod comparison;
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

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    /// Infer an expression nested inside another expression.
    pub fn infer_expression(&mut self, expression: Expression, expected_ty: &Type) -> Expression {
        self.infer_expression_at(expression, expected_ty, true)
    }

    /// Infer a statement or tail expression, where direct control flow is valid.
    pub fn infer_root_expression(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        self.infer_expression_at(expression, expected_ty, false)
    }

    pub(super) fn infer_expression_at(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
        is_subexpression: bool,
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

            Expression::Let { .. } => self.infer_let_binding(expression, expected_ty),

            Expression::Call {
                expression: ref callee,
                span,
                ..
            } => {
                let is_panic =
                    matches!(&**callee, Expression::Identifier { value, .. } if value == "panic");
                let result = self.infer_function_call(expression, expected_ty);
                if is_subexpression && is_panic {
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

            Expression::IfLet { .. } => self.infer_if_let(expression, expected_ty),

            Expression::Match {
                subject,
                arms,
                span,
                ..
            } => self.infer_match(subject, arms, span, expected_ty),

            Expression::Tuple { elements, span, .. } => {
                self.infer_tuple(elements, span, expected_ty)
            }

            Expression::StructCall { .. } => self.infer_struct_call(expression, expected_ty),

            Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } => self.infer_dot_access_or_qualified_path(expression, member, span, expected_ty),

            Expression::Enum { .. } => expression,

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
            } => self.infer_return_statement(expression, span, is_subexpression),

            Expression::Propagate {
                expression, span, ..
            } => {
                if is_subexpression {
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
            } => self.infer_paren(expression, span, expected_ty, is_subexpression),

            Expression::Unary {
                operator,
                expression,
                span,
                ..
            } => self.infer_unary(operator, expression, expected_ty, span),

            Expression::Const { .. } => self.infer_const_binding(expression),

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
                from_colon_syntax,
                ..
            } => {
                if from_colon_syntax {
                    self.infer_colon_subscript(expression, index, span)
                } else {
                    self.infer_indexed_access(expression, index, span, expected_ty)
                }
            }

            Expression::Task {
                expression, span, ..
            } => {
                // Only fire the generic ban when the dedicated
                // `task_in_expression_position` check won't — avoids duplicates.
                if is_subexpression && !self.scopes.is_value_context() {
                    self.sink
                        .push(diagnostics::infer::control_flow_in_expression("task", span));
                }
                self.infer_task(expression, span, expected_ty)
            }

            Expression::Defer {
                expression, span, ..
            } => {
                if is_subexpression && !self.scopes.is_value_context() {
                    self.sink
                        .push(diagnostics::infer::control_flow_in_expression(
                            "defer", span,
                        ));
                }
                self.infer_defer(expression, span, expected_ty)
            }

            Expression::Assert {
                expression, span, ..
            } => self.infer_assert(expression, span, expected_ty),

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

            Expression::Break { value, span } => self.infer_break(value, span, is_subexpression),
            Expression::Continue { span } => self.infer_continue(span, is_subexpression),
            Expression::RawGo { text } => Expression::RawGo { text },
        }
    }

    fn with_value_context<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let prev_ctx = self.scopes.set_value_context();
        let result = f(self);
        self.scopes.restore_use_context(prev_ctx);
        result
    }
}
