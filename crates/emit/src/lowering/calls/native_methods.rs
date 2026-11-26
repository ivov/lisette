use super::{NativeCallContext, NativeCallForm, split_native_receiver};
use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::calls::arguments::converts_for_inference;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::calls::slice_loop::SliceLoopTarget;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::expressions::access::index_access::range_var_bounds;
use crate::lowering::expressions::operand_order::{SequencedArray, SpreadSequenceOptions};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::assignments::lvalues_match;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::output::finish::function_literals::function_literal;
use crate::output::finish::function_literals::immediate_call;
use crate::target::ir::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement};
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::go_expression::{
    FunctionLiteralLayout, GoBinaryOperator, GoParameter, GoUnaryOperator,
};
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::names::go_name;
use crate::target::names::go_name::GeneratedPackage;
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, Literal, UnaryOperator};
use syntax::program::{
    CallOperation, DotAccessResolution, ElementEquality, EqualityRecipe, NativeTypeKind,
};
use syntax::program::{NativeInlineForm, NativeInlinePackage, NativeOperation};
use syntax::types::Type;

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

#[derive(Clone, Copy)]
pub(in crate::lowering) enum SliceGrowth {
    Append,
    Reserve,
}

impl SliceGrowth {
    pub(in crate::lowering) fn of(operation: NativeOperation<'_>) -> Option<Self> {
        match (operation.kind, operation.method) {
            (NativeTypeKind::Slice, "append") => Some(Self::Append),
            (NativeTypeKind::Slice, "reserve") => Some(Self::Reserve),
            _ => None,
        }
    }
}

pub(in crate::lowering) fn slice_growth_call(
    growth: SliceGrowth,
    receiver: GoExpression,
    arguments: &[GoExpression],
) -> GoExpression {
    let callee = match (growth, arguments) {
        (SliceGrowth::Append, []) => return receiver,
        (SliceGrowth::Append, _) => GoExpression::external_name("append".to_string()),
        (SliceGrowth::Reserve, _) => GoExpression::generated(GeneratedPackage::Slices, "Grow"),
    };
    let mut all = vec![receiver];
    all.extend(arguments.iter().cloned());
    GoExpression::call(callee, all)
}

#[derive(Clone, Copy)]
enum ArrayRead {
    ToSlice,
    Get,
}

impl ArrayRead {
    fn of(operation: NativeOperation<'_>) -> Option<Self> {
        match (operation.kind, operation.method) {
            (NativeTypeKind::Array, "to_slice") => Some(Self::ToSlice),
            (NativeTypeKind::Array, "get") => Some(Self::Get),
            _ => None,
        }
    }
}

fn guarded_growth_body(
    growth: SliceGrowth,
    shared: GoExpression,
    receiver: GoExpression,
    clipped: GoExpression,
) -> LoweredBlock {
    let (count, arguments) = match growth {
        SliceGrowth::Append => {
            let count = GoExpression::read_only_call(
                GoExpression::external_name("len".to_string()),
                vec![shared.clone()],
            );
            (count, [GoExpression::spread(shared)])
        }
        SliceGrowth::Reserve => (shared.clone(), [shared]),
    };

    let empty_guard = LoweredStatement::If(IfPlan::plain(
        GoExpression::binary(count, GoBinaryOperator::Equal, GoExpression::integer(0)),
        LoweredBlock {
            statements: vec![plain_return(clone_slice(receiver))],
        },
        ElseArm::None,
    ));
    LoweredBlock {
        statements: vec![
            empty_guard.into(),
            plain_return(slice_growth_call(growth, clipped, &arguments)),
        ],
    }
}

fn clip_shared_capacity(receiver: GoExpression) -> GoExpression {
    GoExpression::read_only_call(
        GoExpression::generated(GeneratedPackage::Slices, "Clip"),
        vec![receiver],
    )
}

