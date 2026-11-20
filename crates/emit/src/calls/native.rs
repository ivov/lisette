use super::{NativeCallContext, NativeCallForm, split_native_receiver};
use crate::Planner;
use crate::analyze::facts::EmitFacts;
use crate::calls::slice_loop::SliceLoopTarget;
use crate::control_flow::propagation::plain_return;
use crate::expressions::access::index_access::range_var_bounds;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::ir::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement};
use crate::ir::go_expression::GoExpressionNode;
use crate::ir::go_expression::{BinaryOp, FunctionLiteralLayout, GoParameter, UnaryOp};
use crate::ir::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::state::expression::ExpressionContext;
use crate::statements::assignments::lvalues_match;
use crate::types::go_type::GoType;
use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::program::{
    CallOperation, DotAccessResolution, ElementEquality, EqualityRecipe, NativeTypeKind,
};
use syntax::program::{NativeInlineForm, NativeInlinePackage, NativeOperation};
use syntax::types::{CompoundKind, Type};

pub(super) struct NativeCallResult {
    pub setup: Vec<Statement>,
    pub value: GoExpression,
}

impl NativeCallResult {
    pub(super) fn new(setup: Vec<Statement>, value: GoExpression) -> Self {
        Self { setup, value }
    }
}

fn clone_slice(receiver: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Slices, "Clone"),
        vec![receiver],
    )
}

pub(crate) fn slice_growth_call(
    method: &str,
    receiver: GoExpression,
    arguments: &[GoExpression],
) -> GoExpression {
    try_inline_native_method(&NativeTypeKind::Slice, method, &receiver, arguments, false)
        .expect("slice growth has an inline rule")
}

fn clip_shared_capacity(receiver: GoExpression) -> GoExpression {
    GoExpression::read_only_call(
        GoExpression::generated(GeneratedPackage::Slices, "Clip"),
        vec![receiver],
    )
}

pub(crate) fn is_clip_safe_path(value: &GoExpression) -> bool {
    fn is_path(node: &GoExpressionNode) -> bool {
        match node {
            GoExpressionNode::Identifier(_) | GoExpressionNode::Qualified { .. } => true,
            GoExpressionNode::Selector { base, .. } => is_path(base),
            _ => false,
        }
    }
    is_path(value.node())
}

fn is_fresh_slice_value(operation: Option<&CallOperation>, receiver: &Expression) -> bool {
    if let Expression::Literal {
        literal: Literal::Slice(_),
        ..
    } = receiver.unwrap_parens()
    {
        return true;
    }
    match operation {
        Some(CallOperation::NativeConstructor {
            kind: NativeTypeKind::Slice,
            ..
        }) => true,
        Some(CallOperation::Native {
            kind: NativeTypeKind::Slice,
            method,
        }) => NativeOperation::new(NativeTypeKind::Slice, method).returns_fresh_slice(),
        Some(CallOperation::Native {
            kind: NativeTypeKind::Array,
            method,
        }) => method == "to_slice",
        _ => false,
    }
}

fn retired_covers_receiver(target: &Expression, receiver: &Expression) -> bool {
    let mut current = receiver.unwrap_parens();
    loop {
        if lvalues_match(target, current) {
            return true;
        }
        match current {
            Expression::DotAccess { expression, .. } => current = expression.unwrap_parens(),
            _ => return false,
        }
    }
}

