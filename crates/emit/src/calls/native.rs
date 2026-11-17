use super::{NativeCallContext, NativeCallForm, split_native_receiver};
use crate::Planner;
use crate::calls::dispatch::extract_native_method_name;
use crate::calls::slice_loop::SliceLoopTarget;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::expressions::access::index_access::range_var_bounds;
use crate::expressions::staging::SpreadSequenceOptions;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::go_expression::{BinaryOp, FunctionLiteralLayout, GoParameter, UnaryOp};
use crate::plan::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::statements::assignments::lvalues_match;
use crate::types::go_type::GoType;
use crate::types::native;
use crate::types::shape::RangeShape;
use syntax::ast::{Expression, Generic, Literal, UnaryOperator};
use syntax::program::{CallKind, DotAccessResolution, NativeTypeKind};
use syntax::types::{CompoundKind, Type, peel_to_range_type};

pub(super) struct NativeCallResult {
    pub setup: Vec<Statement>,
    pub value: GoExpression,
}

impl NativeCallResult {
    pub(super) fn new(setup: Vec<Statement>, value: GoExpression) -> Self {
        Self { setup, value }
    }
}

use syntax::program::{NativeInlineForm, NativeInlinePackage, NativeOperation};

fn clone_slice(receiver: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Slices, "Clone"),
        vec![receiver],
    )
}

/// `append(receiver, arguments...)` or `slices.Grow(receiver, arguments...)`.
pub(crate) fn slice_growth_call(
    method: &str,
    receiver: GoExpression,
    arguments: &[GoExpression],
) -> GoExpression {
    try_inline_native_method(&NativeTypeKind::Slice, method, &receiver, arguments, false)
        .expect("slice growth has an inline rule")
}