pub(in crate::lowering) fn is_clip_safe_path(value: &GoExpression) -> bool {
    fn is_path(node: &GoExpressionNode) -> bool {
        match node {
            GoExpressionNode::Identifier(_) | GoExpressionNode::Qualified { .. } => true,
            GoExpressionNode::Selector { base, .. } => is_path(base),
            GoExpressionNode::Literal(_)
            | GoExpressionNode::Type(_)
            | GoExpressionNode::CompositeLiteral { .. }
            | GoExpressionNode::Call { .. }
            | GoExpressionNode::Instantiation { .. }
            | GoExpressionNode::Index { .. }
            | GoExpressionNode::Slice { .. }
            | GoExpressionNode::TypeAssertion { .. }
            | GoExpressionNode::Unary { .. }
            | GoExpressionNode::AddressOf(_)
            | GoExpressionNode::Dereference(_)
            | GoExpressionNode::Binary { .. }
            | GoExpressionNode::Conversion { .. }
            | GoExpressionNode::Spread(_)
            | GoExpressionNode::FunctionLiteral { .. }
            | GoExpressionNode::Verbatim(_)
            | GoExpressionNode::Unreachable => false,
        }
    }
    is_path(value.node())
}

fn is_fresh_slice_value(operation: Option<&CallOperation>, receiver: &Expression) -> bool {
    if let Expression::Literal {
        literal: Literal::Slice(_),
        ..
    } = receiver
    {
        return true;
    }
    match operation {
        Some(CallOperation::NativeConstructor {
            kind: NativeTypeKind::Slice,
            ..
        }) => true,
        Some(CallOperation::Native { kind, method }) => {
            NativeOperation::new(*kind, method).returns_fresh_slice()
        }
        _ => false,
    }
}

fn assigned_place_covers_receiver(target: &Expression, receiver: &Expression) -> bool {
    let mut current = receiver;
    loop {
        if lvalues_match(target, current) {
            return true;
        }
        let Expression::DotAccess { expression, .. } = current else {
            return false;
        };
        current = expression;
    }
}

