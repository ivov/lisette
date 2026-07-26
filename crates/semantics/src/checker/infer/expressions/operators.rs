use crate::checker::EnvResolve;
use crate::store::Store;
use syntax::ast::{BinaryOperator, Expression, Literal, Span, UnaryOperator};
use syntax::types::{SimpleKind, Type};

use BinaryOperator::*;
use UnaryOperator::*;

use crate::checker::infer::InferCtx;

pub(super) struct InferredOperand {
    pub(super) expression: Expression,
    pub(super) ty: Type,
}

impl InferredOperand {
    pub(super) fn new(expression: Expression, ty: Type) -> Self {
        Self { expression, ty }
    }
}

struct BinaryOperand<'a> {
    ty: &'a Type,
    span: Span,
}

struct BinaryOperands<'a> {
    left: BinaryOperand<'a>,
    right: BinaryOperand<'a>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumericCompat {
    SameAliased,
    DifferentCompatible,
    Neither,
}

impl NumericCompat {
    fn is_compatible(self) -> bool {
        !matches!(self, NumericCompat::Neither)
    }
}

impl InferCtx<'_> {
    pub(super) fn infer_unary(
        &mut self,
        operator: UnaryOperator,
        operand: Box<Expression>,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        let propagate_to_operand = {
            let resolved = expected_ty.resolve_in(&self.env);
            match operator {
                Negative => {
                    resolved.is_numeric() || self.store.has_underlying_numeric_type(&resolved)
                }
                Not => self.store.underlying_simple_kind(&resolved) == Some(SimpleKind::Bool),
                _ => false,
            }
        };
        let operand_expected_ty = if propagate_to_operand {
            expected_ty.clone()
        } else {
            self.new_type_var()
        };

        let new_expression = if operator == Negative {
            self.in_negation(|this| {
                this.with_value_context(|this| {
                    this.infer_expression(*operand, &operand_expected_ty)
                })
            })
        } else {
            self.with_value_context(|this| this.infer_expression(*operand, &operand_expected_ty))
        };
        let operand_span = new_expression.get_span();

        let expression_ty = match operator {
            Negative => {
                let resolved = operand_expected_ty.resolve_in(&self.env);
                if resolved.is_numeric() || self.store.underlying_numeric_type(&resolved).is_some()
                {
                    let is_literal = is_numeric_literal(&new_expression);
                    if resolved.is_unsigned_int() && !is_literal {
                        let type_name = resolved.get_name().unwrap_or_default();
                        self.sink
                            .push(diagnostics::infer::cannot_negate_unsigned(type_name, span));
                    }
                    self.check_negative_literal_overflow(&new_expression, &resolved, span);
                    operand_expected_ty.clone()
                } else {
                    if !resolved.is_error() {
                        self.sink
                            .push(diagnostics::infer::not_numeric(&resolved, operand_span));
                    }
                    operand_expected_ty.clone()
                }
            }
            Not => {
                let resolved = operand_expected_ty.resolve_in(&self.env);
                if !resolved.is_boolean()
                    && self.store.underlying_simple_kind(&resolved) == Some(SimpleKind::Bool)
                {
                    operand_expected_ty.clone()
                } else {
                    let bool_ty = self.type_bool();
                    self.unify(&bool_ty, &operand_expected_ty, &span);
                    bool_ty
                }
            }
            BitwiseNot => {
                let resolved = operand_expected_ty.resolve_in(&self.env);
                if resolved.is_error()
                    || matches!(resolved, Type::Var { .. })
                    || is_integer_type(&resolved, &self.env, self.store)
                {
                    operand_expected_ty.clone()
                } else {
                    self.sink
                        .push(diagnostics::infer::not_integer(&resolved, operand_span));
                    operand_expected_ty.clone()
                }
            }
            Deref => {
                let inner_ty = self.new_type_var();
                let ref_ty = self.type_reference(inner_ty.clone());
                self.unify(&ref_ty, &operand_expected_ty, &span);
                inner_ty
            }
        };

        self.unify(expected_ty, &expression_ty, &span);

        Expression::Unary {
            operator,
            expression: new_expression.into(),
            ty: expression_ty,
            span,
        }
    }

    pub(super) fn infer_binary(
        &mut self,
        operator: BinaryOperator,
        left_operand: Box<Expression>,
        right_operand: Box<Expression>,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        if matches!(*left_operand, Expression::Binary { .. }) {
            let mut stack = vec![(operator, right_operand, span)];
            let mut current = *left_operand;
            while let Expression::Binary {
                operator: op,
                left,
                right,
                span: s,
                ..
            } = current
            {
                stack.push((op, right, s));
                current = *left;
            }
            let left_ty = self.new_type_var();
            let left_inferred = self.infer_expression(current, &left_ty);
            let mut left = InferredOperand::new(left_inferred, left_ty);
            while let Some((op, right, s)) = stack.pop() {
                let result_ty = if stack.is_empty() {
                    expected_ty.clone()
                } else {
                    self.new_type_var()
                };
                left = self.infer_binary_with_left(op, left, right, &result_ty, s);
            }
            return left.expression;
        }

        self.infer_binary_impl(operator, left_operand, right_operand, expected_ty, span)
    }

    /// Infer a binary expression where the left operand is already inferred.
    /// Returns the inferred expression and its result type.
    pub(super) fn infer_binary_with_left(
        &mut self,
        operator: BinaryOperator,
        left: InferredOperand,
        right_operand: Box<Expression>,
        expected_ty: &Type,
        span: Span,
    ) -> InferredOperand {
        let right_operand_ty = self.new_type_var();

        let right_literal_kind = literal_kind(&right_operand);
        let is_right_literal = !matches!(right_literal_kind, LiteralKind::None);

        let new_right_operand = self.with_value_context(|s| {
            if is_right_literal {
                let left_resolved = left.ty.resolve_in(&s.env);
                if literal_can_adapt_to(&right_literal_kind, &left_resolved, self.store) {
                    let _ = s.try_unify(&right_operand_ty, &left_resolved, &span);
                }
            }
            s.infer_expression(*right_operand, &right_operand_ty)
        });

        let right = InferredOperand::new(new_right_operand, right_operand_ty);
        self.finish_binary(operator, left, right, expected_ty, span)
    }

    fn infer_binary_impl(
        &mut self,
        operator: BinaryOperator,
        left_operand: Box<Expression>,
        right_operand: Box<Expression>,
        expected_ty: &Type,
        span: Span,
    ) -> Expression {
        let left_operand_ty = self.new_type_var();
        let right_operand_ty = self.new_type_var();

        // Check for numeric literals before inference so we can propagate
        // type information from the non-literal operand to the literal.
        // This enables coercion like `b == 0` where b: float64 → 0 becomes float64.
        //
        // Integer literals adapt when the target `is_numeric()` (int, float, etc.).
        // Float literals adapt only when the target `is_float()` (float32, float64).
        let left_literal_kind = literal_kind(&left_operand);
        let right_literal_kind = literal_kind(&right_operand);
        let is_left_literal = !matches!(left_literal_kind, LiteralKind::None);
        let is_right_literal = !matches!(right_literal_kind, LiteralKind::None);

        let (new_left_operand, new_right_operand) = self.with_value_context(|s| {
            if is_left_literal && !is_right_literal {
                // Infer the non-literal (right) first so its resolved type
                // can guide the literal's type adaptation.
                let right = s.infer_expression(*right_operand, &right_operand_ty);
                let right_resolved = right_operand_ty.resolve_in(&s.env);
                if literal_can_adapt_to(&left_literal_kind, &right_resolved, self.store) {
                    let _ = s.try_unify(&left_operand_ty, &right_resolved, &span);
                }
                let left = s.infer_expression(*left_operand, &left_operand_ty);
                (left, right)
            } else {
                let left = s.infer_expression(*left_operand, &left_operand_ty);
                if is_right_literal {
                    let left_resolved = left_operand_ty.resolve_in(&s.env);
                    if literal_can_adapt_to(&right_literal_kind, &left_resolved, self.store) {
                        let _ = s.try_unify(&right_operand_ty, &left_resolved, &span);
                    }
                }
                let right = s.infer_expression(*right_operand, &right_operand_ty);
                (left, right)
            }
        });

        let left = InferredOperand::new(new_left_operand, left_operand_ty);
        let right = InferredOperand::new(new_right_operand, right_operand_ty);
        self.finish_binary(operator, left, right, expected_ty, span)
            .expression
    }

    fn finish_binary(
        &mut self,
        operator: BinaryOperator,
        left: InferredOperand,
        right: InferredOperand,
        expected_ty: &Type,
        span: Span,
    ) -> InferredOperand {
        if matches!(operator, Division | Remainder) && is_zero_literal(&right.expression) {
            self.report_zero_divisor(operator, &right.ty, span);
        }
        if matches!(operator, And | Or)
            && let Some(span) = Self::find_propagate(&right.expression)
        {
            self.sink
                .push(diagnostics::infer::propagate_in_condition(span));
        }

        let store = self.store;
        let (expression_ty, reported) = self.tracking_diagnostics(|this| {
            InferCtx::new(this, store).resolve_binary_type(
                &operator,
                BinaryOperands {
                    left: BinaryOperand {
                        ty: &left.ty,
                        span: left.expression.get_span(),
                    },
                    right: BinaryOperand {
                        ty: &right.ty,
                        span: right.expression.get_span(),
                    },
                },
                span,
            )
        });
        if reported {
            self.facts.type_error_spans.insert(span);
        }

        self.unify(expected_ty, &expression_ty, &span);

        let expression = Expression::Binary {
            operator,
            left: left.expression.into(),
            right: right.expression.into(),
            ty: expression_ty.clone(),
            span,
        };
        InferredOperand::new(expression, expression_ty)
    }

    /// Resolve the result type of a binary operation given already-inferred operand types.
    fn resolve_binary_type(
        &mut self,
        operator: &BinaryOperator,
        operands: BinaryOperands<'_>,
        span: Span,
    ) -> Type {
        let left_operand_ty = operands.left.ty;
        let right_operand_ty = operands.right.ty;
        let left_span = &operands.left.span;
        let right_span = &operands.right.span;
        let resolved_left_operand = left_operand_ty.resolve_in(&self.env);
        let resolved_right_operand = right_operand_ty.resolve_in(&self.env);
        match operator {
            Equal | NotEqual => {
                if !self.report_named_type_boundary(
                    &resolved_left_operand,
                    &resolved_right_operand,
                    span,
                ) && !self
                    .numeric_compat(&resolved_left_operand, &resolved_right_operand)
                    .is_compatible()
                {
                    self.unify_binary_operands(operator, left_operand_ty, right_operand_ty, &span);
                }
                let operands_match =
                    left_operand_ty.resolve_in(&self.env) == right_operand_ty.resolve_in(&self.env);
                if self.ensure_comparable(left_operand_ty, left_span, operands_match) {
                    self.ensure_comparable(right_operand_ty, right_span, operands_match);
                }
                self.type_bool()
            }

            And | Or => {
                if self.report_named_type_boundary(
                    &resolved_left_operand,
                    &resolved_right_operand,
                    span,
                ) {
                    left_operand_ty.clone()
                } else if self.store.underlying_simple_kind(&resolved_left_operand)
                    == Some(SimpleKind::Bool)
                    || self.store.underlying_simple_kind(&resolved_right_operand)
                        == Some(SimpleKind::Bool)
                {
                    self.unify_binary_operands(operator, left_operand_ty, right_operand_ty, &span);
                    left_operand_ty.clone()
                } else {
                    let bool_ty = self.type_bool();
                    self.unify(left_operand_ty, &bool_ty, &span);
                    self.unify(right_operand_ty, &bool_ty, &span);
                    bool_ty
                }
            }

            LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual => {
                if self.report_named_type_boundary(
                    &resolved_left_operand,
                    &resolved_right_operand,
                    span,
                ) {
                    return self.type_bool();
                }

                if self
                    .numeric_compat(&resolved_left_operand, &resolved_right_operand)
                    .is_compatible()
                    && self.store.is_orderable(&resolved_left_operand)
                    && self.store.is_orderable(&resolved_right_operand)
                {
                    self.type_bool()
                } else {
                    self.ensure_orderable(left_operand_ty, left_span);
                    self.ensure_orderable(right_operand_ty, right_span);
                    self.unify_binary_operands(operator, left_operand_ty, right_operand_ty, &span);
                    self.type_bool()
                }
            }

            Addition => {
                if let Some(result_ty) = self.try_operation_with_numeric_alias(
                    operator,
                    &resolved_left_operand,
                    &resolved_right_operand,
                    &span,
                ) {
                    result_ty
                } else if self.report_named_type_boundary(
                    &resolved_left_operand,
                    &resolved_right_operand,
                    span,
                ) {
                    left_operand_ty.clone()
                } else {
                    let is_string_like = |ty: &Type| {
                        self.store.underlying_simple_kind(ty) == Some(SimpleKind::String)
                    };
                    let numeric_ok = if !is_string_like(&resolved_left_operand)
                        && !is_string_like(&resolved_right_operand)
                    {
                        self.ensure_numeric_for_binary(operator, left_operand_ty, left_span)
                            & self.ensure_numeric_for_binary(operator, right_operand_ty, right_span)
                    } else {
                        true
                    };

                    if resolved_left_operand.is_complex() || resolved_right_operand.is_complex() {
                        self.type_complex128()
                    } else {
                        if numeric_ok {
                            self.unify_binary_operands(
                                operator,
                                left_operand_ty,
                                right_operand_ty,
                                &span,
                            );
                        }
                        left_operand_ty.clone()
                    }
                }
            }

            Subtraction | Multiplication | Division | Remainder => {
                if matches!(operator, Remainder)
                    && (resolved_left_operand.is_float() || resolved_right_operand.is_float())
                {
                    self.sink
                        .push(diagnostics::infer::float_modulo_not_supported(span));
                }

                if let Some(result_ty) = self.try_operation_with_numeric_alias(
                    operator,
                    &resolved_left_operand,
                    &resolved_right_operand,
                    &span,
                ) {
                    result_ty
                } else if resolved_left_operand.is_complex() || resolved_right_operand.is_complex()
                {
                    self.type_complex128()
                } else {
                    let left_ok =
                        self.ensure_numeric_for_binary(operator, left_operand_ty, left_span);
                    let right_ok =
                        self.ensure_numeric_for_binary(operator, right_operand_ty, right_span);
                    if left_ok && right_ok {
                        self.unify_binary_operands(
                            operator,
                            left_operand_ty,
                            right_operand_ty,
                            &span,
                        );
                    }
                    left_operand_ty.clone()
                }
            }

            BitwiseAnd | BitwiseOr | BitwiseXor | BitwiseAndNot => {
                if let Some(result_ty) = self.try_operation_with_numeric_alias(
                    operator,
                    &resolved_left_operand,
                    &resolved_right_operand,
                    &span,
                ) {
                    result_ty
                } else {
                    let left_ok =
                        self.ensure_integer_for_binary(operator, left_operand_ty, left_span);
                    let right_ok =
                        self.ensure_integer_for_binary(operator, right_operand_ty, right_span);
                    if left_ok && right_ok {
                        self.unify_binary_operands(
                            operator,
                            left_operand_ty,
                            right_operand_ty,
                            &span,
                        );
                    }
                    left_operand_ty.clone()
                }
            }

            ShiftLeft | ShiftRight => {
                self.ensure_integer_for_binary(operator, left_operand_ty, left_span);
                self.ensure_integer_for_binary(operator, right_operand_ty, right_span);
                left_operand_ty.clone()
            }

            Pipeline => {
                panic!("Pipeline operator should have been desugared before type inference")
            }
        }
    }

    /// Classifies whether two resolved operand types are numerically compatible.
    fn numeric_compat(&self, left: &Type, right: &Type) -> NumericCompat {
        if left == right && self.store.is_aliased_numeric_type(left) {
            NumericCompat::SameAliased
        } else if left != right && self.store.is_numeric_compatible_with(left, right) {
            NumericCompat::DifferentCompatible
        } else {
            NumericCompat::Neither
        }
    }

    /// Returns `true` if the type is numeric (or unresolved), `false` if an error was emitted.
    fn ensure_numeric_for_binary(
        &mut self,
        operator: &BinaryOperator,
        ty: &Type,
        span: &Span,
    ) -> bool {
        let resolved_ty = self.env.resolve(ty);
        // Type variables (unresolved inference vars) are allowed, they'll be resolved later.
        // But type parameters (generic T without bounds) should be rejected:
        // Go requires `constraints.Ordered` for arithmetic on type params.
        if matches!(resolved_ty, Type::Var { .. } | Type::Error) {
            return true;
        }
        if matches!(resolved_ty, Type::Parameter(_)) {
            self.sink
                .push(diagnostics::infer::not_orderable(&resolved_ty, *span));
            return false;
        }
        if !resolved_ty.is_numeric() {
            self.sink.push(diagnostics::infer::not_numeric_for_binary(
                operator,
                &resolved_ty,
                *span,
            ));
            return false;
        }
        true
    }

    fn ensure_integer_for_binary(
        &mut self,
        operator: &BinaryOperator,
        ty: &Type,
        span: &Span,
    ) -> bool {
        let resolved_ty = self.env.resolve(ty);
        if matches!(resolved_ty, Type::Var { .. } | Type::Error) {
            return true;
        }
        if !is_integer_type(&resolved_ty, &self.env, self.store) {
            self.sink.push(diagnostics::infer::not_integer_for_binary(
                operator,
                &resolved_ty,
                *span,
            ));
            return false;
        }
        true
    }

    fn ensure_orderable(&mut self, ty: &Type, span: &Span) {
        let resolved_ty = ty.resolve_in(&self.env);

        if resolved_ty.is_error() {
            return;
        }

        if let Type::Parameter(name) = &resolved_ty {
            if !self.parameter_satisfies_bound(name, super::super::unify::BuiltinBound::Ordered) {
                self.sink
                    .push(diagnostics::infer::param_needs_ordered_bound(name, *span));
            }
            return;
        }

        if !self.store.is_orderable(&resolved_ty) {
            self.sink
                .push(diagnostics::infer::not_orderable(&resolved_ty, *span));
        }
    }

    fn unify_binary_operands(
        &mut self,
        operator: &BinaryOperator,
        left_operand_ty: &Type,
        right_operand_ty: &Type,
        span: &Span,
    ) {
        let store = self.store;
        let (unification, reported) = self.tracking_diagnostics(|this| {
            InferCtx::new(this, store).try_unify(left_operand_ty, right_operand_ty, span)
        });
        if unification.is_err() {
            if !reported
                && self
                    .try_unify(right_operand_ty, left_operand_ty, span)
                    .is_ok()
            {
                return;
            }

            let left_resolved = left_operand_ty.resolve_in(&self.env);
            let right_resolved = right_operand_ty.resolve_in(&self.env);
            self.sink
                .push(diagnostics::infer::binary_operator_type_mismatch(
                    operator,
                    &left_resolved,
                    &right_resolved,
                    *span,
                ));
        }
    }

    fn report_named_type_boundary(&mut self, left: &Type, right: &Type, span: Span) -> bool {
        let store = self.store;
        if left == right || store.deep_resolve_alias(left) == store.deep_resolve_alias(right) {
            return false;
        }
        let left_is_defined_type = is_nominal_defined_type(left, store);
        let right_is_defined_type = is_nominal_defined_type(right, store);

        if left_is_defined_type && right_is_defined_type {
            let underlying = store
                .underlying_simple_kind(left)
                .map(Type::Simple)
                .unwrap_or_else(|| left.clone());
            self.sink.push(diagnostics::infer::incompatible_named_types(
                &underlying,
                span,
            ));
            true
        } else if left_is_defined_type && plain_primitive_of_backing(left, right, store) {
            self.sink
                .push(diagnostics::infer::named_primitive_needs_cast(
                    right, left, span,
                ));
            true
        } else if right_is_defined_type && plain_primitive_of_backing(right, left, store) {
            self.sink
                .push(diagnostics::infer::named_primitive_needs_cast(
                    left, right, span,
                ));
            true
        } else {
            false
        }
    }

    fn try_operation_with_numeric_alias(
        &mut self,
        operator: &BinaryOperator,
        left_ty: &Type,
        right_ty: &Type,
        span: &Span,
    ) -> Option<Type> {
        let store = self.store;
        let left_underlying = store.underlying_numeric_type(left_ty);
        let right_underlying = store.underlying_numeric_type(right_ty);

        let (left_underlying, right_underlying) = match (left_underlying, right_underlying) {
            (Some(l), Some(r)) => (l, r),
            _ => return None,
        };

        let left_family = left_underlying.numeric_family()?;
        let right_family = right_underlying.numeric_family()?;

        if left_family != right_family {
            return None;
        }

        let left_is_aliased = store.is_aliased_numeric_type(left_ty);
        let right_is_aliased = store.is_aliased_numeric_type(right_ty);

        match (left_is_aliased, right_is_aliased) {
            (false, false) => None,

            (true, true)
                if left_ty == right_ty
                    || store.deep_resolve_alias(left_ty) == store.deep_resolve_alias(right_ty) =>
            {
                Some(left_ty.clone())
            }

            (true, true) => {
                self.sink.push(diagnostics::infer::incompatible_named_types(
                    &left_underlying,
                    *span,
                ));
                Some(left_ty.clone())
            }

            (true, false) => {
                if is_nominal_defined_type(left_ty, store) {
                    self.sink
                        .push(diagnostics::infer::named_primitive_needs_cast(
                            right_ty, left_ty, *span,
                        ));
                }
                Some(left_ty.clone())
            }
            (false, true) => {
                if is_nominal_defined_type(right_ty, store) {
                    self.sink
                        .push(diagnostics::infer::named_primitive_needs_cast(
                            left_ty, right_ty, *span,
                        ));
                } else if matches!(operator, Division | Remainder) {
                    self.sink.push(diagnostics::infer::invalid_division_order(
                        operator, left_ty, right_ty, *span,
                    ));
                    return None;
                }
                Some(right_ty.clone())
            }
        }
    }

    pub(super) fn infer_range(
        &mut self,
        start: Option<Box<Expression>>,
        end: Option<Box<Expression>>,
        inclusive: bool,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let element_ty = self.new_type_var();

        let (new_start, new_end) = self.with_value_context(|s| {
            let start =
                start.map(|expression| Box::new(s.infer_expression(*expression, &element_ty)));
            let end = end.map(|expression| Box::new(s.infer_expression(*expression, &element_ty)));
            (start, end)
        });

        let range_ty = match (&new_start, &new_end, inclusive) {
            (Some(_), Some(_), false) => self.type_range(store, element_ty.clone()),
            (Some(_), Some(_), true) => self.type_range_inclusive(store, element_ty.clone()),
            (Some(_), None, _) => self.type_range_from(store, element_ty.clone()),
            (None, Some(_), false) => self.type_range_to(store, element_ty.clone()),
            (None, Some(_), true) => self.type_range_to_inclusive(store, element_ty.clone()),
            (None, None, _) => {
                self.sink
                    .push(diagnostics::infer::range_full_not_valid_expression(span));
                let error_ty = self.new_type_var();
                self.type_range(store, error_ty)
            }
        };

        self.unify(expected_ty, &range_ty, &span);

        Expression::Range {
            start: new_start,
            end: new_end,
            inclusive,
            ty: range_ty,
            span,
        }
    }

    pub(super) fn infer_cast(
        &mut self,
        expression: Box<Expression>,
        target_type: syntax::ast::Annotation,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let target_ty = self.convert_to_type(store, &target_type, &span);

        let source_ty_var = self.new_type_var();
        let new_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &source_ty_var));
        let source_ty = source_ty_var.resolve_in(&self.env);

        if is_cast_expression(&new_expression) {
            self.sink.push(diagnostics::infer::chained_cast(span));
        }

        if !self.check_redundant_cast(&source_ty, &target_ty, span) {
            self.check_redundant_literal_cast(&new_expression, &target_ty, expected_ty, span);
        }

        self.check_cast_literal_overflow(&new_expression, &target_ty, span);

        self.check_valid_cast(&source_ty, &target_ty, span);

        if is_float_literal(&new_expression) && is_integer_type(&target_ty, &self.env, self.store) {
            self.sink
                .push(diagnostics::infer::float_literal_int_cast(span));
        }

        let result_ty = if source_ty.contains_error() || target_ty.contains_error() {
            Type::Error
        } else {
            target_ty.clone()
        };

        self.unify(expected_ty, &result_ty, &span);

        Expression::Cast {
            expression: new_expression.into(),
            target_type,
            ty: result_ty,
            span,
        }
    }

    fn report_zero_divisor(&mut self, operator: BinaryOperator, divisor_ty: &Type, span: Span) {
        let resolved = divisor_ty.resolve_in(&self.env);
        let ieee = resolved.is_float() || resolved.is_complex();
        if ieee && matches!(operator, Remainder) {
            return;
        }
        self.sink
            .push(diagnostics::infer::division_by_zero(span, ieee));
    }
}

