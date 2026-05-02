use std::cell::Cell;

use crate::checker::EnvResolve;
use crate::checker::scopes::{CarrierKind, DepthCounter, RecoverBlockContext, TryBlockContext};
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::program::Visibility;
use syntax::types::Type;

use super::super::TaskState;

impl TaskState<'_> {
    pub(super) fn infer_propagate(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if self.scopes.lookup_recover_block_context().is_some()
            && self.scopes.lookup_try_block_context().is_none()
        {
            self.sink
                .push(diagnostics::infer::recover_cannot_use_question_mark(span));
        }

        let tried_ty = self.new_type_var();
        let new_expression = self.infer_expression(store, *expression, &tried_ty);
        let resolved_tried_ty = new_expression.get_type().resolve_in(&self.env);

        if resolved_tried_ty.is_partial() {
            self.sink
                .push(diagnostics::infer::propagate_on_partial(span));
        }

        let try_block_types = if let Some(ctx) = self.scopes.lookup_try_block_context() {
            let is_result = resolved_tried_ty.is_result();
            let is_option = resolved_tried_ty.is_option();

            ctx.has_question_mark.set(true);

            let has_mismatch = match (is_result, is_option, ctx.carrier.get()) {
                (true, _, None) => {
                    ctx.carrier.set(Some(CarrierKind::Result));
                    false
                }
                (_, true, None) => {
                    ctx.carrier.set(Some(CarrierKind::Option));
                    false
                }
                (true, _, Some(CarrierKind::Option)) | (_, true, Some(CarrierKind::Result)) => true,
                _ => false,
            };

            let ok_ty = ctx.ok_ty.clone();
            let err_ty = ctx.err_ty.clone();

            if !is_result
                && !is_option
                && !resolved_tried_ty.is_partial()
                && !resolved_tried_ty.is_error()
            {
                self.sink
                    .push(diagnostics::infer::try_requires_result_or_option(span));
            }
            if has_mismatch {
                self.sink
                    .push(diagnostics::infer::mixed_carriers_in_try_block(span));
            }

            Some((ok_ty, err_ty))
        } else {
            None
        };

        if let Some((try_ok_ty, try_err_ty)) = try_block_types {
            return self.infer_propagate_in_block(
                store,
                new_expression,
                &resolved_tried_ty,
                &try_ok_ty,
                &try_err_ty,
                span,
                expected_ty,
            );
        }

        self.infer_propagate_in_function(
            store,
            new_expression,
            &resolved_tried_ty,
            span,
            expected_ty,
        )
    }

    fn propagate_as_error(&mut self, store: &Store, expected_ty: &Type, span: Span) -> Type {
        self.unify(store, expected_ty, &Type::Error, &span);
        Type::Error
    }

    #[allow(clippy::too_many_arguments)]
    fn infer_propagate_in_block(
        &mut self,
        store: &mut Store,
        new_expression: Expression,
        tried_ty: &Type,
        try_ok_ty: &Type,
        try_err_ty: &Type,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let ty = if tried_ty.is_error() {
            self.propagate_as_error(store, expected_ty, span)
        } else if tried_ty.is_result() {
            let ok_ty = tried_ty.ok_type();
            self.unify(store, try_err_ty, &tried_ty.err_type(), &span);
            if ok_ty.resolve_in(&self.env).is_variable() {
                self.unify(store, try_ok_ty, &ok_ty, &span);
            }
            self.unify(store, expected_ty, &ok_ty, &span);
            ok_ty
        } else if tried_ty.is_option() {
            let some_ty = tried_ty.ok_type();
            if some_ty.resolve_in(&self.env).is_variable() {
                self.unify(store, try_ok_ty, &some_ty, &span);
            }
            self.unify(store, expected_ty, &some_ty, &span);
            some_ty
        } else {
            self.propagate_as_error(store, expected_ty, span)
        };

        Expression::Propagate {
            expression: new_expression.into(),
            ty,
            span,
        }
    }

    fn infer_propagate_in_function(
        &mut self,
        store: &mut Store,
        new_expression: Expression,
        tried_ty: &Type,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let fn_return_ty = self
            .scopes
            .lookup_fn_return_type()
            .cloned()
            .unwrap_or_else(|| {
                self.sink
                    .push(diagnostics::infer::try_outside_function(span));
                Type::Error
            });

        let ty = if tried_ty.is_error() {
            self.propagate_as_error(store, expected_ty, span)
        } else if tried_ty.is_result() {
            let ok_ty = tried_ty.ok_type();
            let err_ty = tried_ty.err_type();
            let new_ok = self.new_type_var();
            let expected_return = self.type_result(store, new_ok, err_ty);

            if !fn_return_ty.resolve_in(&self.env).is_result() {
                self.sink.push(diagnostics::infer::try_return_type_mismatch(
                    "Result<T, E>",
                    &fn_return_ty.resolve_in(&self.env),
                    span,
                ));
            }

            self.unify(store, &expected_return, &fn_return_ty, &span);
            self.unify(store, expected_ty, &ok_ty, &span);
            ok_ty
        } else if tried_ty.is_option() {
            let some_ty = tried_ty.ok_type();
            let new_some = self.new_type_var();
            let expected_return = self.type_option(store, new_some);

            if !fn_return_ty.resolve_in(&self.env).is_option() {
                self.sink.push(diagnostics::infer::try_return_type_mismatch(
                    "Option<T>",
                    &fn_return_ty.resolve_in(&self.env),
                    span,
                ));
            }

            self.unify(store, &expected_return, &fn_return_ty, &span);
            self.unify(store, expected_ty, &some_ty, &span);
            some_ty
        } else if tried_ty.is_partial() {
            self.propagate_as_error(store, expected_ty, span)
        } else {
            self.sink
                .push(diagnostics::infer::try_requires_result_or_option(span));
            self.propagate_as_error(store, expected_ty, span)
        };

        Expression::Propagate {
            expression: new_expression.into(),
            ty,
            span,
        }
    }

    pub(super) fn infer_try_block(
        &mut self,
        store: &mut Store,
        items: Vec<Expression>,
        try_keyword_span: Span,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        if items.is_empty() {
            self.sink
                .push(diagnostics::infer::try_block_empty(try_keyword_span));
            let unit_ty = self.type_unit();
            let err_ty = self.new_type_var();
            let block_ty = self.type_result(store, unit_ty, err_ty);
            self.unify(store, expected_ty, &block_ty, &span);
            return Expression::TryBlock {
                items: vec![],
                ty: block_ty,
                try_keyword_span,
                span,
            };
        }

        let ok_ty = self.new_type_var();
        let err_ty = self.new_type_var();

        self.scopes.push();
        {
            let scope = self.scopes.current_mut();
            scope.try_block_context = Some(TryBlockContext {
                ok_ty: ok_ty.clone(),
                err_ty: err_ty.clone(),
                carrier: Cell::new(None),
                has_question_mark: Cell::new(false),
                try_span: span,
                loop_depth: DepthCounter::new(),
            });
        }

        self.register_types_and_values(store, &items, &Visibility::Local);

        let new_items = self.infer_block_items(store, items, ok_ty.clone());

        let (has_question_mark, carrier) = {
            let ctx = self
                .scopes
                .current()
                .try_block_context
                .as_ref()
                .expect("try_block_context must exist");
            (ctx.has_question_mark.get(), ctx.carrier.get())
        };

        if !has_question_mark {
            self.sink
                .push(diagnostics::infer::try_block_no_question_mark(
                    try_keyword_span,
                ));
        }

        let last_item = new_items.last().expect("block must have at least one item");

        if let Expression::Propagate {
            expression,
            span: propagate_span,
            ..
        } = last_item
        {
            let is_always_error = match expression.as_ref() {
                Expression::Identifier { .. } => {
                    expression.as_result_constructor() == Some(Err(()))
                        || expression.as_option_constructor() == Some(Err(()))
                }
                Expression::Call {
                    expression: callee, ..
                } => {
                    callee.as_result_constructor() == Some(Err(()))
                        || callee.as_option_constructor() == Some(Err(()))
                }
                _ => false,
            };
            if is_always_error {
                self.facts.add_always_failing_try_block(*propagate_span);
            }
        }

        let inner_ty = last_item.get_type();

        let block_ty = match carrier {
            Some(CarrierKind::Result) => {
                self.unify(store, &ok_ty, &inner_ty, &span);
                self.type_result(store, inner_ty, err_ty)
            }
            Some(CarrierKind::Option) => {
                self.unify(store, &ok_ty, &inner_ty, &span);
                self.type_option(store, inner_ty)
            }
            None => {
                let new_err_ty = self.new_type_var();
                self.type_result(store, inner_ty, new_err_ty)
            }
        };

        self.unify(store, expected_ty, &block_ty, &try_keyword_span);
        self.scopes.pop();

        Expression::TryBlock {
            items: new_items,
            ty: block_ty,
            try_keyword_span,
            span,
        }
    }

    pub(super) fn increment_try_block_loop_depth(&mut self) {
        if let Some(ctx) = self.scopes.lookup_try_block_context() {
            ctx.loop_depth.increment();
        }
    }

    pub(super) fn decrement_try_block_loop_depth(&mut self) {
        if let Some(ctx) = self.scopes.lookup_try_block_context() {
            ctx.loop_depth.decrement();
        }
    }

    pub(super) fn infer_recover_block(
        &mut self,
        store: &mut Store,
        items: Vec<Expression>,
        recover_keyword_span: Span,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let inner_ty = self.new_type_var();

        if items.is_empty() {
            self.sink.push(diagnostics::infer::recover_block_empty(
                recover_keyword_span,
            ));
            let unit_ty = self.type_unit();
            let panic_value_ty = self.type_panic_value(store);
            let block_ty = self.type_result(store, unit_ty, panic_value_ty);
            self.unify(store, expected_ty, &block_ty, &span);
            return Expression::RecoverBlock {
                items: vec![],
                ty: block_ty,
                recover_keyword_span,
                span,
            };
        }

        self.scopes.push();
        {
            let scope = self.scopes.current_mut();
            scope.recover_block_context = Some(RecoverBlockContext {
                inner_ty: inner_ty.clone(),
                recover_span: span,
                loop_depth: DepthCounter::new(),
            });
        }

        self.register_types_and_values(store, &items, &Visibility::Local);

        let new_items = self.infer_block_items(store, items, inner_ty.clone());

        self.scopes.pop();

        let last_item = new_items.last().expect("block must have at least one item");
        let result_inner_ty = last_item.get_type();

        let panic_value_ty = self.type_panic_value(store);
        let block_ty = self.type_result(store, result_inner_ty, panic_value_ty);

        self.unify(store, expected_ty, &block_ty, &recover_keyword_span);

        Expression::RecoverBlock {
            items: new_items,
            ty: block_ty,
            recover_keyword_span,
            span,
        }
    }

    pub(super) fn increment_recover_block_loop_depth(&mut self) {
        if let Some(ctx) = self.scopes.lookup_recover_block_context() {
            ctx.loop_depth.increment();
        }
    }

    pub(super) fn decrement_recover_block_loop_depth(&mut self) {
        if let Some(ctx) = self.scopes.lookup_recover_block_context() {
            ctx.loop_depth.decrement();
        }
    }
}
