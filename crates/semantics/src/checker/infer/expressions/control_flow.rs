use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span};
use syntax::types::{SimpleKind, Type};

use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(super) fn infer_condition(&mut self, condition: Expression, span: &Span) -> Expression {
        let cond_ty = self.new_type_var();
        let inferred = self.infer_expression(condition, &cond_ty);
        if self
            .store
            .underlying_simple_kind(&cond_ty.resolve_in(&self.env))
            != Some(SimpleKind::Bool)
        {
            let bool_ty = self.type_bool();
            self.unify(&bool_ty, &cond_ty, span);
        }
        inferred
    }

    pub(super) fn infer_return_statement(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        is_subexpression: bool,
    ) -> Expression {
        if is_subexpression {
            self.sink
                .push(diagnostics::infer::control_flow_in_expression(
                    "return", span,
                ));
        }
        self.check_return_in_try_block(span);
        self.check_return_in_recover_block(span);
        self.check_return_in_defer_block(span);
        match &*expression {
            Expression::Break { span: s, .. } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression("break", *s));
            }
            Expression::Continue { span: s } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression(
                        "continue", *s,
                    ));
            }
            Expression::Return { span: s, .. } => {
                self.sink
                    .push(diagnostics::infer::control_flow_in_expression("return", *s));
            }
            _ => {}
        }
        self.infer_return(expression, span)
    }

    fn infer_return(&mut self, expression: Box<Expression>, span: Span) -> Expression {
        let return_ty = self
            .scopes
            .lookup_fn_return_type()
            .cloned()
            .unwrap_or_else(|| {
                self.sink
                    .push(diagnostics::infer::return_outside_function(span));
                Type::Error
            });

        let new_expression =
            self.with_value_context(|s| s.infer_root_expression(*expression, &return_ty));

        Expression::Return {
            expression: new_expression.into(),
            ty: self.type_never(),
            span,
        }
    }

    pub(super) fn infer_defer(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if self.is_value_context() {
            self.sink
                .push(diagnostics::infer::defer_in_expression_position(span));
        }

        if self.is_inside_loop() {
            self.sink.push(diagnostics::infer::defer_in_loop(span));
        }

        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        let is_block = matches!(*expression, Expression::Block { .. });
        let defer_ty = self.new_type_var();
        let new_expression = if is_block {
            self.in_defer_block(|this| this.infer_expression(*expression, &defer_ty))
        } else {
            self.infer_expression(*expression, &defer_ty)
        };

        if let Some(propagate_span) = Self::find_propagate(&new_expression) {
            self.sink
                .push(diagnostics::infer::propagate_in_defer(propagate_span));
        }

        self.check_deferred_lock(&new_expression);

        Expression::Defer {
            expression: new_expression.into(),
            ty: self.type_unit(),
            span,
        }
    }

    pub(super) fn infer_assert(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let new_expression = self.infer_condition(*expression, &span);
        if let Some(propagate_span) = Self::find_propagate(&new_expression) {
            self.sink
                .push(diagnostics::infer::propagate_in_assert(propagate_span));
        }
        if !self.scopes.has_test_handle() {
            self.sink
                .push(diagnostics::infer::assert_without_test_context(span));
        }
        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        Expression::Assert {
            expression: new_expression.into(),
            ty: unit_ty,
            span,
        }
    }

    pub(crate) fn find_propagate(expression: &Expression) -> Option<Span> {
        if let Expression::Propagate { span, .. } = expression {
            return Some(*span);
        }
        expression
            .children()
            .into_iter()
            .find_map(Self::find_propagate)
    }

    pub(super) fn infer_task(
        &mut self,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if self.is_value_context() {
            self.sink
                .push(diagnostics::infer::task_in_expression_position(span));
        }

        let unit_ty = self.type_unit();
        self.unify(expected_ty, &unit_ty, &span);

        // task spawns a new goroutine, enclosing loop context doesn't apply
        let task_ty = self.new_type_var();
        let new_expression =
            self.without_enclosing_loop(|this| this.infer_expression(*expression, &task_ty));

        Expression::Task {
            expression: new_expression.into(),
            ty: self.type_unit(),
            span,
        }
    }
}