fn build_inline(
    native_type: &NativeTypeKind,
    form: NativeInlineForm,
    package: NativeInlinePackage,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    negated: bool,
) -> GoExpression {
    match form {
        NativeInlineForm::Call(callee) => {
            let callee = match package {
                NativeInlinePackage::None => GoExpression::external_name(callee.to_string()),
                NativeInlinePackage::Slices => {
                    GoExpression::generated(GeneratedPackage::Slices, callee)
                }
                NativeInlinePackage::Strings => {
                    GoExpression::generated(GeneratedPackage::Strings, callee)
                }
                NativeInlinePackage::Prelude => {
                    GoExpression::generated(GeneratedPackage::Prelude, callee)
                }
            };
            let mut all = vec![receiver.clone()];
            all.extend(arguments.iter().cloned());
            GoExpression::call(callee, all)
        }
        NativeInlineForm::IsEmpty => {
            let operator = if negated { BinaryOp::Ne } else { BinaryOp::Eq };
            if matches!(native_type, NativeTypeKind::String) {
                return GoExpression::binary(
                    receiver.clone(),
                    operator,
                    GoExpression::literal("\"\"".to_string()),
                );
            }
            GoExpression::binary(
                GoExpression::call(
                    GoExpression::external_name("len".to_string()),
                    vec![receiver.clone()],
                ),
                operator,
                GoExpression::literal("0".to_string()),
            )
        }
        NativeInlineForm::Receiver => receiver.clone(),
        NativeInlineForm::Conversion(go_type) => GoExpression::call(
            GoExpression::type_name(GoType::slice_of(&GoType::new(go_type))),
            vec![receiver.clone()],
        ),
        NativeInlineForm::Index => {
            let base = receiver.clone();
            let index = arguments
                .first()
                .expect("an index rule takes one argument")
                .clone();
            GoExpression::index(base, index)
        }
    }
}

pub(super) fn try_inline_native_method(
    native_type: &NativeTypeKind,
    method: &str,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    negated: bool,
) -> Option<GoExpression> {
    if !negated && method == "append" && arguments.is_empty() {
        return Some(receiver.clone());
    }
    let rule = NativeOperation::new(*native_type, method).inline_rule(arguments.len())?;
    if negated && !matches!(rule.form, NativeInlineForm::IsEmpty) {
        return None;
    }
    Some(build_inline(
        native_type,
        rule.form,
        rule.package,
        receiver,
        arguments,
        negated,
    ))
}

fn contains_call_expression(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. } | Expression::Task { .. }
    ) || expression
        .children()
        .into_iter()
        .any(contains_call_expression)
}

fn array_to_slice_receiver<'e>(
    operation: Option<&CallOperation>,
    expression: &'e Expression,
) -> Option<&'e Expression> {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    let function = callee.unwrap_parens();
    if spread.is_some()
        || !matches!(
            operation,
            Some(CallOperation::Native {
                kind: NativeTypeKind::Array,
                method,
            }) if method == "to_slice"
        )
    {
        return None;
    }
    match split_native_receiver(function, args)? {
        (receiver, []) => Some(receiver),
        _ => None,
    }
}

fn has_inline_negation(native_type: &NativeTypeKind, method: &str, arity: usize) -> bool {
    NativeOperation::new(*native_type, method)
        .inline_rule(arity)
        .is_some_and(|rule| matches!(rule.form, NativeInlineForm::IsEmpty))
}

struct StagedNativeMethod {
    setup: Vec<Statement>,
    receiver: GoExpression,
    arguments: Vec<GoExpression>,
}

impl StagedNativeMethod {
    fn finish(self, value: GoExpression) -> NativeCallResult {
        NativeCallResult::new(self.setup, value)
    }
}

