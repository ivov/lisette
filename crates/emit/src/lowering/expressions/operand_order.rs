use crate::Planner;
use crate::lowering::abi::callable::CallableAbi;
use crate::lowering::calls::call_plan::{CallableOrigin, FunctionLayoutChange};
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::{SequencedValues, ValuePlan};
use crate::output::finish::verify::verify_operand_order;
use crate::target::ir::bodies::direct_first;
use crate::target::ir::bodies::{Definition, LoweredStatement, Statement};
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::CompositeLayout;
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::go_expression::Position;
use crate::target::ir::prelude::prelude_constructor;
use crate::target::ir::values::{CaptureBoundary, GoExpression, Stability};
use crate::target::names::go_name::{GeneratedPackage, MemberSpelling};
use crate::target::types::go_type::GoType;
use syntax::ast::{Expression, UnaryOperator};
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone)]
pub(in crate::lowering) struct VariadicCombine {
    pub element_ty: Type,
    pub fixed_count: usize,
}

pub(in crate::lowering) struct SpreadSequenceOptions {
    pub(in crate::lowering) wrap_to_any: bool,
    pub(in crate::lowering) combine: Option<VariadicCombine>,
    pub(in crate::lowering) boundary: CaptureBoundary,
}

#[derive(Default)]
pub(in crate::lowering) struct LaterOperands {
    before: Effects,
    inline: Effects,
}

pub(in crate::lowering) struct OrderedOperand {
    pub(in crate::lowering) plan: ValuePlan,
    pub(in crate::lowering) prefix: &'static str,
    pub(in crate::lowering) capture: bool,
    pub(in crate::lowering) line: Option<Position>,
}

impl LaterOperands {
    pub(in crate::lowering) fn sequenced(setup: &[Statement], inline: Effects) -> Self {
        Self {
            before: setup_effects(setup),
            inline,
        }
    }

    pub(in crate::lowering) fn can_change(&self, stability: Stability) -> bool {
        !self.can_follow(Effects::read_of(stability))
    }

    fn can_follow(&self, effects: Effects) -> bool {
        effects.can_move_across(self.before)
            && effects.without_go_order().can_move_across(self.inline)
    }

    pub(in crate::lowering) fn prepend(&mut self, operand: &ValuePlan) -> bool {
        let effects = operand.effects();
        if !self.can_follow(effects) {
            self.prepend_pinned(operand);
            return true;
        }
        self.before = self.before.union(setup_effects(operand.setup()));
        self.inline = self.inline.union(effects);
        false
    }

    pub(in crate::lowering) fn prepend_pinned(&mut self, operand: &ValuePlan) {
        self.before = self
            .before
            .union(setup_effects(operand.setup()))
            .union(operand.effects());
    }
}

pub(in crate::lowering) struct SequencedArray<const N: usize> {
    pub(in crate::lowering) setup: Vec<Statement>,
    pub(in crate::lowering) values: [GoExpression; N],
    pub(in crate::lowering) stability: Stability,
}

struct OperandPlacement {
    setup: Vec<Statement>,
    stability: Stability,
    eager: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperandPin {
    Inline,
    Pinned,
}

impl OperandPlacement {
    fn new(boundary: CaptureBoundary) -> Self {
        Self {
            setup: Vec::new(),
            stability: Stability::Fixed,
            eager: boundary.delays_reads(),
        }
    }
}

fn mark_pins<'a>(
    operands: impl DoubleEndedIterator<Item = &'a mut (OrderedOperand, OperandPin)>,
    boundary: CaptureBoundary,
) {
    let eager = boundary.delays_reads();
    let mut later = LaterOperands::default();
    for (operand, pin) in operands.rev() {
        let pinned = if operand.capture || (eager && operand.plan.must_evaluate_now()) {
            later.prepend_pinned(&operand.plan);
            true
        } else {
            later.prepend(&operand.plan)
        };
        if pinned {
            *pin = OperandPin::Pinned;
        }
    }
}

fn check_operand_order(values: &[GoExpression]) {
    if cfg!(debug_assertions) {
        verify_operand_order(values).unwrap_or_else(|error| panic!("{error}"));
    }
}