fn is_zero_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Integer { value: 0, .. },
            ..
        } => true,
        Expression::Literal {
            literal: Literal::Float { value, .. },
            ..
        } => *value == 0.0,
        _ => false,
    }
}

fn is_float_literal(expression: &Expression) -> bool {
    match expression.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Float { .. },
            ..
        } => true,
        Expression::Unary {
            operator: Negative,
            expression,
            ..
        } => is_float_literal(expression),
        _ => false,
    }
}

fn is_integer_type(ty: &Type, env: &crate::checker::TypeEnv, store: &Store) -> bool {
    let resolved = ty.resolve_in(env);
    let direct_match = matches!(
        resolved.get_name(),
        Some(
            "int"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "uint"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uintptr"
                | "byte"
                | "rune"
        )
    );

    if direct_match {
        return true;
    }

    store
        .underlying_numeric_type(&resolved)
        .is_some_and(|underlying| {
            matches!(
                underlying.get_name(),
                Some(
                    "int"
                        | "int8"
                        | "int16"
                        | "int32"
                        | "int64"
                        | "uint"
                        | "uint8"
                        | "uint16"
                        | "uint32"
                        | "uint64"
                        | "uintptr"
                        | "byte"
                        | "rune"
                )
            )
        })
}

fn is_cast_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Cast { .. } => true,
        Expression::Paren { expression, .. } => is_cast_expression(expression),
        _ => false,
    }
}