/// `slices.Clip(receiver)`, so a later `append` cannot write through an alias.
pub(crate) fn clip_shared_capacity(receiver: GoExpression) -> GoExpression {
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

fn growth_detach_applies(context: &NativeCallContext, receiver: &Expression) -> bool {
    NativeOperation::new(context.native_type, context.method).grows_slice()
        && !is_fresh_slice_value(receiver)
        && !context
            .retired_receiver
            .is_some_and(|target| retired_covers_receiver(target, receiver))
}

fn is_fresh_slice_value(receiver: &Expression) -> bool {
    match receiver.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Slice(_),
            ..
        } => true,
        Expression::Call {
            expression: callee,
            call_kind,
            ..
        } => match call_kind {
            CallKind::NativeConstructor(NativeTypeKind::Slice) => true,
            CallKind::NativeMethod(NativeTypeKind::Slice)
            | CallKind::NativeMethodIdentifier(NativeTypeKind::Slice) => NativeOperation::new(
                NativeTypeKind::Slice,
                extract_native_method_name(callee.unwrap_parens()),
            )
            .returns_fresh_slice(),
            CallKind::NativeMethod(NativeTypeKind::Array)
            | CallKind::NativeMethodIdentifier(NativeTypeKind::Array) => {
                extract_native_method_name(callee.unwrap_parens()) == "to_slice"
            }
            _ => false,
        },
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
        // A slice type needs no parentheses as a callee, unlike `Conversion`'s general form.
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

/// Try to inline a native-type method call. `negated` asks for the rule's
/// negated form (`None` when the rule has none).
pub(super) fn try_inline_native_method(
    native_type: &NativeTypeKind,
    method: &str,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    negated: bool,
) -> Option<GoExpression> {
    // Go's `append` requires at least 2 args, so zero-arg `append` returns
    // the receiver unchanged.
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

fn array_to_slice_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        call_kind,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    let function = callee.unwrap_parens();
    if spread.is_some()
        || extract_native_method_name(function) != "to_slice"
        || !matches!(
            call_kind,
            CallKind::NativeMethod(NativeTypeKind::Array)
                | CallKind::NativeMethodIdentifier(NativeTypeKind::Array)
        )
    {
        return None;
    }
    match split_native_receiver(function, args)? {
        (receiver, []) => Some(receiver),
        _ => None,
    }
}

/// Whether a rule for `(type, method, arity)` has a negated form.
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
    /// Grow into independent storage even when nothing is added. The receiver
    /// is staged before any argument effects and is safe to read more than once.
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
                // A constant expression can be zero; only a literal has a known value here.
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
        // Keep argument effects visible to operand ordering, outside the IIFE.
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
                let body = self.equality_test(
                    staged.receiver.clone(),
                    staged.arguments[0].clone(),
                    &receiver_ty,
                    &[],
                    false,
                );
                return staged.finish(body);
            }
        }

        if context.method == "contains" && matches!(context.native_type, NativeTypeKind::Slice) {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if receiver_ty.is_slice()
                && let Some(element) = receiver_ty.inner()
            {
                let mut staged = self.stage_native_method(context);
                let body = if self.needs_custom_equality(&element, &[]) {
                    let searched = staged.arguments[0].clone();
                    let target =
                        self.hoist_tmp_value_statement(&mut staged.setup, "want", searched);
                    let predicate =
                        self.contains_predicate(&element, GoExpression::identifier(target), &[]);
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
            && matches!(
                self.facts.strip_and_peel(&receiver_expression.get_type()),
                Type::Array { .. }
            )
        {
            let mut staged = self.stage_native_method(context);
            let index = staged.arguments.first().cloned();
            let body = self.lower_array_method_body(
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
            if is_cloneable_container(&receiver_ty) {
                let staged = self.stage_native_method(context);
                let body = self.clone_expression(staged.receiver.clone(), &receiver_ty);
                return staged.finish(body);
            }
        }

        let mut staged = self.stage_native_method(context);
        if growth_detach_applies(context, receiver_expression) {
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
                native::method_prefix(context.native_type),
                go_name::snake_to_camel(context.method)
            ),
        );
        let type_args = match context.form {
            NativeCallForm::Dot { receiver }
                if !context.resolved_type_args.is_empty() && context.call_ty.is_some() =>
            {
                self.format_type_args_with_receiver(
                    &receiver.get_type(),
                    context.resolved_type_args,
                )
            }
            NativeCallForm::Dot { .. }
            | NativeCallForm::Identifier
            | NativeCallForm::Constructor => {
                self.format_resolved_type_args(context.resolved_type_args)
            }
        };
        let mut emitted_args = vec![staged.receiver.clone()];
        emitted_args.extend(staged.arguments.iter().cloned());
        staged.finish(GoExpression::call(
            GoExpression::instantiation(fn_name, type_args),
            emitted_args,
        ))
    }

    fn lower_array_method_body(
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
        if expression.get_type().is_ref() {
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
                    || (matches!(origin, Expression::Call { .. }) && !base.get_type().is_ref());
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

    /// Returns `None` when the rule has no direct negated form, without staging.
    pub(super) fn try_emit_negated_native_method(
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

    /// Stage `to_slice()` as `arr[:]` when the consumer cannot observe the skipped clone.
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
        let array = array_to_slice_receiver(receiver_expression)?;
        if !matches!(
            self.facts.strip_and_peel(&array.get_type()),
            Type::Array { .. }
        ) {
            return None;
        }
        if matches!(context.method, "contains" | "equals") {
            let element = self
                .facts
                .strip_and_peel(&receiver_expression.get_type())
                .inner()?;
            if self.needs_custom_equality(&element, &[]) {
                return None;
            }
        }
        Some(self.stage_array_view(array))
    }

    fn stage_array_view(&mut self, array: &Expression) -> ValuePlan {
        let mut staged = self.plan_operand(array, ExpressionContext::value());
        if array.get_type().is_ref() {
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
        if is_dot && receiver.get_type().is_ref() {
            let receiver = stages.remove(0).dereference();
            stages.insert(0, receiver);
        }
        if growth_detach_applies(ctx, receiver)
            && (!arguments.is_empty() || ctx.spread.is_some())
            && !is_clip_safe_path(stages[0].expression())
        {
            self.pin_staged(&mut stages[0], "recv");
        }

        let sequenced = self.sequence_with_spread_values(
            stages,
            spread_stage,
            SpreadSequenceOptions {
                wrap_to_any: false,
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

    /// Lower `m.get(k)` to the native comma-ok index expression `m[k]`.
    pub(super) fn lower_map_index_pair(
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
            unreachable!("lower_map_index_pair requires a Call expression");
        };
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");
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
            resolved_type_args,
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
        let is_ref_receiver = receiver_expr.get_type().is_ref();
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
        let range_kind = peel_to_range_type(&arg_ty, |id| self.facts.definition(id))
            .and_then(|ty| RangeShape::of(&ty))
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
        ty: &Type,
        generics: &[Generic],
    ) -> GoExpression {
        self.equality_test(lhs, rhs, ty, generics, false)
    }

    /// `!=` where equality is an operator, a `!` prefix where it is a call.
    pub(crate) fn inequality_expression(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        ty: &Type,
        generics: &[Generic],
    ) -> GoExpression {
        self.equality_test(lhs, rhs, ty, generics, true)
    }

    fn equality_test(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        ty: &Type,
        generics: &[Generic],
        negated: bool,
    ) -> GoExpression {
        let operator = if negated { BinaryOp::Ne } else { BinaryOp::Eq };
        let negate = |call: GoExpression| {
            if negated {
                GoExpression::unary(UnaryOp::Not, call)
            } else {
                call
            }
        };
        let peeled = self.facts.peel_alias(ty);
        if peeled.is_ref() {
            return GoExpression::binary(lhs, operator, rhs);
        }
        if peeled.is_slice() {
            return match peeled.inner() {
                Some(elem) if self.needs_custom_equality(&elem, generics) => {
                    let eq = self.equality_closure(&elem, generics);
                    negate(GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Slices, "EqualFunc"),
                        vec![lhs, rhs, eq],
                    ))
                }
                _ => negate(GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Equal"),
                    vec![lhs, rhs],
                )),
            };
        }
        if peeled.is_map() {
            let value = peeled
                .as_compound()
                .and_then(|(_, args)| args.get(1).cloned());
            return match value {
                Some(value) if self.needs_custom_equality(&value, generics) => {
                    let eq = self.equality_closure(&value, generics);
                    negate(GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Maps, "EqualFunc"),
                        vec![lhs, rhs, eq],
                    ))
                }
                _ => negate(GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Maps, "Equal"),
                    vec![lhs, rhs],
                )),
            };
        }
        if self.type_has_equals(&peeled, generics) {
            let method = self.equals_method_go_name();
            return negate(GoExpression::call(
                GoExpression::selector(lhs, method.to_string()),
                vec![rhs],
            ));
        }
        GoExpression::binary(lhs, operator, rhs)
    }

    fn equality_closure(&mut self, ty: &Type, generics: &[Generic]) -> GoExpression {
        let go_ty = self.go_type(ty);
        let a = self.fresh_local(Some("a"));
        let b = self.fresh_local(Some("b"));
        let body = self.equality_expression(
            GoExpression::identifier(a.clone()),
            GoExpression::identifier(b.clone()),
            ty,
            generics,
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
        ty: &Type,
        target: GoExpression,
        generics: &[Generic],
    ) -> GoExpression {
        let go_ty = self.go_type(ty);
        let element = self.fresh_local(Some("e"));
        let body = self.equality_expression(
            GoExpression::identifier(element.clone()),
            target,
            ty,
            generics,
        );
        predicate_literal(vec![GoParameter::new(element.clone(), go_ty.clone())], body)
    }

    fn needs_custom_equality(&self, ty: &Type, generics: &[Generic]) -> bool {
        self.is_container(ty) || self.type_has_equals(ty, generics)
    }

    fn is_container(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        peeled.is_slice() || peeled.is_map()
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

fn is_cloneable_container(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Compound {
            kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
            ..
        }
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