impl Planner<'_> {
    pub(crate) fn detach_slice_growth(
        &mut self,
        setup: &mut Vec<Statement>,
        method: &str,
        receiver: GoExpression,
        arguments: &[GoExpression],
        result_ty: &Type,
    ) -> GoExpression {
        let clipped = clip_shared_capacity(receiver.clone());
        let shared = match method {
            "append" => {
                let Some(last) = arguments.last() else {
                    return clone_slice(receiver);
                };
                let GoExpressionNode::Spread(spread) = last.node() else {
                    return slice_growth_call(method, clipped, arguments);
                };
                last.peeled(spread)
            }
            "reserve" => {
                let count = &arguments[0];
                if let Some(value) = count
                    .as_literal()
                    .and_then(|value| value.parse::<i64>().ok())
                {
                    return if value == 0 {
                        clone_slice(receiver)
                    } else {
                        slice_growth_call(method, clipped, arguments)
                    };
                }
                count.clone()
            }
            _ => unreachable!("not a slice growth method: {method}"),
        };
        let shared = self.stable_source(setup, "added", shared);
        self.with_scope(|this| {
            let mut statements = Vec::new();
            let (count, arguments) = if method == "append" {
                let count = GoExpression::read_only_call(
                    GoExpression::external_name("len".to_string()),
                    vec![shared.clone()],
                );
                (count, [GoExpression::spread(shared)])
            } else {
                (shared.clone(), [shared])
            };
            statements.push(
                LoweredStatement::If(IfPlan::plain(
                    GoExpression::binary(
                        count,
                        BinaryOp::Eq,
                        GoExpression::literal("0".to_string()),
                    ),
                    LoweredBlock {
                        statements: vec![plain_return(clone_slice(receiver))],
                    },
                    ElseArm::None,
                ))
                .into(),
            );
            statements.push(plain_return(slice_growth_call(method, clipped, &arguments)));
            GoExpression::immediate_call(
                this.go_type(result_ty),
                LoweredBlock { statements },
                FunctionLiteralLayout::MultiLine,
            )
        })
    }

    pub(super) fn lower_native_method(&mut self, context: &NativeCallContext) -> NativeCallResult {
        let mut result = self.lower_selected_native_method(context);

        let operation = NativeOperation::new(context.native_type, context.method);
        if operation.read_only_without_callbacks() {
            result.value = result.value.into_read_only_call();
        }
        result
    }

    fn lower_selected_native_method(&mut self, context: &NativeCallContext) -> NativeCallResult {
        let (receiver_expression, arguments) = context
            .receiver_and_arguments()
            .expect("native method has a receiver");

        if matches!(context.native_type, NativeTypeKind::String)
            && context.method == "substring"
            && !arguments.is_empty()
        {
            return self.lower_string_substring(
                receiver_expression,
                arguments,
                context.capture_boundary,
            );
        }

        if context.method == "equals"
            && matches!(
                context.native_type,
                NativeTypeKind::Slice | NativeTypeKind::Map
            )
        {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let staged = self.stage_native_method(context);
                let recipe = self.facts.native_equality(context);
                let body = self.equality_test(
                    staged.receiver.clone(),
                    staged.arguments[0].clone(),
                    &recipe,
                    false,
                );
                return staged.finish(body);
            }
        }

        if context.method == "contains" && matches!(context.native_type, NativeTypeKind::Slice) {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if receiver_ty.is_slice() {
                let mut staged = self.stage_native_method(context);
                let body = if let EqualityRecipe::Slice(Some(element)) =
                    &self.facts.native_equality(context)
                {
                    let searched = staged.arguments[0].clone();
                    let target =
                        self.hoist_tmp_value_statement(&mut staged.setup, "want", searched);
                    let predicate =
                        self.contains_predicate(element, GoExpression::identifier(target));
                    GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Slices, "ContainsFunc"),
                        vec![staged.receiver.clone(), predicate],
                    )
                } else {
                    GoExpression::read_only_call(
                        GoExpression::generated(GeneratedPackage::Slices, "Contains"),
                        vec![staged.receiver.clone(), staged.arguments[0].clone()],
                    )
                };
                return staged.finish(body);
            }
        }

        if matches!(context.native_type, NativeTypeKind::Array)
            && NativeOperation::new(context.native_type, context.method).reads_array_in_place()
            && self.facts.native_kind(&receiver_expression.get_type())
                == Some(NativeTypeKind::Array)
        {
            let mut staged = self.stage_native_method(context);
            let index = staged.arguments.first().cloned();
            let body = self.plan_array_method_body(
                context.method,
                receiver_expression,
                staged.receiver.clone(),
                index,
                &mut staged.setup,
            );
            return staged.finish(body);
        }

        if matches!(context.native_type, NativeTypeKind::Slice)
            && let Some(result) = self.try_lower_slice_loop(context, SliceLoopTarget::Fresh)
        {
            return result;
        }

        if context.method == "clone" {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if self.facts.is_clone_container(&receiver_ty) {
                let staged = self.stage_native_method(context);
                let body = self.clone_expression(staged.receiver.clone(), &receiver_ty);
                return staged.finish(body);
            }
        }

        let mut staged = self.stage_native_method(context);
        if self
            .facts
            .growth_detach_applies(context, receiver_expression)
        {
            let detached = self.detach_slice_growth(
                &mut staged.setup,
                context.method,
                staged.receiver.clone(),
                &staged.arguments,
                &receiver_expression.get_type(),
            );
            return staged.finish(detached);
        }

        if let Some(inlined) = try_inline_native_method(
            &context.native_type,
            context.method,
            &staged.receiver,
            &staged.arguments,
            false,
        ) {
            return staged.finish(inlined);
        }

        let fn_name = GoExpression::generated(
            GeneratedPackage::Prelude,
            format!(
                "{}{}",
                method_prefix(context.native_type),
                go_name::snake_to_camel(context.method)
            ),
        );
        let written = self
            .facts
            .checked(context.function)
            .callee()
            .written_type_arguments(context.written_type_arguments);
        let type_args = match context.form {
            NativeCallForm::Dot { receiver }
                if context.written_type_arguments > 0 && context.call_ty.is_some() =>
            {
                self.format_type_args_with_receiver(&receiver.get_type(), written)
            }
            NativeCallForm::Dot { .. }
            | NativeCallForm::Identifier
            | NativeCallForm::Constructor => self.format_type_arg_iter(written),
        };
        let mut emitted_args = vec![staged.receiver.clone()];
        let first_param = usize::from(!matches!(context.form, NativeCallForm::Dot { .. }));
        let argument_expressions = context
            .receiver_and_arguments()
            .map_or(&[][..], |(_, arguments)| arguments);
        emitted_args.extend(staged.arguments.iter().enumerate().map(|(index, value)| {
            let parameter = context.abi.param(first_param + index);
            match argument_expressions.get(index).zip(parameter) {
                Some((argument, parameter))
                    if self.facts.converts_for_inference(argument, parameter) =>
                {
                    GoExpression::conversion(self.go_type(&parameter.instantiated), value.clone())
                }
                _ => value.clone(),
            }
        }));
        staged.finish(GoExpression::call(
            GoExpression::instantiation(fn_name, type_args),
            emitted_args,
        ))
    }

    fn plan_array_method_body(
        &mut self,
        method: &str,
        receiver_expr: &Expression,
        receiver: GoExpression,
        index: Option<GoExpression>,
        setup: &mut Vec<Statement>,
    ) -> GoExpression {
        match method {
            "to_slice" => {
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Clone"),
                    vec![view],
                )
            }
            "get" => {
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Prelude, "SliceGet"),
                    vec![view, index.expect("get needs an index")],
                )
            }
            other => unreachable!("not a native array method: {other}"),
        }
    }

    fn sliceable_receiver(
        &mut self,
        expression: &Expression,
        receiver: GoExpression,
        setup: &mut Vec<Statement>,
    ) -> GoExpression {
        let base = if self.receiver_is_addressable(expression) {
            receiver
        } else {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, "arr", receiver))
        };
        GoExpression::slice(base, None, None, None)
    }

    pub(crate) fn receiver_is_addressable(&self, expression: &Expression) -> bool {
        if self.facts.is_reference(&expression.get_type()) {
            return true;
        }
        match expression.unwrap_parens() {
            Expression::Identifier { .. } => true,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => true,
            Expression::DotAccess {
                expression: base,
                resolution,
                ..
            } => {
                if matches!(
                    resolution,
                    DotAccessResolution::TupleStructField { is_newtype: true }
                ) {
                    return false;
                }
                let origin = base.unwrap_parens();
                let fresh_value = matches!(origin, Expression::StructCall { .. })
                    || (matches!(origin, Expression::Call { .. })
                        && !self.facts.is_reference(&base.get_type()));
                !fresh_value && self.receiver_is_addressable(base)
            }
            Expression::IndexedAccess {
                expression: base, ..
            } => match self.facts.strip_and_peel(&base.get_type()).as_compound() {
                Some((CompoundKind::Map, _)) => false,
                Some((CompoundKind::Slice, _)) => true,
                _ => self.receiver_is_addressable(base),
            },
            _ => false,
        }
    }

    pub(super) fn try_plan_negated_native_method(
        &mut self,
        setup: &mut Vec<Statement>,
        ctx: &NativeCallContext,
    ) -> Option<GoExpression> {
        let (_, arguments) = ctx.receiver_and_arguments()?;
        if !has_inline_negation(&ctx.native_type, ctx.method, arguments.len()) {
            return None;
        }
        let staged = self.stage_native_method(ctx);
        let inlined = try_inline_native_method(
            &ctx.native_type,
            ctx.method,
            &staged.receiver,
            &staged.arguments,
            true,
        )?;
        setup.extend(staged.setup);
        Some(inlined)
    }

    fn try_stage_to_slice_view(
        &mut self,
        context: &NativeCallContext,
        receiver_expression: &Expression,
        arguments: &[Expression],
    ) -> Option<ValuePlan> {
        if !matches!(context.native_type, NativeTypeKind::Slice) {
            return None;
        }
        if !matches!(context.capture_boundary, CaptureBoundary::SiblingSequence) {
            return None;
        }
        if context.spread.is_some()
            || !NativeOperation::new(context.native_type, context.method)
                .reads_elements_only(arguments.len())
            || arguments.iter().any(contains_call_expression)
        {
            return None;
        }
        let array = array_to_slice_receiver(
            self.facts.call_operation(receiver_expression),
            receiver_expression,
        )?;
        if self.facts.native_kind(&array.get_type()) != Some(NativeTypeKind::Array) {
            return None;
        }
        if matches!(context.method, "contains" | "equals")
            && matches!(
                self.facts.native_equality(context),
                EqualityRecipe::Slice(Some(_))
            )
        {
            return None;
        }
        Some(self.stage_array_view(array))
    }

    fn stage_array_view(&mut self, array: &Expression) -> ValuePlan {
        let mut staged = self.plan_operand(array, ExpressionContext::value());
        if self.facts.is_reference(&array.get_type()) {
            staged = staged.dereference();
        }
        if !self.receiver_is_addressable(array) {
            self.pin_staged(&mut staged, "arr");
        }
        staged.map_observable_expression(|_setup, array| {
            let base = array;
            GoExpression::slice(base, None, None, None)
        })
    }

    fn stage_native_method(&mut self, ctx: &NativeCallContext) -> StagedNativeMethod {
        let (receiver, arguments) = ctx
            .receiver_and_arguments()
            .expect("native method has a receiver");
        let is_dot = matches!(ctx.form, NativeCallForm::Dot { .. });
        let view = self.try_stage_to_slice_view(ctx, receiver, arguments);
        let mut stages = match view {
            None if is_dot => vec![self.plan_operand(receiver, ExpressionContext::value())],
            None => Vec::new(),
            Some(view) => vec![view],
        };
        let first_unstaged = if is_dot { 0 } else { stages.len() };
        stages.extend(self.stage_native_method_args_from(&ctx.abi, ctx.args, first_unstaged));
        let spread_stage = ctx.spread.map(|spread| {
            if matches!(ctx.native_type, NativeTypeKind::Slice) && ctx.method == "append" {
                self.plan_copied_spread(spread)
            } else {
                self.plan_operand(spread, ExpressionContext::value())
            }
        });
        if is_dot && self.facts.is_reference(&receiver.get_type()) {
            let receiver = stages.remove(0).dereference();
            stages.insert(0, receiver);
        }
        if self.facts.growth_detach_applies(ctx, receiver)
            && (!arguments.is_empty() || ctx.spread.is_some())
            && !is_clip_safe_path(stages[0].expression())
        {
            self.pin_staged(&mut stages[0], "recv");
        }

        let sequenced = self.sequence_with_spread_values(
            stages,
            spread_stage,
            SpreadSequenceOptions {
                wrap_to_any: ctx.abi.spread_wraps_to_any(&self.facts, ctx.spread),
                combine: ctx.abi.variadic_combine(usize::from(is_dot)),
                boundary: ctx.capture_boundary,
            },
        );
        let mut values = sequenced.values;
        let receiver = values.remove(0);
        StagedNativeMethod {
            setup: sequenced.setup,
            receiver,
            arguments: values,
        }
    }

    pub(super) fn plan_map_index_pair(
        &mut self,
        expression: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let Expression::Call {
            expression: function,
            args,
            type_arguments,
            ..
        } = expression
        else {
            unreachable!("plan_map_index_pair requires a Call expression");
        };
        let native_type = NativeTypeKind::Map;
        let function = function.unwrap_parens();
        let plan = self
            .plan_call(expression)
            .expect("plan_call yields Some for a Call expression");
        let ctx = NativeCallContext {
            function,
            form: NativeCallForm::method(function),
            args,
            spread: None,
            written_type_arguments: type_arguments.annotations().len(),
            abi: plan.resolved.abi,
            call_ty: None,
            native_type,
            method: "get",
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
        };
        let mut staged = self.stage_native_method(&ctx);
        let receiver = staged.receiver;
        let key = staged.arguments.remove(0);
        (staged.setup, GoExpression::index(receiver, key))
    }

    fn lower_string_substring(
        &mut self,
        receiver_expr: &Expression,
        args: &[Expression],
        capture_boundary: CaptureBoundary,
    ) -> NativeCallResult {
        let arg = &args[0];
        let is_ref_receiver = self.facts.is_reference(&receiver_expr.get_type());
        let deref = |raw: GoExpression| -> GoExpression {
            if is_ref_receiver {
                GoExpression::dereference(raw)
            } else {
                raw
            }
        };

        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = arg
        {
            let mut stages = vec![self.plan_operand(receiver_expr, ExpressionContext::value())];
            if let Some(s) = start.as_deref() {
                stages.push(self.plan_operand(s, ExpressionContext::value()));
            }
            if let Some(e) = end.as_deref() {
                stages.push(self.plan_operand(e, ExpressionContext::value()));
            }
            let sequenced = self.sequence_values(stages, capture_boundary, "arg");
            let mut values = sequenced.values.into_iter();
            let receiver = values.next().expect("substring has a receiver");
            let start_bound = start
                .is_some()
                .then(|| values.next().expect("range has a start"));
            let end_bound = end.is_some().then(|| {
                let end = values.next().expect("range has an end");
                if *inclusive {
                    GoExpression::binary(end, BinaryOp::Add, GoExpression::literal("1".to_string()))
                } else {
                    end
                }
            });
            return NativeCallResult::new(
                sequenced.setup,
                substring_call(deref(receiver), start_bound, end_bound),
            );
        }

        let arg_ty = arg.get_type();
        let range_kind = self
            .facts
            .range_shape(&arg_ty)
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.plan_operand(receiver_expr, ExpressionContext::value());
        let range_staged = self.stage_or_capture(arg, "range");
        let sequenced =
            self.sequence_values(vec![receiver_staged, range_staged], capture_boundary, "arg");
        let mut values = sequenced.values.into_iter();
        let receiver = values.next().expect("substring has a receiver");
        let range = values.next().expect("substring has a range");
        let (start, end) = range_var_bounds(&range, range_kind);
        NativeCallResult::new(sequenced.setup, substring_call(deref(receiver), start, end))
    }

    pub(crate) fn equality_expression(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        recipe: &EqualityRecipe,
    ) -> GoExpression {
        self.equality_test(lhs, rhs, recipe, false)
    }

    pub(crate) fn inequality_expression(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        recipe: &EqualityRecipe,
    ) -> GoExpression {
        self.equality_test(lhs, rhs, recipe, true)
    }

    fn equality_test(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        recipe: &EqualityRecipe,
        negated: bool,
    ) -> GoExpression {
        let operator = if negated { BinaryOp::Ne } else { BinaryOp::Eq };
        let call = match recipe {
            EqualityRecipe::Operator => return GoExpression::binary(lhs, operator, rhs),
            EqualityRecipe::Method { .. } => GoExpression::call(
                GoExpression::selector(lhs, self.facts.equals_method_go_name().to_string()),
                vec![rhs],
            ),
            EqualityRecipe::Slice(element) | EqualityRecipe::Map(element) => {
                let package = if matches!(recipe, EqualityRecipe::Slice(_)) {
                    GeneratedPackage::Slices
                } else {
                    GeneratedPackage::Maps
                };
                match element {
                    Some(element) => {
                        let eq = self.equality_closure(element);
                        GoExpression::call(
                            GoExpression::generated(package, "EqualFunc"),
                            vec![lhs, rhs, eq],
                        )
                    }
                    None => GoExpression::call(
                        GoExpression::generated(package, "Equal"),
                        vec![lhs, rhs],
                    ),
                }
            }
        };
        if negated {
            GoExpression::unary(UnaryOp::Not, call)
        } else {
            call
        }
    }

    fn equality_closure(&mut self, element: &ElementEquality) -> GoExpression {
        let go_ty = self.go_type(&element.ty);
        let a = self.fresh_local(Some("a"));
        let b = self.fresh_local(Some("b"));
        let body = self.equality_expression(
            GoExpression::identifier(a.clone()),
            GoExpression::identifier(b.clone()),
            &element.recipe,
        );
        predicate_literal(
            vec![
                GoParameter::new(a.clone(), go_ty.clone()),
                GoParameter::new(b.clone(), go_ty.clone()),
            ],
            body,
        )
    }

    fn contains_predicate(
        &mut self,
        element: &ElementEquality,
        target: GoExpression,
    ) -> GoExpression {
        let go_ty = self.go_type(&element.ty);
        let candidate = self.fresh_local(Some("e"));
        let body = self.equality_expression(
            GoExpression::identifier(candidate.clone()),
            target,
            &element.recipe,
        );
        predicate_literal(
            vec![GoParameter::new(candidate.clone(), go_ty.clone())],
            body,
        )
    }
}

