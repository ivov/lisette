use crate::abi::is_tagged_shape_fn_value;
use crate::expressions::access::struct_call::emit_struct_literal;
use syntax::program::DefinitionBody;

use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::abi::transition::emit_lisette_callback_wrapper;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::is_order_sensitive;
use crate::plan::bodies::{
    ExpressionStatementForm, ExpressionStatementPlan, LoweredBlock, LoweredStatement,
};
use crate::plan::calls::CallableOrigin;
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::state::bindings::BindingValue;
use syntax::ast::Expression;
use syntax::program::CallKind;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn lower_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if let Some(callee) = self.resolve_callable_value(expression)
            && matches!(callee.origin, CallableOrigin::GoInterop)
        {
            let abi = &callee.abi.result;
            if !self.go_fn_matches_lowered_slot(expression, abi, ctx) {
                let mut setup = Vec::new();
                let value = if self.go_fn_needs_lowered_tuple_adapter(expression, abi, ctx) {
                    self.emit_go_fn_lowered_tuple_adapter(&mut setup, expression)
                } else if let CallableReturnAbi::Option {
                    encoding: OptionReturnAbi::Sentinel(value),
                    ..
                } = abi
                    && !ctx.forces_tagged_go_function()
                {
                    self.emit_go_fn_sentinel_adapter(&mut setup, expression, *value)
                } else {
                    self.emit_go_fn_wrapper(&mut setup, expression, abi)
                };
                return value_plan_from_statements(setup, value);
            }
        }
        if self.is_go_array_return_value(expression) {
            let mut setup = Vec::new();
            let value = self.emit_array_return_wrapper(&mut setup, expression);
            return value_plan_from_statements(setup, value);
        }

        self.plan_operand(expression, ctx)
    }

    pub(crate) fn lower_composite_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        if expression.get_type().is_unit()
            && matches!(
                expression.unwrap_parens(),
                Expression::Call { .. } | Expression::Block { .. }
            )
        {
            let (mut setup, call_str) = self.lower_value(expression, ctx).into_parts();
            if !call_str.is_empty() {
                setup.push(LoweredStatement::RawGo(format!("{call_str}\n")));
            }
            return value_plan_from_statements(setup, "struct{}{}".to_string());
        }
        self.lower_value(expression, ctx)
    }

    /// Wrap a captured tagged-shape prelude fn ref into a lowered-ABI closure
    /// so its Go type matches what the rest of the pipeline expects.
    fn maybe_lower_tagged_fn_ref(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        ty: &Type,
        raw: String,
        ctx: ExpressionContext<'_>,
    ) -> String {
        if ctx.is_callee() || ctx.forces_tagged_go_function() {
            return raw;
        }
        if !is_tagged_shape_fn_value(expression) {
            return raw;
        }
        let fn_ty = ty.unwrap_forall();
        let Type::Function(f) = fn_ty else {
            return raw;
        };
        if self.classify_direct_emission(&f.return_type).is_none() {
            return raw;
        }
        emit_lisette_callback_wrapper(self, setup, &raw, fn_ty)
    }

    /// True when a Go function value's natural ABI matches the slot's
    /// lowered shape — wrapping would be identity.
    fn go_fn_matches_lowered_slot(
        &self,
        expression: &Expression,
        source: &CallableReturnAbi,
        ctx: ExpressionContext<'_>,
    ) -> bool {
        if ctx.forces_tagged_go_function() {
            return false;
        }
        let fn_ty = expression.get_type();
        let Some(f) = fn_ty.as_function_type() else {
            return false;
        };
        let target = self
            .classify_direct_emission(&f.return_type)
            .unwrap_or_else(|| self.value_return_abi(&f.return_type));
        source == &target
    }

    /// True when a tuple-ok fallible Go function value must be wrapped to match a lowered slot.
    fn go_fn_needs_lowered_tuple_adapter(
        &self,
        expression: &Expression,
        source: &CallableReturnAbi,
        ctx: ExpressionContext<'_>,
    ) -> bool {
        if ctx.forces_tagged_go_function() {
            return false;
        }
        let fn_ty = expression.get_type();
        let Some(f) = fn_ty.as_function_type() else {
            return false;
        };
        let target = self
            .classify_direct_emission(&f.return_type)
            .unwrap_or_else(|| self.value_return_abi(&f.return_type));
        matches!(
            (source, target),
            (
                CallableReturnAbi::Result {
                    payload: PayloadLayout::Flattened,
                    ..
                } | CallableReturnAbi::Partial {
                    payload: PayloadLayout::Flattened,
                },
                CallableReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                    ..
                } | CallableReturnAbi::Partial {
                    payload: PayloadLayout::Packed,
                }
            )
        )
    }

    /// Plan a value-position leaf expression (one `plan_operand` does not lower
    /// structurally) into a `ValuePlan`.
    pub(crate) fn plan_operand_leaf(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        match expression {
            Expression::Literal { literal, ty, .. } => self.emit_literal(literal, ty),
            Expression::Identifier {
                value,
                ty,
                qualified,
                ..
            } => {
                let mut setup: Vec<LoweredStatement> = Vec::new();
                let raw = self.emit_identifier(value, qualified.as_deref(), ty, ctx);
                let value = self.maybe_lower_tagged_fn_ref(&mut setup, expression, ty, raw, ctx);
                value_plan_from_statements(setup, value)
            }
            Expression::Call { ty, .. } => {
                let plan = self
                    .plan_call(expression)
                    .expect("plan_call yields Some for a Call expression");
                match plan.result_transition {
                    AbiTransition::Identity => {
                        let (setup, value) = self.lower_call(expression, Some(ty), ctx);
                        value_plan_from_statements(setup, value)
                    }
                    AbiTransition::WrapToTagged => {
                        self.lower_abi_wrapped_call(expression, &plan.resolved.abi.result, ty)
                    }
                    AbiTransition::LowerFromTagged
                    | AbiTransition::Reencode
                    | AbiTransition::Incompatible => {
                        unreachable!("call results target their Lisette value representation")
                    }
                }
            }
            Expression::RawGo { text } => ValuePlan::Operand(text.clone()),
            Expression::Unit { .. } => ValuePlan::Operand("struct{}{}".to_string()),
            Expression::NoOp => ValuePlan::Operand(String::new()),
            Expression::Lambda {
                params, body, ty, ..
            }
            | Expression::Function {
                params, body, ty, ..
            } => ValuePlan::Operand(self.emit_lambda(params, body, ty, ctx)),
            Expression::IfLet { ty, .. }
            | Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
            | Expression::Block { ty, .. } => self.lower_to_operand_temp(expression, ty),
            Expression::Return {
                expression: return_expression,
                ..
            } => {
                let plan = self.build_return_plan(return_expression);
                value_plan_from_statements(vec![LoweredStatement::Return(plan)], String::new())
            }
            Expression::Assignment { target, value, .. } => {
                let setup = self.lower_assignment_operand(target, value);
                value_plan_from_statements(setup, "struct{}{}".to_string())
            }
            Expression::Assert { .. } => value_plan_from_statements(
                vec![self.lower_assert_statement(expression)],
                "struct{}{}".to_string(),
            ),
            _ => unreachable!(
                "unexpected leaf expression in plan_operand: {:?}",
                expression
            ),
        }
    }

    /// Emit a Go tuple literal. `in_tail` widens slot types to the declared
    /// return slots so per-element coercion matches the return site.
    /// Plan a tuple literal as `lisette.MakeTupleN(...)`.
    pub(crate) fn plan_tuple_value(
        &mut self,
        elements: &[Expression],
        ty: &Type,
        in_tail: bool,
    ) -> ValuePlan {
        use value_plan_from_statements;

        let inferred_slot_types: Vec<Type> = match ty {
            Type::Tuple(slots) => slots.clone(),
            _ => Vec::new(),
        };
        let slot_types = self.resolve_tuple_slot_types(inferred_slot_types, in_tail);

        let stages: Vec<StagedExpression> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let element_ctx =
                    ExpressionContext::value().with_expected_slot_type(slot_types.get(i));
                self.stage_composite(e, element_ctx)
            })
            .collect();
        let (mut setup, element_expressions) = self.sequence_structured(stages, "_v");

        let mut wrapped_expressions: Vec<String> = Vec::with_capacity(element_expressions.len());
        for (i, (expr, emitted)) in elements.iter().zip(element_expressions).enumerate() {
            let value = match slot_types.get(i) {
                Some(slot) => {
                    let coercion = Coercion::resolve(
                        self,
                        &expr.get_type(),
                        slot,
                        CoercionDirection::Internal,
                    );
                    let (coercion_setup, coerced) = coercion.lower(self, emitted);
                    setup.extend(coercion_setup);
                    coerced
                }
                None => emitted,
            };
            wrapped_expressions.push(value);
        }
        let element_expressions = wrapped_expressions;

        self.require_stdlib();
        let arity = element_expressions.len();

        let needs_explicit_type_args =
            !slot_types.is_empty() && slot_types.iter().any(|t| self.facts.is_interface(t));

        let value = if !needs_explicit_type_args {
            format!(
                "lisette.MakeTuple{}({})",
                arity,
                element_expressions.join(", ")
            )
        } else {
            let slot_ty_strs: Vec<String> =
                slot_types.iter().map(|t| self.go_type_string(t)).collect();
            format!(
                "lisette.MakeTuple{}[{}]({})",
                arity,
                slot_ty_strs.join(", "),
                element_expressions.join(", ")
            )
        };
        value_plan_from_statements(setup, value)
    }

    /// Plan a `cast` expression. The interface-target path resolves through
    /// a coercion (may emit setup); the primitive/named path becomes a
    /// structured `ValuePlan::Cast { go_type, inner }`. The inner value is
    /// planned first to preserve the original mutation order (inner emitted
    /// before the target type is formatted).
    pub(crate) fn plan_cast(
        &mut self,
        expression: &Expression,
        ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let inner = self.plan_operand(expression, ctx);

        if self.facts.is_interface(ty) {
            let (mut setup, value) = inner.into_parts();
            let source_ty = expression.get_type();
            let coercion = Coercion::resolve(self, &source_ty, ty, CoercionDirection::Internal);
            let (coercion_setup, coerced) = coercion.lower(self, value);
            setup.extend(coercion_setup);
            return value_plan_from_statements(setup, coerced);
        }

        let go_type = self.go_type_string(ty);

        if let Some(source_go_type) = self.shift_pin_go_type(expression, ty) {
            return ValuePlan::Cast {
                go_type,
                inner: Box::new(ValuePlan::Cast {
                    go_type: source_go_type,
                    inner: Box::new(inner),
                }),
            };
        }

        ValuePlan::Cast {
            go_type,
            inner: Box::new(inner),
        }
    }

    fn shift_pin_go_type(&self, expression: &Expression, target_ty: &Type) -> Option<String> {
        let target_is_float = target_ty
            .underlying_simple_kind()
            .is_some_and(|kind| kind.is_float());
        if !target_is_float || !self.contains_untyped_constant_shift(expression) {
            return None;
        }
        let source_ty = expression.get_type();
        source_ty
            .underlying_simple_kind()
            .is_some_and(|kind| kind.integer_range().is_some())
            .then(|| self.go_type_string(&source_ty))
    }

    /// Plan a `&inner` reference, hoisting to a temp when the inner is
    /// Go-unaddressable.
    pub(crate) fn plan_reference(&mut self, inner: &Expression, ty: &Type) -> ValuePlan {
        if inner.get_type().is_unit() && matches!(inner.unwrap_parens(), Expression::Call { .. }) {
            let staged = self.stage_operand(inner.unwrap_parens(), ExpressionContext::value());
            let mut setup = staged.setup;
            if !staged.value.is_empty() {
                setup.push(LoweredStatement::Expression(ExpressionStatementPlan {
                    form: ExpressionStatementForm::Async {
                        value: ValuePlan::Operand(staged.value),
                    },
                }));
            }
            let tmp = self.hoist_tmp_value_statement(&mut setup, "ref", "struct{}{}");
            return value_plan_from_statements(setup, format!("&{}", tmp));
        }

        let (mut setup, emitted) = self
            .lower_value(inner, ExpressionContext::value())
            .into_parts();

        let value = if inner.get_type() == *ty {
            emitted
        } else if self.is_go_unaddressable(inner) || matches!(inner.get_type(), Type::Function(_)) {
            let tmp = self.hoist_tmp_value_statement(&mut setup, "ref", &emitted);
            format!("&{}", tmp)
        } else {
            format!("&{}", emitted)
        };
        value_plan_from_statements(setup, value)
    }

    pub(crate) fn contains_newtype_access(&self, expression: &Expression) -> bool {
        let mut current = expression;
        while let Expression::DotAccess {
            expression: inner,
            member,
            ..
        } = current
        {
            if member.parse::<usize>().is_ok()
                && self.is_newtype_struct(&inner.get_type().strip_refs())
            {
                return true;
            }
            current = inner;
        }
        false
    }

    fn lower_assignment_operand(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Vec<LoweredStatement> {
        let rhs_staged = self.stage_composite(value, ExpressionContext::value());

        let rhs = crate::statements::assignments::RhsEffects {
            has_setup: !rhs_staged.setup.is_empty(),
            has_effectful_call: self.contains_effectful_call(value),
        };
        let mut setup: Vec<LoweredStatement> = Vec::new();
        let target_str = if is_order_sensitive(target) {
            self.emit_left_value_capturing(&mut setup, target, rhs)
        } else {
            self.emit_left_value(&mut setup, target)
        };
        setup.extend(rhs_staged.setup);

        if let Expression::DotAccess {
            expression: receiver,
            ty,
            ..
        } = target
            && self.go_imported_shape(&receiver.get_type()).is_some()
            && self.is_go_nullable(ty)
        {
            let coercion =
                Coercion::resolve(self, &value.get_type(), ty, CoercionDirection::ToGoBoundary);
            let (coercion_setup, unwrapped) = coercion.lower(self, rhs_staged.value);
            setup.extend(coercion_setup);
            setup.push(LoweredStatement::RawGo(format!(
                "{} = {}\n",
                target_str, unwrapped
            )));
        } else {
            setup.push(LoweredStatement::RawGo(format!(
                "{} = {}\n",
                target_str, rhs_staged.value
            )));
        }
        setup
    }

    pub(crate) fn plan_range_value(
        &mut self,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        _inclusive: bool,
        ty: &Type,
    ) -> ValuePlan {
        let type_string = self.go_type_string(ty);

        let mut stages: Vec<StagedExpression> = Vec::new();
        let has_start = start.is_some();
        if let Some(s) = start {
            stages.push(self.stage_operand(s, ExpressionContext::value()));
        }
        if let Some(e) = end {
            stages.push(self.stage_operand(e, ExpressionContext::value()));
        }

        if stages.is_empty() {
            return ValuePlan::Operand("struct{}{}".to_string());
        }

        let (setup, values) = self.sequence_structured(stages, "_range");
        let mut fields = Vec::new();
        if has_start {
            fields.push(("Start".to_string(), values[0].clone()));
            if values.len() > 1 {
                fields.push(("End".to_string(), values[1].clone()));
            }
        } else {
            fields.push(("End".to_string(), values[0].clone()));
        }

        let value = emit_struct_literal(&type_string, &fields, ExpressionContext::value());
        value_plan_from_statements(setup, value)
    }

    pub(crate) fn with_fresh_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let frame = self.scope.enter_isolated_function();
        let result = f(self);
        self.scope.exit_isolated_function(frame);
        result
    }

    fn with_eager_operand_capture<R>(
        &mut self,
        enabled: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.function_state.set_eager_operand_capture(enabled);
        let result = f(self);
        self.function_state.set_eager_operand_capture(previous);
        result
    }

    /// Plan a `Task`/`Defer` operand.
    pub(crate) fn plan_async_wrapper(
        &mut self,
        keyword: &str,
        expression: &Expression,
    ) -> ValuePlan {
        if let Expression::Block { .. } = expression {
            let body = self.with_fresh_scope(|planner| planner.lower_block_as_body(expression));
            let setup = vec![LoweredStatement::Expression(ExpressionStatementPlan {
                form: ExpressionStatementForm::AsyncBlock {
                    keyword: keyword.to_string(),
                    body,
                },
            })];
            return value_plan_from_statements(setup, String::new());
        }

        let mut setup: Vec<LoweredStatement> = Vec::new();
        if let Some(call_str) = self.emit_go_call_discarded(&mut setup, expression) {
            return value_plan_from_statements(setup, format!("{} {}", keyword, call_str));
        }

        let (setup, inner) = self
            .lower_value(expression, ExpressionContext::value())
            .into_parts();
        if needs_iife_for_async(expression, &inner) {
            let (mut setup, inner) = self.with_eager_operand_capture(true, |planner| {
                planner
                    .lower_value(expression, ExpressionContext::value())
                    .into_parts()
            });
            let mut body_statements = Vec::new();
            if !inner.is_empty() {
                let line = if expression.get_type().is_unit() {
                    format!("{}\n", inner)
                } else {
                    format!("_ = {}\n", inner)
                };
                body_statements.push(LoweredStatement::RawGo(line));
            }
            let body = LoweredBlock {
                statements: body_statements,
            };
            setup.push(LoweredStatement::Expression(ExpressionStatementPlan {
                form: ExpressionStatementForm::AsyncBlock {
                    keyword: keyword.to_string(),
                    body,
                },
            }));
            return value_plan_from_statements(setup, String::new());
        }
        value_plan_from_statements(setup, format!("{} {}", keyword, inner))
    }
}