fn build_inline(
    native_type: &NativeTypeKind,
    form: NativeInlineForm,
    package: NativeInlinePackage,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    polarity: Polarity,
) -> GoExpression {
    match form {
        NativeInlineForm::Call(callee) => {
            let mut all = vec![receiver.clone()];
            all.extend(arguments.iter().cloned());
            GoExpression::call(inline_callee(package, callee), all)
        }
        NativeInlineForm::IsEmpty => is_empty_test(native_type, receiver, polarity),
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

fn inline_callee(package: NativeInlinePackage, callee: &str) -> GoExpression {
    match package {
        NativeInlinePackage::None => GoExpression::external_name(callee.to_string()),
        NativeInlinePackage::Slices => GoExpression::generated(GeneratedPackage::Slices, callee),
        NativeInlinePackage::Strings => GoExpression::generated(GeneratedPackage::Strings, callee),
        NativeInlinePackage::Prelude => GoExpression::generated(GeneratedPackage::Prelude, callee),
    }
}

fn is_empty_test(
    native_type: &NativeTypeKind,
    receiver: &GoExpression,
    polarity: Polarity,
) -> GoExpression {
    let operator = match polarity {
        Polarity::Negated => GoBinaryOperator::NotEqual,
        Polarity::Positive => GoBinaryOperator::Equal,
    };
    if matches!(native_type, NativeTypeKind::String) {
        return GoExpression::binary(
            receiver.clone(),
            operator,
            GoExpression::token("\"\"".to_string()),
        );
    }
    GoExpression::binary(
        GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![receiver.clone()],
        ),
        operator,
        GoExpression::integer(0),
    )
}

pub(super) fn try_inline_native_method(
    native_type: &NativeTypeKind,
    method: &str,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    polarity: Polarity,
) -> Option<GoExpression> {
    let rule = NativeOperation::new(*native_type, method).inline_rule(arguments.len())?;
    if polarity == Polarity::Negated && !matches!(rule.form, NativeInlineForm::IsEmpty) {
        return None;
    }
    Some(build_inline(
        native_type,
        rule.form,
        rule.package,
        receiver,
        arguments,
        polarity,
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
    } = expression
    else {
        return None;
    };

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
    match split_native_receiver(callee, args)? {
        (receiver, []) => Some(receiver),
        _ => None,
    }
}

fn has_inline_negation(native_type: &NativeTypeKind, method: &str, arity: usize) -> bool {
    NativeOperation::new(*native_type, method)
        .inline_rule(arity)
        .is_some_and(|rule| matches!(rule.form, NativeInlineForm::IsEmpty))
}

struct NativeMethodOperands {
    setup: Vec<Statement>,
    receiver: GoExpression,
    arguments: Vec<GoExpression>,
}

impl NativeMethodOperands {
    fn finish(self, value: GoExpression) -> NativeCallResult {
        NativeCallResult::new(self.setup, value)
    }
}

struct SubstringOperands {
    setup: Vec<Statement>,
    receiver: GoExpression,
    start: Option<GoExpression>,
    end: Option<GoExpression>,
}

impl SubstringOperands {
    fn new(
        setup: Vec<Statement>,
        receiver: GoExpression,
        start: Option<GoExpression>,
        end: Option<GoExpression>,
    ) -> Self {
        Self {
            setup,
            receiver,
            start,
            end,
        }
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn detach_slice_growth(
        &mut self,
        setup: &mut Vec<Statement>,
        growth: SliceGrowth,
        receiver: GoExpression,
        arguments: &[GoExpression],
        result_ty: &Type,
    ) -> GoExpression {
        let clipped = clip_shared_capacity(receiver.clone());
        let shared = match growth {
            SliceGrowth::Append => {
                let Some(last) = arguments.last() else {
                    return clone_slice(receiver);
                };
                let GoExpressionNode::Spread(spread) = last.node() else {
                    return slice_growth_call(growth, clipped, arguments);
                };
                last.peeled(spread)
            }
            SliceGrowth::Reserve => {
                let count = &arguments[0];
                if let Some(value) = count.as_integer() {
                    return if value == 0 {
                        clone_slice(receiver)
                    } else {
                        slice_growth_call(growth, clipped, arguments)
                    };
                }
                count.clone()
            }
        };

        let shared = self.stable_source(setup, "added", shared);
        self.with_scope(|this| {
            immediate_call(
                Some(this.queries().go_type(result_ty)),
                guarded_growth_body(growth, shared, receiver, clipped),
                FunctionLiteralLayout::MultiLine,
            )
        })
    }

    pub(super) fn lower_native_method(
        &mut self,
        context: &NativeCallContext,
        receiver: &Expression,
        arguments: &[Expression],
    ) -> NativeCallResult {
        let mut result = self.lower_selected_native_method(context, receiver, arguments);

        let operation = NativeOperation::new(context.native_type, context.method);
        if operation.read_only_without_callbacks() {
            result.value = result.value.into_read_only_call();
        }
        result
    }

    fn lower_selected_native_method(
        &mut self,
        context: &NativeCallContext,
        receiver_expression: &Expression,
        arguments: &[Expression],
    ) -> NativeCallResult {
        if matches!(context.native_type, NativeTypeKind::String)
            && context.method == "substring"
            && let Some(range) = arguments.first()
        {
            return self.lower_string_substring(
                receiver_expression,
                range,
                context.capture_boundary,
            );
        }

        if let Some(result) =
            self.try_lower_native_equality(context, receiver_expression, arguments)
        {
            return result;
        }

        if let Some(read) = ArrayRead::of(NativeOperation::new(context.native_type, context.method))
        {
            let mut operand =
                self.plan_native_method_operands(context, receiver_expression, arguments);
            let index = operand.arguments.first().cloned();
            let body = self.plan_array_method_body(
                read,
                receiver_expression,
                operand.receiver.clone(),
                index,
                &mut operand.setup,
            );

            return operand.finish(body);
        }

        if matches!(context.native_type, NativeTypeKind::Slice)
            && let Some(result) = self.try_lower_slice_loop(context, SliceLoopTarget::Fresh)
        {
            return result;
        }

        if context.method == "clone"
            && let Some(receiver_ty) = self.facts.clone_container(&receiver_expression.get_type())
        {
            let operand = self.plan_native_method_operands(context, receiver_expression, arguments);
            let body = self.clone_expression(operand.receiver.clone(), &receiver_ty);
            return operand.finish(body);
        }

        self.lower_native_method_from_operands(context, receiver_expression, arguments)
    }

    fn lower_native_method_from_operands(
        &mut self,
        context: &NativeCallContext,
        receiver_expression: &Expression,
        arguments: &[Expression],
    ) -> NativeCallResult {
        let mut operand = self.plan_native_method_operands(context, receiver_expression, arguments);
        if let Some(growth) =
            SliceGrowth::of(NativeOperation::new(context.native_type, context.method))
            && growth_detach_applies(&self.facts, context, receiver_expression)
        {
            let detached = self.detach_slice_growth(
                &mut operand.setup,
                growth,
                operand.receiver.clone(),
                &operand.arguments,
                &receiver_expression.get_type(),
            );

            return operand.finish(detached);
        }

        if let Some(inlined) = try_inline_native_method(
            &context.native_type,
            context.method,
            &operand.receiver,
            &operand.arguments,
            Polarity::Positive,
        ) {
            return operand.finish(inlined);
        }

        self.queries()
            .lower_prelude_native_call(context, arguments, operand)
    }

    fn try_lower_native_equality(
        &mut self,
        context: &NativeCallContext,
        receiver: &Expression,
        arguments: &[Expression],
    ) -> Option<NativeCallResult> {
        if context.method == "equals"
            && matches!(
                context.native_type,
                NativeTypeKind::Slice | NativeTypeKind::Map
            )
        {
            let operand = self.plan_native_method_operands(context, receiver, arguments);
            let recipe = native_equality(&self.facts, context);
            let body = self.equality_test(
                operand.receiver.clone(),
                operand.arguments[0].clone(),
                &recipe,
                GoBinaryOperator::Equal,
            );

            return Some(operand.finish(body));
        }

        if context.method == "contains" && matches!(context.native_type, NativeTypeKind::Slice) {
            let mut operand = self.plan_native_method_operands(context, receiver, arguments);
            let body = if let EqualityRecipe::Slice(Some(element)) =
                &native_equality(&self.facts, context)
            {
                let searched = operand.arguments[0].clone();
                let target = self.hoist_temp(&mut operand.setup, "want", searched);
                let predicate = self.contains_predicate(element, GoExpression::identifier(target));
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "ContainsFunc"),
                    vec![operand.receiver.clone(), predicate],
                )
            } else {
                GoExpression::read_only_call(
                    GoExpression::generated(GeneratedPackage::Slices, "Contains"),
                    vec![operand.receiver.clone(), operand.arguments[0].clone()],
                )
            };

            return Some(operand.finish(body));
        }
        None
    }

    fn plan_array_method_body(
        &mut self,
        read: ArrayRead,
        receiver_expression: &Expression,
        receiver: GoExpression,
        index: Option<GoExpression>,
        setup: &mut Vec<Statement>,
    ) -> GoExpression {
        match read {
            ArrayRead::ToSlice => {
                let view = self.sliceable_receiver(receiver_expression, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Clone"),
                    vec![view],
                )
            }
            ArrayRead::Get => {
                let view = self.sliceable_receiver(receiver_expression, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Prelude, "SliceGet"),
                    vec![view, index.expect("get needs an index")],
                )
            }
        }
    }

    fn sliceable_receiver(
        &mut self,
        expression: &Expression,
        receiver: GoExpression,
        setup: &mut Vec<Statement>,
    ) -> GoExpression {
        let base = if self.queries().receiver_is_addressable(expression) {
            receiver
        } else {
            GoExpression::identifier(self.hoist_temp(setup, "arr", receiver))
        };
        GoExpression::slice(base, None, None, None)
    }

    pub(super) fn try_plan_negated_native_method(
        &mut self,
        setup: &mut Vec<Statement>,
        ctx: &NativeCallContext,
    ) -> Option<GoExpression> {
        let (receiver, arguments) = ctx.receiver_and_arguments()?;
        if !has_inline_negation(&ctx.native_type, ctx.method, arguments.len()) {
            return None;
        }
        let operand = self.plan_native_method_operands(ctx, receiver, arguments);
        let inlined = try_inline_native_method(
            &ctx.native_type,
            ctx.method,
            &operand.receiver,
            &operand.arguments,
            Polarity::Negated,
        )?;

        setup.extend(operand.setup);
        Some(inlined)
    }

    fn try_plan_to_slice_view(
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
        if matches!(context.method, "contains" | "equals")
            && matches!(
                native_equality(&self.facts, context),
                EqualityRecipe::Slice(Some(_))
            )
        {
            return None;
        }
        Some(self.plan_array_view(array))
    }

    fn plan_array_view(&mut self, array: &Expression) -> ValuePlan {
        let mut operand = self.plan_operand(array, ExpressionContext::value());
        if self.facts.is_reference(&array.get_type()) {
            operand = operand.dereference();
        }
        if !self.queries().receiver_is_addressable(array) {
            self.pin_to_temp(&mut operand, "arr");
        }
        operand.map_observable_expression(|_setup, array| {
            let base = array;
            GoExpression::slice(base, None, None, None)
        })
    }

    fn plan_native_method_operands(
        &mut self,
        ctx: &NativeCallContext,
        receiver: &Expression,
        arguments: &[Expression],
    ) -> NativeMethodOperands {
        let is_dot = matches!(ctx.form, NativeCallForm::Dot { .. });
        let view = self.try_plan_to_slice_view(ctx, receiver, arguments);
        let mut operands = match view {
            None if is_dot => vec![self.plan_operand(receiver, ExpressionContext::value())],
            None => Vec::new(),
            Some(view) => vec![view],
        };
        let first_unplanned = if is_dot { 0 } else { operands.len() };
        operands.extend(self.plan_native_method_args_from(&ctx.abi, ctx.args, first_unplanned));
        let spread_operand = ctx.spread.map(|spread| {
            if NativeOperation::new(ctx.native_type, ctx.method).grows_slice() {
                self.plan_copied_spread(spread)
            } else {
                self.plan_operand(spread, ExpressionContext::value())
            }
        });

        if is_dot && self.facts.is_reference(&receiver.get_type()) {
            let receiver = operands.remove(0).dereference();
            operands.insert(0, receiver);
        }
        if growth_detach_applies(&self.facts, ctx, receiver)
            && (!arguments.is_empty() || ctx.spread.is_some())
            && !operands[0].expression().is_some_and(is_clip_safe_path)
        {
            self.pin_to_temp(&mut operands[0], "recv");
        }

        let sequenced = self.sequence_with_spread_values(
            operands,
            spread_operand,
            SpreadSequenceOptions {
                wrap_to_any: ctx.abi.spread_wraps_to_any(&self.facts, ctx.spread),
                combine: ctx.abi.variadic_combine(usize::from(is_dot)),
                boundary: ctx.capture_boundary,
            },
        );

        let mut values = sequenced.values;
        let receiver = values.remove(0);
        NativeMethodOperands {
            setup: sequenced.setup,
            receiver,
            arguments: values,
        }
    }

    pub(super) fn plan_map_index_pair(
        &mut self,
        site: CallSite<'_>,
        receiver: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let plan = self.queries().plan_call(site);
        let ctx = NativeCallContext {
            form: NativeCallForm::Dot { receiver },
            spread: None,
            ..NativeCallContext::new(site, plan.resolved.abi, NativeTypeKind::Map, "get")
        };

        let mut operand = self.plan_native_method_operands(&ctx, receiver, site.args);
        let receiver = operand.receiver;
        let key = operand.arguments.remove(0);
        (operand.setup, GoExpression::index(receiver, key))
    }

    fn lower_string_substring(
        &mut self,
        receiver_expression: &Expression,
        range: &Expression,
        capture_boundary: CaptureBoundary,
    ) -> NativeCallResult {
        let operands = if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = range
        {
            let mut operands = self.sequence_range_literal_substring(
                receiver_expression,
                start.as_deref(),
                end.as_deref(),
                capture_boundary,
            );
            if *inclusive {
                operands.end = operands.end.map(|end| {
                    GoExpression::binary(end, GoBinaryOperator::Addition, GoExpression::integer(1))
                });
            }
            operands
        } else {
            self.sequence_range_value_substring(receiver_expression, range, capture_boundary)
        };

        let receiver = if self.facts.is_reference(&receiver_expression.get_type()) {
            GoExpression::dereference(operands.receiver)
        } else {
            operands.receiver
        };
        NativeCallResult::new(
            operands.setup,
            substring_call(receiver, operands.start, operands.end),
        )
    }

    fn sequence_range_literal_substring(
        &mut self,
        receiver_expression: &Expression,
        start: Option<&Expression>,
        end: Option<&Expression>,
        capture_boundary: CaptureBoundary,
    ) -> SubstringOperands {
        let receiver = self.plan_operand(receiver_expression, ExpressionContext::value());
        let start = start.map(|start| self.plan_operand(start, ExpressionContext::value()));
        let end = end.map(|end| self.plan_operand(end, ExpressionContext::value()));
        let boundary = capture_boundary;
        match (start, end) {
            (Some(start), Some(end)) => {
                let sequenced = self.sequence_value_array([receiver, start, end], boundary, "arg");
                let [receiver, start, end] = sequenced.values;
                SubstringOperands::new(sequenced.setup, receiver, Some(start), Some(end))
            }
            (Some(start), None) => {
                let sequenced = self.sequence_value_array([receiver, start], boundary, "arg");
                let [receiver, start] = sequenced.values;
                SubstringOperands::new(sequenced.setup, receiver, Some(start), None)
            }
            (None, Some(end)) => {
                let sequenced = self.sequence_value_array([receiver, end], boundary, "arg");
                let [receiver, end] = sequenced.values;
                SubstringOperands::new(sequenced.setup, receiver, None, Some(end))
            }
            (None, None) => {
                let sequenced = self.sequence_value_array([receiver], boundary, "arg");
                let [receiver] = sequenced.values;
                SubstringOperands::new(sequenced.setup, receiver, None, None)
            }
        }
    }

    fn sequence_range_value_substring(
        &mut self,
        receiver_expression: &Expression,
        range: &Expression,
        capture_boundary: CaptureBoundary,
    ) -> SubstringOperands {
        let range_kind = self
            .facts
            .checked(range)
            .range_kind()
            .expect("substring arg should resolve to a known range type");
        let receiver_operand = self.plan_operand(receiver_expression, ExpressionContext::value());
        let range_operand = self.plan_or_capture(range, "range");
        let SequencedArray {
            setup,
            values: [receiver, range],
            ..
        } = self.sequence_value_array([receiver_operand, range_operand], capture_boundary, "arg");
        let (start, end) = range_var_bounds(&range, range_kind);
        SubstringOperands {
            setup,
            receiver,
            start,
            end,
        }
    }

    pub(in crate::lowering) fn equality_test(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        recipe: &EqualityRecipe,
        operator: GoBinaryOperator,
    ) -> GoExpression {
        let call = match recipe {
            EqualityRecipe::Operator => return GoExpression::binary(lhs, operator, rhs),
            EqualityRecipe::Method { .. } => GoExpression::call(
                GoExpression::selector(lhs, self.facts.equals_method_go_name()),
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

        if operator == GoBinaryOperator::NotEqual {
            GoExpression::unary(GoUnaryOperator::Not, call)
        } else {
            call
        }
    }

    fn equality_closure(&mut self, element: &ElementEquality) -> GoExpression {
        let go_ty = self.queries().go_type(&element.ty);
        let a = self.fresh_local("a");
        let b = self.fresh_local("b");
        let body = self.equality_test(
            GoExpression::identifier(a.clone()),
            GoExpression::identifier(b.clone()),
            &element.recipe,
            GoBinaryOperator::Equal,
        );

        predicate_literal(
            vec![
                GoParameter::new(a, go_ty.clone()),
                GoParameter::new(b, go_ty),
            ],
            body,
        )
    }

    fn contains_predicate(
        &mut self,
        element: &ElementEquality,
        target: GoExpression,
    ) -> GoExpression {
        let go_ty = self.queries().go_type(&element.ty);
        let candidate = self.fresh_local("e");
        let body = self.equality_test(
            GoExpression::identifier(candidate.clone()),
            target,
            &element.recipe,
            GoBinaryOperator::Equal,
        );

        predicate_literal(vec![GoParameter::new(candidate, go_ty)], body)
    }
}

fn predicate_literal(parameters: Vec<GoParameter>, body: GoExpression) -> GoExpression {
    function_literal(
        parameters,
        Some(GoType::new("bool")),
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
        NativeTypeKind::Slice
        | NativeTypeKind::EnumeratedSlice
        | NativeTypeKind::Map
        | NativeTypeKind::Channel
        | NativeTypeKind::Sender
        | NativeTypeKind::Receiver
        | NativeTypeKind::Array => kind.name(),
    }
}

fn native_equality(facts: &EmitFacts<'_>, context: &NativeCallContext<'_>) -> EqualityRecipe {
    facts
        .checked(context.function)
        .equality()
        .expect("internal error: a built-in comparison has a selected equality")
        .clone()
}

fn growth_detach_applies(
    facts: &EmitFacts<'_>,
    context: &NativeCallContext,
    receiver: &Expression,
) -> bool {
    facts.checked(context.function).callee().copies_storage
        && !is_fresh_slice_value(facts.call_operation(receiver), receiver)
        && !context
            .assigned_place
            .is_some_and(|target| assigned_place_covers_receiver(target, receiver))
}

impl PlannerQueries<'_, '_> {
    fn lower_prelude_native_call(
        &self,
        context: &NativeCallContext,
        argument_expressions: &[Expression],
        operand: NativeMethodOperands,
    ) -> NativeCallResult {
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

        let mut emitted_args = vec![operand.receiver.clone()];
        let first_param = usize::from(!matches!(context.form, NativeCallForm::Dot { .. }));
        emitted_args.extend(operand.arguments.iter().enumerate().map(|(index, value)| {
            let parameter = context.abi.param(first_param + index);
            match argument_expressions.get(index).zip(parameter) {
                Some((argument, parameter))
                    if converts_for_inference(self.facts, argument, parameter, |_| {
                        type_args.is_some()
                    }) =>
                {
                    GoExpression::conversion(self.go_type(&parameter.instantiated), value.clone())
                }
                _ => value.clone(),
            }
        }));
        operand.finish(GoExpression::call(
            GoExpression::instantiation(fn_name, type_args),
            emitted_args,
        ))
    }

    #[allow(clippy::wildcard_enum_match_arm)]
    pub(in crate::lowering) fn receiver_is_addressable(&self, expression: &Expression) -> bool {
        if self.facts.is_reference(&expression.get_type()) {
            return true;
        }
        match expression {
            Expression::Identifier { .. }
            | Expression::Unary {
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
                let fresh_value = matches!(&**base, Expression::StructCall { .. })
                    || (matches!(&**base, Expression::Call { .. })
                        && !self.facts.is_reference(&base.get_type()));
                !fresh_value && self.receiver_is_addressable(base)
            }
            Expression::IndexedAccess {
                expression: base, ..
            } => match self.facts.native_kind(&base.get_type()) {
                Some(NativeTypeKind::Map) => false,
                Some(NativeTypeKind::Slice) => true,
                Some(
                    NativeTypeKind::EnumeratedSlice
                    | NativeTypeKind::Channel
                    | NativeTypeKind::Sender
                    | NativeTypeKind::Receiver
                    | NativeTypeKind::String
                    | NativeTypeKind::Array,
                )
                | None => self.receiver_is_addressable(base),
            },
            _ => false,
        }
    }
}