fn predicate_literal(parameters: Vec<GoParameter>, body: GoExpression) -> GoExpression {
    GoExpression::function_literal(
        parameters,
        GoType::new("bool"),
        LoweredBlock {
            statements: vec![plain_return(body)],
        },
        FunctionLiteralLayout::Inline,
    )
}

fn substring_call(
    receiver: GoExpression,
    start: Option<GoExpression>,
    end: Option<GoExpression>,
) -> GoExpression {
    let (function, bounds) = match (start, end) {
        (Some(start), Some(end)) => ("Substring", vec![start, end]),
        (Some(start), None) => ("SubstringFrom", vec![start]),
        (None, Some(end)) => ("SubstringTo", vec![end]),
        (None, None) => unreachable!("`s.substring(..)` is rejected upstream"),
    };
    let mut arguments = vec![receiver];
    arguments.extend(bounds);
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Prelude, function),
        arguments,
    )
}

fn method_prefix(kind: NativeTypeKind) -> &'static str {
    match kind {
        NativeTypeKind::String => "String",
        _ => kind.name(),
    }
}

impl EmitFacts<'_> {
    fn native_equality(&self, context: &NativeCallContext<'_>) -> EqualityRecipe {
        self.checked(context.function)
            .equality()
            .expect("internal error: a built-in comparison has a selected equality")
            .clone()
    }

    fn growth_detach_applies(&self, context: &NativeCallContext, receiver: &Expression) -> bool {
        self.checked(context.function).callee().copies_storage
            && !is_fresh_slice_value(self.call_operation(receiver), receiver)
            && !context
                .retired_receiver
                .is_some_and(|target| retired_covers_receiver(target, receiver))
    }
}
