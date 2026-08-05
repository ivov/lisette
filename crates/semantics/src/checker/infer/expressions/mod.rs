pub mod aliasing;
pub mod bindings;
mod branches;
mod call_effects;
pub mod calls;
pub mod comparison;
pub mod control_flow;
pub mod definitions;
pub mod dot_access;
pub mod functions;
pub mod impl_blocks;
pub mod indexed_access;
pub mod literals;
mod loops;
mod method_access;
pub mod operators;
pub mod patterns;
pub mod primitives;
pub mod propagate;
mod qualified_path;
pub mod select;
pub mod struct_call;

use syntax::ast::{BinaryOperator, CallTypeArguments, Expression, Span};
use syntax::program::CallKind;
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
                operator: BinaryOperator::Pipeline,
                left,
                right,
                span,
                ..
            } => {
                let lowered = self.lower_pipeline(*left, *right, span);
                if matches!(
                    &lowered,
                    Expression::Unit {
                        ty: Type::Error,
                        ..
                    }
                ) {
                    lowered
                } else {
                    self.infer_expression_at(lowered, expected_ty, is_subexpression)
                }
            }

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
                self.check_control_flow_in_expression("task", span, is_subexpression);
                self.infer_task(expression, span, expected_ty)
            }

            Expression::Defer {
                expression, span, ..
            } => {
                self.check_control_flow_in_expression("defer", span, is_subexpression);
                self.infer_defer(expression, span, expected_ty)
            }

            Expression::Assert {
                expression, span, ..
            } => self.infer_assert(expression, span, expected_ty),

            Expression::Select { arms, span, .. } => self.infer_select(arms, span, expected_ty),

            Expression::PackageImport {
                name,
                name_span,
                alias,
                span,
            } => Expression::PackageImport {
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

    fn lower_pipeline(&mut self, left: Expression, right: Expression, span: Span) -> Expression {
        let mut segments = vec![(right, span)];
        let mut current = left;
        while let Expression::Binary {
            operator: BinaryOperator::Pipeline,
            left,
            right,
            span,
            ..
        } = current
        {
            segments.push((*right, span));
            current = *left;
        }

        let mut lowered = current;
        while let Some((right, span)) = segments.pop() {
            if matches!(
                &lowered,
                Expression::Unit {
                    ty: Type::Error,
                    ..
                }
            ) {
                lowered = Expression::Unit {
                    ty: Type::Error,
                    span,
                };
                continue;
            }
            lowered = self.lower_pipeline_step(lowered, right, span);
        }
        lowered
    }

    fn lower_pipeline_step(
        &mut self,
        left: Expression,
        right: Expression,
        span: Span,
    ) -> Expression {
        let right = Self::unwrap_parens_owned(right);
        let right = match right {
            Expression::Binary {
                operator: BinaryOperator::Pipeline,
                left,
                right,
                span,
                ..
            } => self.lower_pipeline(*left, *right, span),
            other => other,
        };

        match right {
            Expression::Identifier { .. } | Expression::DotAccess { .. } => Expression::Call {
                expression: Box::new(right),
                args: vec![left],
                spread: None,
                type_arguments: CallTypeArguments::none(),
                ty: Type::uninferred(),
                span,
                call_kind: CallKind::Unresolved,
            },
            Expression::Call {
                expression,
                args,
                spread,
                type_arguments,
                ty,
                call_kind,
                ..
            } => {
                let mut new_args = Vec::with_capacity(args.len() + 1);
                new_args.push(left);
                new_args.extend(args);
                Expression::Call {
                    expression,
                    args: new_args,
                    spread,
                    type_arguments,
                    ty,
                    span,
                    call_kind,
                }
            }
            Expression::Propagate {
                span: propagate_span,
                ..
            } => {
                self.sink
                    .push(diagnostics::infer::propagate_in_pipeline(propagate_span));
                Expression::Unit {
                    ty: Type::Error,
                    span,
                }
            }
            other => {
                self.sink.push(diagnostics::infer::invalid_pipeline_target(
                    other.get_span(),
                ));
                Expression::Unit {
                    ty: Type::Error,
                    span,
                }
            }
        }
    }

    fn unwrap_parens_owned(mut expression: Expression) -> Expression {
        loop {
            match expression {
                Expression::Paren {
                    expression: inner, ..
                } => expression = *inner,
                other => return other,
            }
        }
    }

    /// Bans `task`/`defer` as a bare subexpression, skipped when the dedicated check already fires.
    fn check_control_flow_in_expression(&mut self, kind: &str, span: Span, is_subexpression: bool) {
        if is_subexpression && !self.scopes.is_value_context() {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(kind, span));
        }
    }

    fn with_use_context<F, R>(&mut self, context: crate::checker::scopes::UseContext, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let prev_ctx = self.scopes.replace_use_context(context);
        let result = f(self);
        self.scopes.restore_use_context(prev_ctx);
        result
    }

    fn with_value_context<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.with_use_context(crate::checker::scopes::UseContext::Value, f)
    }

    fn with_dot_access_base<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let previous = self.scopes.replace_dot_access_base(true);
        let result = f(self);
        self.scopes.replace_dot_access_base(previous);
        result
    }

    fn with_pattern<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let previous = self.scopes.replace_in_pattern(true);
        let result = f(self);
        self.scopes.replace_in_pattern(previous);
        result
    }

    fn with_let_binding_rhs<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let previous = self.scopes.replace_let_binding_rhs(true);
        let result = f(self);
        self.scopes.replace_let_binding_rhs(previous);
        result
    }
}