fn setup_effects(setup: &[Statement]) -> Effects {
    setup
        .iter()
        .try_fold(Effects::default(), |effects, statement| {
            match &statement.kind {
                LoweredStatement::Define(Definition { value, .. }) => {
                    let value = value.effects();
                    let reads = if value.reads() == Stability::Fixed {
                        Stability::Fixed
                    } else {
                        Stability::Observable
                    };
                    Some(effects.union(value).union(Effects::read_of(reads)))
                }
                LoweredStatement::If(_)
                | LoweredStatement::Loop(_)
                | LoweredStatement::Block(_)
                | LoweredStatement::Body(_)
                | LoweredStatement::Break(_)
                | LoweredStatement::Continue(_)
                | LoweredStatement::Const(_)
                | LoweredStatement::Return(_)
                | LoweredStatement::Assign(_)
                | LoweredStatement::Async { .. }
                | LoweredStatement::Select(_)
                | LoweredStatement::Switch(_)
                | LoweredStatement::VarDeclaration { .. }
                | LoweredStatement::Discard { .. }
                | LoweredStatement::ExpressionStatement { .. }
                | LoweredStatement::UnreachablePanic => None,
            }
        })
        .unwrap_or_else(Effects::anything)
}

pub(in crate::lowering) fn takes_prelude_callback(declared_param: Option<&Type>) -> bool {
    declared_param.is_some_and(|p| matches!(p.unwrap_forall(), Type::Function(_)))
}

