use crate::abi::is_tagged_shape_fn_value;
use crate::expressions::access::struct_call::emit_struct_literal;
use syntax::program::DefinitionBody;

use crate::EmitEffects;
use crate::GoCallStrategy;
use crate::Planner;
use crate::Renderer;
use crate::ReturnContext;
use crate::abi::AbiShape;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::abi::transition::{emit_callee_abi_wrapping, emit_lisette_callback_wrapper};
use crate::calls::CallBoundary;
use crate::context::expression::ExpressionContext;
use crate::expressions::emission::StagedExpression;
use crate::is_order_sensitive;
use crate::plan::bodies::{
    ExpressionStatementForm, ExpressionStatementPlan, LoweredBlock, LoweredStatement,
};
use crate::plan::values::{ValuePlan, setup_from_string, value_plan_from_statements};
use crate::state::bindings::BindingValue;
use crate::write_line;
use syntax::ast::Expression;
use syntax::program::CallKind;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn declare_result_var(
        &mut self,
        output: &mut String,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let result_var = self.fresh_var(None);
        write_line!(output, "var {} {}", result_var, self.go_type_string(ty, fx));
        self.declare(&result_var);
        result_var
    }

    pub(crate) fn emit_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        if let Some(strategy) = self.classify_go_fn_value(expression) {
            if self.go_fn_matches_lowered_slot(expression, &strategy, ctx) {
                return self.emit_operand(output, expression, ctx, fx);
            }
            if self.go_fn_needs_lowered_tuple_adapter(expression, ctx) {
                return self.emit_go_fn_lowered_tuple_adapter(output, expression, fx);
            }
            return self.emit_go_fn_wrapper(output, expression, &strategy, fx);
        }

        if self.is_go_array_return_value(expression) {
            return self.emit_array_return_wrapper(output, expression, fx);
        }

        self.emit_operand(output, expression, ctx, fx)
    }

    /// Typed counterpart of `emit_value`.
    pub(crate) fn lower_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if self.classify_go_fn_value(expression).is_some()
            || self.is_go_array_return_value(expression)
        {
            let mut buffer = String::new();
            let value = self.emit_value(&mut buffer, expression, ctx, fx);
            return (setup_from_string(buffer), value);
        }
        let plan = self.plan_operand(expression, ctx, fx);
        let staged = StagedExpression::from_plan(plan, expression);
        (staged.setup, staged.value)
    }

    /// Typed counterpart of `emit_composite_value`.
    pub(crate) fn lower_composite_value(
        &mut self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let staged = self.stage_composite(expression, ctx, fx);
        (staged.setup, staged.value)
    }

    /// Wrap a captured tagged-shape prelude fn ref into a lowered-ABI closure
    /// so its Go type matches what the rest of the pipeline expects.
    fn maybe_lower_tagged_fn_ref(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ty: &Type,
        raw: String,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
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
        emit_lisette_callback_wrapper(self, output, &raw, fn_ty, fx)
    }

    /// True when a Go function value's natural ABI matches the slot's
    /// lowered shape — wrapping would be identity.
    fn go_fn_matches_lowered_slot(
        &self,
        expression: &Expression,
        strategy: &GoCallStrategy,
        ctx: ExpressionContext<'_>,
    ) -> bool {
        if ctx.forces_tagged_go_function() {
            return false;
        }
        let fn_ty = expression.get_type();
        let Type::Function(f) = fn_ty.unwrap_forall() else {
            return false;
        };
        let Some(shape) = self.classify_direct_emission(&f.return_type) else {
            return false;
        };
        if self.fallible_tuple_return(&f.return_type) {
            return false;
        }
        shape.matches_go_strategy(strategy)
    }

    /// True for a `Result`/`Partial` whose ok-type is a multi-element tuple, which lowers to one bundled value.
    fn fallible_tuple_return(&self, return_type: &Type) -> bool {
        matches!(
            self.classify_direct_emission(return_type),
            Some(AbiShape::ResultTuple | AbiShape::PartialTuple)
        ) && self
            .facts
            .peel_alias(return_type)
            .ok_type()
            .tuple_arity()
            .is_some_and(|arity| arity >= 2)
    }

    /// True when a tuple-ok fallible Go function value must be wrapped to match a lowered slot.
    fn go_fn_needs_lowered_tuple_adapter(
        &self,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> bool {
        if ctx.forces_tagged_go_function() {
            return false;
        }
        let fn_ty = expression.get_type();
        let Type::Function(f) = fn_ty.unwrap_forall() else {
            return false;
        };
        self.fallible_tuple_return(&f.return_type)
    }

    pub(crate) fn emit_composite_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        if expression.get_type().is_unit()
            && matches!(
                expression.unwrap_parens(),
                Expression::Call { .. } | Expression::Block { .. }
            )
        {
            let call_str = self.emit_value(output, expression, ctx, fx);
            if !call_str.is_empty() {
                write_line!(output, "{call_str}");
            }
            return "struct{}{}".to_string();
        }
        self.emit_value(output, expression, ctx, fx)
    }

    /// Emit a value-position expression: plan, then render.
    pub(crate) fn emit_operand(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        let plan = self.plan_operand(expression, ctx, fx);
        Renderer.render_value(output, &plan)
    }

    /// String-emit body for expression kinds not yet lowered to a
    /// structured `ValuePlan`.
    pub(crate) fn emit_operand_raw(
        &mut self,
        output: &mut String,
        expression: &Expression,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        match expression {
            Expression::Literal { literal, ty, .. } => {
                self.emit_literal(output, literal, ty, ctx.ambient_return_ctx(), fx)
            }
            Expression::Identifier { value, ty, .. } => {
                let raw = self.emit_identifier(value, ty, ctx, fx);
                self.maybe_lower_tagged_fn_ref(output, expression, ty, raw, ctx, fx)
            }
            Expression::Binary { .. } => {
                unreachable!("Binary is handled structurally by plan_operand")
            }
            Expression::Unary { .. } => {
                unreachable!("Unary is handled structurally by plan_operand")
            }
            Expression::Call { ty, .. } => match self.classify_call(expression) {
                CallBoundary::GoWrapped(strategy) => {
                    self.emit_go_wrapped_call(output, expression, &strategy, ty, fx)
                }
                CallBoundary::LoweredCallee(shape) => {
                    fx.require_stdlib();
                    let call_str = self.emit_call(output, expression, Some(ty), ctx, fx);
                    emit_callee_abi_wrapping(self, output, &shape, &call_str, ty, fx)
                }
                CallBoundary::Plain => self.emit_call(output, expression, Some(ty), ctx, fx),
            },
            Expression::DotAccess { .. } => {
                unreachable!("DotAccess is handled structurally by plan_operand")
            }
            Expression::IndexedAccess { .. } => {
                unreachable!("IndexedAccess is handled structurally by plan_operand")
            }
            Expression::StructCall { .. } => {
                unreachable!("StructCall is handled structurally by plan_operand")
            }
            Expression::Paren { .. } => {
                unreachable!("Paren is handled structurally by plan_operand")
            }
            Expression::Reference { .. } => {
                unreachable!("Reference is handled structurally by plan_operand")
            }
            Expression::Task { .. } => {
                unreachable!("Task is handled structurally by plan_operand")
            }
            Expression::Defer { .. } => {
                unreachable!("Defer is handled structurally by plan_operand")
            }
            Expression::RawGo { text } => text.clone(),
            Expression::Unit { .. } => "struct{}{}".to_string(),
            Expression::NoOp => String::new(),
            Expression::Lambda {
                params, body, ty, ..
            } => self.emit_lambda(params, body, ty, ctx, fx),
            Expression::Function {
                params, body, ty, ..
            } => self.emit_lambda(params, body, ty, ctx, fx),
            Expression::Propagate { expression, .. } => {
                let return_ctx = self
                    .resolve_ambient_return_ctx(ctx)
                    .expect("operand-position propagate requires an enclosing return context");
                self.emit_propagate(output, expression, None, &return_ctx, fx)
            }
            Expression::TryBlock { .. } => {
                unreachable!("TryBlock is handled structurally by plan_operand")
            }
            Expression::RecoverBlock { .. } => {
                unreachable!("RecoverBlock is handled structurally by plan_operand")
            }
            Expression::Tuple { .. } => {
                unreachable!("Tuple is handled structurally by plan_operand")
            }
            Expression::If { .. } => {
                unreachable!("If is handled structurally by plan_operand")
            }
            Expression::Loop { .. } => {
                unreachable!("Loop is handled structurally by plan_operand")
            }
            Expression::Match { ty, .. } | Expression::Select { ty, .. } if !ty.is_never() => {
                unreachable!("non-never Match/Select is handled structurally by plan_operand")
            }
            Expression::Match { ty, .. }
            | Expression::Select { ty, .. }
            | Expression::Block { ty, .. } => {
                self.emit_to_operand_temp(output, expression, ty, ctx.ambient_return_ctx(), fx)
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Return {
                expression: return_expression,
                ..
            } => {
                let return_ctx = self
                    .resolve_ambient_return_ctx(ctx)
                    .expect("operand-position return requires an enclosing return context");
                self.emit_return(output, return_expression, &return_ctx, fx);
                String::new()
            }
            Expression::Range { .. } => {
                unreachable!("Range is handled structurally by plan_operand")
            }
            Expression::Cast { .. } => {
                unreachable!("Cast is handled structurally by plan_operand")
            }
            Expression::Assignment { target, value, .. } => {
                self.emit_assignment_operand(output, target, value, fx);
                "struct{}{}".to_string()
            }
            _ => unreachable!("unexpected expression in emit: {:?}", expression),
        }
    }

    /// Emit a Go tuple literal. `in_tail` widens slot types to the declared
    /// return slots so per-element coercion matches the return site.
    pub(crate) fn emit_tuple_value(
        &mut self,
        output: &mut String,
        elements: &[Expression],
        ty: &Type,
        in_tail: bool,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> String {
        let plan = self.plan_tuple_value(elements, ty, in_tail, ambient, fx);
        Renderer.render_value(output, &plan)
    }

    /// Plan a tuple literal as `lisette.MakeTupleN(...)`.
    pub(crate) fn plan_tuple_value(
        &mut self,
        elements: &[Expression],
        ty: &Type,
        in_tail: bool,
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        use value_plan_from_statements;

        let inferred_slot_types: Vec<Type> = match ty {
            Type::Tuple(slots) => slots.clone(),
            _ => Vec::new(),
        };
        let slot_types = self.resolve_tuple_slot_types(inferred_slot_types, in_tail, ambient);

        let stages: Vec<StagedExpression> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let element_ctx = ExpressionContext::value()
                    .with_expected_slot_type(slot_types.get(i))
                    .with_ambient_return_ctx_opt(ambient);
                self.stage_composite(e, element_ctx, fx)
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
                    let (coercion_setup, coerced) = coercion.lower(self, emitted, fx);
                    setup.extend(coercion_setup);
                    coerced
                }
                None => emitted,
            };
            wrapped_expressions.push(value);
        }
        let element_expressions = wrapped_expressions;

        fx.require_stdlib();
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
            let slot_ty_strs: Vec<String> = slot_types
                .iter()
                .map(|t| self.go_type_string(t, fx))
                .collect();
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
        target_type: &syntax::ast::Annotation,
        ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let inner = self.plan_operand(expression, ctx, fx);

        if let Type::Nominal { id, .. } = &self.facts.peel_alias(ty)
            && matches!(
                self.facts.definition(id.as_str()).map(|d| &d.body),
                Some(DefinitionBody::Interface { .. })
            )
        {
            let staged = StagedExpression::from_plan(inner, expression);
            let mut setup = staged.setup;
            let source_ty = expression.get_type();
            let coercion = Coercion::resolve(self, &source_ty, ty, CoercionDirection::Internal);
            let (coercion_setup, coerced) = coercion.lower(self, staged.value, fx);
            setup.extend(coercion_setup);
            return value_plan_from_statements(setup, coerced);
        }

        let go_type = self.annotation_to_go_type(target_type, fx);
        ValuePlan::Cast {
            go_type,
            inner: Box::new(inner),
        }
    }

    /// Plan a `&inner` reference, hoisting to a temp when the inner is
    /// Go-unaddressable.
    pub(crate) fn plan_reference(
        &mut self,
        inner: &Expression,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        if inner.get_type().is_unit() && matches!(inner.unwrap_parens(), Expression::Call { .. }) {
            let staged = self.stage_operand(inner.unwrap_parens(), ExpressionContext::value(), fx);
            let mut setup = staged.setup;
            if !staged.value.is_empty() {
                setup.push(LoweredStatement::Expression(ExpressionStatementPlan {
                    directive: String::new(),
                    form: ExpressionStatementForm::Async {
                        value: ValuePlan::Operand(staged.value),
                    },
                }));
            }
            let tmp = self.hoist_tmp_value_statement(&mut setup, "ref", "struct{}{}");
            return value_plan_from_statements(setup, format!("&{}", tmp));
        }

        let (mut setup, emitted) =
            if self.classify_go_fn_value(inner).is_some() || self.is_go_array_return_value(inner) {
                let mut buffer = String::new();
                let value = self.emit_value(&mut buffer, inner, ExpressionContext::value(), fx);
                (setup_from_string(buffer), value)
            } else {
                let staged = self.stage_operand(inner, ExpressionContext::value(), fx);
                (staged.setup, staged.value)
            };

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

    fn emit_assignment_operand(
        &mut self,
        output: &mut String,
        target: &Expression,
        value: &Expression,
        fx: &mut EmitEffects,
    ) {
        let rhs_staged = self.stage_composite(value, ExpressionContext::value(), fx);

        let target_str = if is_order_sensitive(target) {
            self.emit_left_value_capturing(output, target, !rhs_staged.setup.is_empty(), fx)
        } else {
            self.emit_left_value(output, target, fx)
        };
        output.push_str(&Renderer.render_setup(&rhs_staged.setup));

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
            let unwrapped = coercion.apply(self, output, rhs_staged.value, fx);
            write_line!(output, "{} = {}", target_str, unwrapped);
        } else {
            write_line!(output, "{} = {}", target_str, rhs_staged.value);
        }
    }

    pub(crate) fn plan_range_value(
        &mut self,
        start: &Option<Box<Expression>>,
        end: &Option<Box<Expression>>,
        _inclusive: bool,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let type_string = self.go_type_string(ty, fx);

        let mut stages: Vec<StagedExpression> = Vec::new();
        let has_start = start.is_some();
        if let Some(s) = start {
            stages.push(self.stage_operand(s, ExpressionContext::value(), fx));
        }
        if let Some(e) = end {
            stages.push(self.stage_operand(e, ExpressionContext::value(), fx));
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
        ambient: Option<&ReturnContext>,
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        if let Expression::Block { .. } = expression {
            let return_ctx = ambient
                .cloned()
                .or_else(|| self.current_return_ctx().cloned())
                .expect("async block body requires an enclosing return context");
            let body = self.with_fresh_scope(|planner| {
                planner.lower_block_as_body(expression, &return_ctx, fx)
            });
            let setup = vec![LoweredStatement::Expression(ExpressionStatementPlan {
                directive: String::new(),
                form: ExpressionStatementForm::AsyncBlock {
                    keyword: keyword.to_string(),
                    body,
                },
            })];
            return value_plan_from_statements(setup, String::new());
        }

        let mut buffer = String::new();
        if let Some(call_str) = self.emit_go_call_discarded(&mut buffer, expression, fx) {
            return value_plan_from_statements(
                setup_from_string(buffer),
                format!("{} {}", keyword, call_str),
            );
        }

        let (setup, inner) = self.lower_value(expression, ExpressionContext::value(), fx);
        if needs_iife_for_async(expression, &inner) {
            let (mut setup, inner) = self.with_eager_operand_capture(true, |planner| {
                planner.lower_value(expression, ExpressionContext::value(), fx)
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
                directive: String::new(),
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
    !is_valid_go_async_target(emitted)
}

fn is_valid_go_async_target(emitted: &str) -> bool {
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