impl Planner<'_> {
    fn is_go_unaddressable(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Call { .. } => true,
            Expression::Identifier { value, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function(_)) =>
            {
                self.identifier_is_unaddressable(value, ty)
            }
            Expression::DotAccess { expression, ty, .. }
                if !matches!(ty.unwrap_forall(), Type::Function(_)) =>
            {
                self.dot_access_is_unaddressable(expression, ty)
            }
            _ => false,
        }
    }

    fn identifier_is_unaddressable(&self, value: &str, ty: &Type) -> bool {
        match self.scope.resolve_identifier_binding(value) {
            Some(BindingValue::GoName(_)) => false,
            Some(BindingValue::InlineExpr(_)) => true,
            None => self.ty_is_enum(ty),
        }
    }

    fn dot_access_is_unaddressable(&self, receiver: &Expression, ty: &Type) -> bool {
        if !self.ty_is_enum(ty) {
            return false;
        }
        let Type::Nominal {
            id: receiver_id, ..
        } = &receiver.get_type()
        else {
            return false;
        };
        matches!(
            self.facts.definition(receiver_id.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Enum { .. } | DefinitionBody::TypeAlias { .. })
        )
    }

    /// Whether `ty` is a nominal type whose definition is an `enum`.
    fn ty_is_enum(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        matches!(
            self.facts.definition(id.as_str()).map(|d| &d.body),
            Some(DefinitionBody::Enum { .. })
        )
    }
}

fn is_native_method_call(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Call {
            call_kind: Some(CallKind::NativeMethod(_) | CallKind::NativeMethodIdentifier(_)),
            ..
        }
    )
}

fn needs_iife_for_async(expression: &Expression, emitted: &str) -> bool {
    if !is_native_method_call(expression) {
        return false;
    }
    !is_plain_go_call(emitted)
}

/// Whether emitted Go text is a plain non-builtin call. Valid as a `go` or
/// `defer` target, and evaluated in lexical order among sibling operands.
pub(crate) fn is_plain_go_call(emitted: &str) -> bool {
    let trimmed = emitted.trim();
    if !trimmed.ends_with(')') {
        return false;
    }
    let Some(open) = trimmed.find('(') else {
        return false;
    };
    let callee = trimmed[..open].trim_end();
    if callee.is_empty() || callee.starts_with('[') {
        return false;
    }
    !matches!(
        callee,
        "len" | "cap" | "append" | "copy" | "new" | "make" | "complex" | "real" | "imag"
    )
}