fn prelude_arg_context<'a>(
    declared_param: Option<&Type>,
    param_ty: Option<&'a Type>,
) -> (bool, ExpressionContext<'a>) {
    let suppress = takes_prelude_callback(declared_param);
    let spelling = if suppress {
        MemberSpelling::TaggedCounterpart
    } else {
        MemberSpelling::Plain
    };
    let arg_ctx = ExpressionContext::value()
        .with_go_function_spelling(spelling)
        .with_generic_result_slot(declared_param, param_ty);
    (suppress, arg_ctx)
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_or_capture(
        &mut self,
        expression: &Expression,
        prefix: &'static str,
    ) -> ValuePlan {
        if matches!(
            expression,
            Expression::Literal { .. } | Expression::Identifier { .. }
        ) {
            return self.plan_operand(expression, ExpressionContext::value());
        }

        let operand = self.plan_operand(expression, ExpressionContext::value());
        let (mut setup, value) = operand.into_parts();
        let temp_var = self.hoist_temp(&mut setup, prefix, value);
        ValuePlan::captured(setup, temp_var)
    }

    pub(in crate::lowering) fn pin_to_temp(
        &mut self,
        operand: &mut ValuePlan,
        prefix: &'static str,
    ) {
        operand.pin(|setup, value| self.hoist_temp(setup, prefix, value));
    }

    pub(in crate::lowering) fn eager_operand(
        &mut self,
        mut value: ValuePlan,
        prefix: &'static str,
    ) -> ValuePlan {
        if value.must_evaluate_now()
            && value
                .expression()
                .and_then(GoExpression::constant_kind)
                .is_none()
            && !self.queries().plan_rests_in_stable_name(&value)
        {
            self.pin_to_temp(&mut value, prefix);
        }
        value
    }

    pub(in crate::lowering) fn capture_value_at_boundary(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        prefix: &'static str,
        boundary: CaptureBoundary,
    ) -> GoExpression {
        let plan = self.plan_composite_value(expression, ExpressionContext::value());
        let requires_capture = boundary.delays_reads() && plan.must_evaluate_now();
        let (value_setup, value) = plan.into_parts();
        setup.extend(value_setup);
        if requires_capture {
            GoExpression::identifier(self.hoist_temp(setup, prefix, value))
        } else {
            value
        }
    }

    pub(in crate::lowering) fn plan_prelude_arg(
        &mut self,
        expression: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> ValuePlan {
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        let operand = self.plan_composite_value(expression, arg_ctx);

        if suppress
            && let Some(adapter) = self
                .queries()
                .detect_lower_arg_to_tagged(expression, param_ty)
        {
            return operand.map_expression(|setup, value| {
                self.plan_function_layout_change(setup, value, &adapter.source, &adapter.target)
            });
        }

        self.convert_to_recorded_slot(expression, operand)
    }

    pub(in crate::lowering) fn plan_native_method_args_from(
        &mut self,
        abi: &CallableAbi,
        args: &[Expression],
        start_index: usize,
    ) -> Vec<ValuePlan> {
        args.iter()
            .enumerate()
            .skip(start_index)
            .map(|(i, arg)| {
                let param = abi.param(i);
                self.plan_prelude_arg(
                    arg,
                    param.and_then(|param| param.declared.as_ref()),
                    param.map(|param| &param.instantiated),
                )
            })
            .collect()
    }

    pub(in crate::lowering) fn sequence_values(
        &mut self,
        operands: Vec<ValuePlan>,
        boundary: CaptureBoundary,
        prefix: &'static str,
    ) -> SequencedValues {
        self.sequence_operands(
            operands
                .into_iter()
                .map(|plan| OrderedOperand {
                    plan,
                    prefix,
                    capture: false,
                    line: None,
                })
                .collect(),
            boundary,
        )
    }

    pub(in crate::lowering) fn sequence_value_array<const N: usize>(
        &mut self,
        operands: [ValuePlan; N],
        boundary: CaptureBoundary,
        prefix: &'static str,
    ) -> SequencedArray<N> {
        let mut marked = operands.map(|plan| {
            let operand = OrderedOperand {
                plan,
                prefix,
                capture: false,
                line: None,
            };
            (operand, OperandPin::Inline)
        });
        mark_pins(marked.iter_mut(), boundary);
        let mut placement = OperandPlacement::new(boundary);
        let values =
            marked.map(|(operand, pin)| self.sequence_operand(&mut placement, operand, pin));
        check_operand_order(&values);
        SequencedArray {
            setup: placement.setup,
            values,
            stability: placement.stability,
        }
    }

    pub(in crate::lowering) fn sequence_operands(
        &mut self,
        operands: Vec<OrderedOperand>,
        boundary: CaptureBoundary,
    ) -> SequencedValues {
        let mut marked: Vec<(OrderedOperand, OperandPin)> = operands
            .into_iter()
            .map(|operand| (operand, OperandPin::Inline))
            .collect();
        mark_pins(marked.iter_mut(), boundary);
        let mut placement = OperandPlacement::new(boundary);
        let values: Vec<GoExpression> = marked
            .into_iter()
            .map(|(operand, pin)| self.sequence_operand(&mut placement, operand, pin))
            .collect();
        check_operand_order(&values);
        SequencedValues {
            setup: placement.setup,
            values,
            stability: placement.stability,
        }
    }

    fn sequence_operand(
        &mut self,
        placement: &mut OperandPlacement,
        operand: OrderedOperand,
        pin: OperandPin,
    ) -> GoExpression {
        let OrderedOperand {
            plan,
            prefix,
            capture: _,
            line,
        } = operand;

        let setup = &mut placement.setup;
        let start = setup.len();
        let can_delay = plan.can_delay();
        let (operand_setup, expression, operand_stability) = plan.into_parts_with_stability();
        setup.extend(operand_setup);
        let value = if pin == OperandPin::Pinned || (placement.eager && !can_delay) {
            GoExpression::identifier(self.hoist_temp(setup, prefix, expression))
        } else {
            placement.stability = placement.stability.max(operand_stability);
            expression
        };
        direct_first(&mut placement.setup[start..], line);
        value
    }

    pub(in crate::lowering) fn sequence_with_spread_values(
        &mut self,
        mut operands: Vec<ValuePlan>,
        spread_operand: Option<ValuePlan>,
        options: SpreadSequenceOptions,
    ) -> SequencedValues {
        let has_spread = spread_operand.is_some();
        operands.extend(spread_operand);
        let mut sequenced = self.sequence_values(operands, options.boundary, "arg");
        if has_spread {
            self.queries().finalize_spread_operand(
                &mut sequenced.values,
                options.wrap_to_any,
                options.combine,
            );
        }
        sequenced
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn callee_lowers_to_type_construction(
        &self,
        callee: &Expression,
    ) -> bool {
        self.resolve_callee_definition(callee)
            .1
            .is_some_and(|definition| definition.is_type_definition())
    }

    pub(in crate::lowering) fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        prelude_constructor(callee).is_some() || self.callee_lowers_to_type_construction(callee)
    }

    pub(in crate::lowering) fn plan_rests_in_stable_name(&self, plan: &ValuePlan) -> bool {
        if plan.rests_in_fixed_name() {
            return true;
        }
        plan.expression()
            .and_then(GoExpression::as_identifier)
            .is_some_and(|local| {
                plan.binds_local(local)
                    && local
                        .id()
                        .is_some_and(|id| self.scope.lacks_binding_for_local(id))
            })
    }

    pub(in crate::lowering) fn path_read_stability(&self, path: &GoExpression) -> Stability {
        if path.effects().reads() == Stability::Observable {
            return Stability::Observable;
        }
        let mut stability = Stability::Fixed;
        path.node().visit(&mut |node| {
            if let GoExpressionNode::Identifier(name) = node {
                stability = stability.max(self.scope.local_read_stability(self.facts, name));
            }
        });
        stability
    }

    pub(in crate::lowering) fn place_read_stability(&self, place: &Expression) -> Stability {
        match place {
            Expression::Identifier {
                value, resolution, ..
            } => self
                .scope
                .identifier_read_stability(self.facts, value, resolution),
            Expression::DotAccess {
                expression,
                resolution,
                ..
            } if resolution.is_field_read()
                && !self.facts.reads_value_member(
                    resolution,
                    expression,
                    &expression.get_type(),
                ) =>
            {
                Stability::Observable
            }
            Expression::DotAccess { expression, .. } => self.place_read_stability(expression),
            Expression::IndexedAccess { .. }
            | Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => Stability::Observable,
            Expression::Literal { .. }
            | Expression::Lambda { .. }
            | Expression::Block { .. }
            | Expression::Let { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Tuple { .. }
            | Expression::StructCall { .. }
            | Expression::Assignment { .. }
            | Expression::Return { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
            | Expression::Binary { .. }
            | Expression::Unary { .. }
            | Expression::Paren { .. }
            | Expression::RawGo { .. }
            | Expression::Loop { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::Reference { .. }
            | Expression::Task { .. }
            | Expression::Defer { .. }
            | Expression::Assert { .. }
            | Expression::Select { .. }
            | Expression::Unit { .. }
            | Expression::Range { .. }
            | Expression::Cast { .. }
            | declaration_expression!() => Stability::StableAcrossCalls,
        }
    }

    pub(in crate::lowering) fn prelude_callback_operands_as_is(
        &self,
        callback: &Expression,
        declared_param: Option<&Type>,
        param_ty: Option<&Type>,
    ) -> bool {
        let (suppress, arg_ctx) = prelude_arg_context(declared_param, param_ty);
        suppress
            && self
                .never_result_layout_change(&callback.get_type(), arg_ctx)
                .is_none()
            && (!self.is_go_callable(callback)
                || self
                    .function_value_layout_change(callback, arg_ctx)
                    .is_none())
            && self
                .detect_lower_arg_to_tagged(callback, param_ty)
                .is_none()
    }

    pub(in crate::lowering) fn detect_lower_arg_to_tagged(
        &self,
        arg: &Expression,
        param_ty: Option<&Type>,
    ) -> Option<FunctionLayoutChange> {
        if matches!(arg, Expression::Lambda { .. }) {
            return None;
        }
        let callee = self.resolve_callable_value(arg)?;
        if matches!(callee.origin, CallableOrigin::GoInterop) {
            return None;
        }
        if !callee.abi.result.is_lowered() {
            return None;
        }
        self.callable_slot_layout_change(
            callee.function_layout(self.facts),
            param_ty?.unwrap_forall(),
            None,
            SlotOrigin::Lisette,
        )
    }

    fn finalize_spread_operand(
        &self,
        values: &mut Vec<GoExpression>,
        wrap_to_any: bool,
        combine: Option<VariadicCombine>,
    ) {
        let mut spread = values.pop().expect("a spread argument is always last");
        if wrap_to_any {
            spread = GoExpression::call(
                GoExpression::generated(GeneratedPackage::Prelude, "SliceToAny"),
                vec![spread],
            );
        }
        if let Some(combine) = combine
            && values.len() > combine.fixed_count
        {
            let element_go = self.go_type(&combine.element_ty);
            let leading = GoExpression::composite(
                Some(GoType::slice_of(&element_go)),
                values
                    .drain(combine.fixed_count..)
                    .map(|value| (None, value))
                    .collect(),
                CompositeLayout::Inline { padded: false },
            );

            spread = GoExpression::call(
                GoExpression::external_name("append".to_string()),
                vec![leading, GoExpression::spread(spread)],
            );
        }
        values.push(GoExpression::spread(spread));
    }
}