fn is_numeric_literal(expression: &Expression) -> bool {
    match expression {
        Expression::Literal {
            literal: Literal::Integer { .. } | Literal::Float { .. },
            ..
        } => true,
        Expression::Paren { expression, .. } => is_numeric_literal(expression),
        _ => false,
    }
}

/// Literal kinds for type adaptation purposes.
enum LiteralKind {
    Integer,
    Float,
    String,
    Boolean,
    None,
}

fn literal_kind(expression: &Expression) -> LiteralKind {
    match expression {
        Expression::Literal {
            literal: Literal::Integer { .. },
            ..
        } => LiteralKind::Integer,
        Expression::Literal {
            literal: Literal::Float { .. },
            ..
        } => LiteralKind::Float,
        Expression::Literal {
            literal: Literal::String { .. },
            ..
        } => LiteralKind::String,
        Expression::Literal {
            literal: Literal::Boolean(_),
            ..
        } => LiteralKind::Boolean,
        Expression::Paren { expression, .. } => literal_kind(expression),
        Expression::Unary {
            operator: Negative | BitwiseNot | Not,
            expression,
            ..
        } => literal_kind(expression),
        _ => LiteralKind::None,
    }
}

fn literal_can_adapt_to(kind: &LiteralKind, target: &Type, store: &Store) -> bool {
    let named_backed_by = |k: SimpleKind| {
        matches!(target, Type::Nominal { .. }) && store.underlying_simple_kind(target) == Some(k)
    };
    match kind {
        LiteralKind::Integer => store.literal_adaptation_target(target).is_some(),
        LiteralKind::Float => store
            .literal_adaptation_target(target)
            .is_some_and(|underlying| underlying.is_float()),
        LiteralKind::String => named_backed_by(SimpleKind::String),
        LiteralKind::Boolean => named_backed_by(SimpleKind::Bool),
        LiteralKind::None => false,
    }
}

fn is_nominal_defined_type(ty: &Type, store: &Store) -> bool {
    matches!(store.deep_resolve_alias(ty), Type::Nominal { id, .. } if store.is_nominal_defined_type(id.as_str()))
}

fn is_plain_numeric_primitive(ty: &Type, store: &Store) -> bool {
    ty.is_numeric() && !store.is_aliased_numeric_type(ty)
}

fn plain_primitive_of_backing(defined_ty: &Type, other: &Type, store: &Store) -> bool {
    match store.underlying_simple_kind(defined_ty) {
        Some(SimpleKind::String) => other.is_string(),
        Some(SimpleKind::Bool) => other.is_boolean(),
        _ => is_plain_numeric_primitive(other, store),
    }
}
