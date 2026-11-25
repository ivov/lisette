use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::inline_uses::region_blocks_inline;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::native_methods::{SliceGrowth, is_clip_safe_path, slice_growth_call};
use crate::lowering::control_flow::branches::Branching;
use crate::lowering::expressions::operand_order::{LaterOperands, SpreadSequenceOptions};
use crate::lowering::expressions::tuple_literal::TupleLiteral;
use crate::lowering::expressions::values::TuplePosition;
use crate::lowering::expressions::values::contains_newtype_access;
use crate::lowering::patterns::binding_declarations::pattern_binds_name;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::assignments::PlaceOrdering;
use crate::lowering::statements::assignments::is_lvalue_chain;
use crate::lowering::statements::blocks::{BlockTail, TailTarget};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::with_setup;
use crate::target::ir::bodies::{
    AssignForm, Definition, ElseArm, LoopHeader, LoweredBlock, LoweredStatement, Statement, assign,
    directed_first, discard, effect_statement,
};
use crate::target::ir::evaluation::Effects;
use crate::target::ir::go_expression::{GoBinaryOperator, GoExpressionNode, GoUnaryOperator};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{ConstructorKind, Fallible};
use crate::target::ir::values::Stability;
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::types::go_type::GoType;
use std::slice;
use syntax::ast::Pattern;
use syntax::ast::{Expression, Literal};
use syntax::program::CallOperation;
use syntax::program::SlotOrigin;
use syntax::types::Type;

pub(in crate::lowering) fn unreachable_panic_if_needed(
    place: &PlacePlan,
    is_exhaustive: bool,
) -> Option<Statement> {
    (place.is_return() && !is_exhaustive).then(|| LoweredStatement::UnreachablePanic.into())
}

fn calls_a_function(operation: Option<&CallOperation>) -> bool {
    matches!(
        operation,
        Some(
            CallOperation::Function
                | CallOperation::BoundMethod { .. }
                | CallOperation::UfcsFunction
                | CallOperation::Native { .. }
                | CallOperation::ReceiverMethod { .. }
        )
    )
}

fn fallible_constructor<'e>(
    fallible: &Fallible,
    callee: &Expression,
    args: &'e [Expression],
) -> Option<(&'static str, Option<(&'e Expression, Type)>)> {
    match (fallible.classify_constructor(callee), args) {
        (Some(ConstructorKind::Success), [argument]) => Some((
            fallible.ok_constructor(),
            Some((argument, fallible.ok_ty().clone())),
        )),
        (Some(ConstructorKind::Failure), [argument]) if let Some(err_ty) = fallible.err_ty() => {
            Some((fallible.err_constructor(), Some((argument, err_ty.clone()))))
        }
        (Some(ConstructorKind::Failure), _) if fallible.err_ty().is_none() => {
            Some((fallible.err_constructor(), None))
        }
        _ => None,
    }
}

fn simple_assign(target: &GoExpression, value: ValuePlan) -> LoweredStatement {
    let (setup, value) = value.into_parts();
    with_setup(
        setup,
        LoweredStatement::Assign(AssignForm::Simple {
            target: target.clone(),
            value,
        }),
    )
}

pub(in crate::lowering) fn rebind_trailing_temp(
    statements: &mut [Statement],
    name: &GoIdentifier,
    temp: &GoIdentifier,
) -> bool {
    statements.last_mut().is_some_and(|statement| {
        statement.kind.binds_local(temp) && statement.kind.rename_bound_name(name)
    })
}

pub(in crate::lowering) fn rebind_updated_temp(
    statements: &mut [Statement],
    name: &GoIdentifier,
    temp: &GoIdentifier,
) -> bool {
    let Some(start) = statements.iter().rposition(|s| s.kind.binds_local(temp)) else {
        return false;
    };
    let (head, writes) = statements.split_at_mut(start + 1);
    let copy = &mut head[start];
    let writes_field_of_temp = |statement: &Statement| {
        let LoweredStatement::Assign(AssignForm::Simple { target, value }) = &statement.kind else {
            return false;
        };
        matches!(
            target.node(),
            GoExpressionNode::Selector { base, .. }
                if matches!(base.as_ref(), GoExpressionNode::Identifier(read) if read == temp)
        ) && !value.node().mentions_local(name)
            && !value.node().mentions_local(temp)
    };

    if writes.is_empty() || !writes.iter().all(writes_field_of_temp) {
        return false;
    }
    if !copy.kind.rename_bound_name(name) {
        return false;
    }
    for write in writes {
        if let LoweredStatement::Assign(AssignForm::Simple { target, .. }) = &mut write.kind
            && let GoExpressionNode::Selector { base, .. } = target.node_mut()
        {
            **base = GoExpressionNode::Identifier(name.clone());
        }
    }
    true
}

pub(in crate::lowering) fn collapse_declared_temp(
    statements: &mut Vec<Statement>,
    name: &GoIdentifier,
    value_has_declared_type: bool,
) {
    let [declaration, filler] = statements.as_slice() else {
        return;
    };
    let LoweredStatement::VarDeclaration {
        name: declared,
        go_type,
        value: None,
    } = &declaration.kind
    else {
        return;
    };

    if declared != name {
        return;
    }
    let declared = declared.clone();
    let value = match &filler.kind {
        LoweredStatement::Assign(AssignForm::Simple { target, value }) => {
            if !infers_declared_type(go_type, value, value_has_declared_type)
                || target.as_identifier() != Some(name)
                || value.effects().runs_code()
            {
                return;
            }
            value.clone()
        }
        LoweredStatement::If(plan) => {
            if !go_type.is_named("bool") || plan.initializer.is_some() {
                return;
            }
            let ElseArm::Else {
                body: else_body, ..
            } = &plan.else_arm
            else {
                return;
            };

            let Some(then_value) = single_simple_assign_value(&plan.then_body, name) else {
                return;
            };
            let Some(else_value) = single_simple_assign_value(else_body, name) else {
                return;
            };
            let Some(joined) = join_boolean_branches(&plan.condition, &then_value, &else_value)
            else {
                return;
            };
            joined
        }
        _ => return,
    };

    statements.pop();
    statements[0].kind = LoweredStatement::Define(Definition::single(declared, value));
}

fn infers_declared_type(
    go_type: &GoType,
    value: &GoExpression,
    value_has_declared_type: bool,
) -> bool {
    if let Some(kind) = value.constant_kind() {
        return kind.is_default_go_type(go_type);
    }
    match value.node() {
        GoExpressionNode::CompositeLiteral {
            go_type: Some(literal_type),
            ..
        } => literal_type.as_ref() == go_type,
        GoExpressionNode::CompositeLiteral { go_type: None, .. }
        | GoExpressionNode::FunctionLiteral { .. }
        | GoExpressionNode::Verbatim(_) => false,
        GoExpressionNode::Binary { operator, .. } => match operator {
            GoBinaryOperator::Equal
            | GoBinaryOperator::NotEqual
            | GoBinaryOperator::LessThan
            | GoBinaryOperator::LessThanOrEqual
            | GoBinaryOperator::GreaterThan
            | GoBinaryOperator::GreaterThanOrEqual
            | GoBinaryOperator::And
            | GoBinaryOperator::Or => go_type.is_named("bool"),
            GoBinaryOperator::ShiftLeft
            | GoBinaryOperator::ShiftRight
            | GoBinaryOperator::Addition
            | GoBinaryOperator::Subtraction
            | GoBinaryOperator::Multiplication
            | GoBinaryOperator::Division
            | GoBinaryOperator::Remainder
            | GoBinaryOperator::BitwiseAnd
            | GoBinaryOperator::BitwiseOr
            | GoBinaryOperator::BitwiseXor
            | GoBinaryOperator::BitwiseAndNot => value_has_declared_type,
        },
        GoExpressionNode::Unary {
            operator: GoUnaryOperator::Not,
            ..
        } => go_type.is_named("bool"),
        _ => value_has_declared_type && !go_type.is_channel(),
    }
}

fn single_simple_assign_value(body: &LoweredBlock, name: &GoIdentifier) -> Option<GoExpression> {
    let [
        Statement {
            kind: LoweredStatement::Assign(assign),
            ..
        },
    ] = body.statements.as_slice()
    else {
        return None;
    };

    let AssignForm::Simple { target, value } = assign else {
        return None;
    };
    (target.as_identifier() == Some(name) && !value.effects().runs_code()).then(|| value.clone())
}

fn join_boolean_branches(
    condition: &GoExpression,
    then_value: &GoExpression,
    else_value: &GoExpression,
) -> Option<GoExpression> {
    let and = |left: GoExpression, right: GoExpression| {
        GoExpression::binary(left, GoBinaryOperator::And, right)
    };
    let or = |left: GoExpression, right: GoExpression| {
        GoExpression::binary(left, GoBinaryOperator::Or, right)
    };
    let not = |operand: &GoExpression| GoExpression::unary(GoUnaryOperator::Not, operand.clone());
    Some(match (then_value.as_boolean(), else_value.as_boolean()) {
        (Some(true), Some(false)) => condition.clone(),
        (Some(false), Some(true)) => not(condition),
        (Some(true), Some(true)) | (Some(false), Some(false)) => return None,
        (_, Some(false)) => and(condition.clone(), then_value.clone()),
        (Some(true), _) => or(condition.clone(), else_value.clone()),
        (Some(false), _) => and(not(condition), else_value.clone()),
        (_, Some(true)) => or(not(condition), then_value.clone()),
        _ => return None,
    })
}

pub(in crate::lowering) fn requires_temp_var(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::If { .. }
            | Expression::IfLet { .. }
            | Expression::Match { .. }
            | Expression::Block { .. }
            | Expression::Loop { .. }
            | Expression::Propagate { .. }
            | Expression::TryBlock { .. }
            | Expression::Select { .. }
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ElidableTail {
    Branching,
    FallibleBlock,
}

impl ElidableTail {
    fn accepts(self, value: &Expression) -> bool {
        match self {
            Self::Branching => matches!(
                value,
                Expression::If { .. } | Expression::IfLet { .. } | Expression::Match { .. }
            ),
            Self::FallibleBlock => matches!(value, Expression::TryBlock { .. }),
        }
    }
}

pub(in crate::lowering) fn expression_contains_binding(
    expression: &Expression,
    name: &str,
) -> bool {
    use syntax::ast::SelectArm;
    match expression {
        Expression::IfLet {
            pattern,
            consequence,
            alternative,
            ..
        } => {
            pattern_binds_name(pattern, name)
                || expression_contains_binding(consequence, name)
                || alternative
                    .expression()
                    .is_some_and(|alternative| expression_contains_binding(alternative, name))
        }
        Expression::Match { arms, .. } => arms
            .iter()
            .any(|arm| pattern_binds_name(&arm.pattern, name)),
        Expression::Block { items, .. } => items.iter().any(|item| match item {
            Expression::Let { binding, .. } => pattern_binds_name(&binding.pattern, name),
            _ => false,
        }),
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            expression_contains_binding(consequence, name)
                || alternative
                    .as_deref()
                    .is_some_and(|alternative| expression_contains_binding(alternative, name))
        }
        Expression::Select { arms, .. } => arms.iter().any(|arm| match arm {
            SelectArm::Receive { binding, .. } => pattern_binds_name(binding, name),
            SelectArm::MatchReceive { arms, .. } => {
                arms.iter().any(|a| pattern_binds_name(&a.pattern, name))
            }
            _ => false,
        }),
        Expression::Loop { body, .. } => expression_contains_binding(body, name),
        _ => false,
    }
}

impl<'p> Planner<'p> {
    pub(in crate::lowering) fn lower_discard_value(
        &mut self,
        value: &Expression,
    ) -> Vec<Statement> {
        self.record_converted_value(value);

        if is_side_effect_free_discard(&self.facts, value) {
            self.conversions.record_dropped_code(value.get_span());
            return Vec::new();
        }

        if let Expression::Propagate { expression, .. } = value {
            return self.lower_propagate_statement(expression);
        }

        let value_ty = value.get_type();
        if self.facts.is_never(&value_ty) {
            return self.lower_never_value(value);
        }
        if self.facts.has_no_go_value(&value_ty) {
            return self.lower_valueless_discard(value);
        }

        if let Expression::Call { .. } = value {
            let mut statements: Vec<Statement> = Vec::new();
            if let Some(call) = self.try_plan_go_call_discarded(&mut statements, value) {
                statements.push(effect_statement(call));
                return statements;
            }
        }

        let operand = self.plan_operand(value, ExpressionContext::value());
        let (mut statements, value_operand) = operand.into_parts();
        if statements.is_empty() && value_operand.constant_kind().is_some() {
            self.conversions.record_dropped_code(value.get_span());
            return statements;
        }
        if calls_a_function(self.facts.call_operation(value))
            && matches!(value, Expression::Call { .. })
            && value_operand.stands_as_a_statement()
        {
            statements.push(effect_statement(value_operand));
        } else {
            let value_operand = match self
                .queries()
                .constant_needs_go_type(value_operand.constant_kind(), &value_ty)
            {
                Some(go_type) => GoExpression::conversion(go_type, value_operand),
                None => value_operand,
            };
            statements.push(discard(value_operand));
        }
        statements
    }

    fn lower_valueless_discard(&mut self, value: &Expression) -> Vec<Statement> {
        let operand = self.plan_operand(value, ExpressionContext::value());
        let (mut statements, value_operand) = operand.into_parts();
        let effects = value_operand.effects();
        let reads_or_acts = !effects.can_erase() || effects.reads() != Stability::Fixed;
        if !value_operand.is_empty() && reads_or_acts {
            if matches!(value, Expression::Call { .. }) && value_operand.stands_as_a_statement() {
                statements.push(effect_statement(value_operand));
            } else {
                statements.push(discard(value_operand));
            }
        }
        statements
    }

    pub(in crate::lowering) fn lower_assign(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        if self.facts.is_option_or_result(&expression.get_type()) {
            return self.lower_option_result_assignment(target, target_ty, expression);
        }

        if let Expression::Loop { body, .. } = expression {
            let plan = self.with_loop(Some(target.clone()), target_ty.cloned(), |this| {
                this.lower_loop_with_header(LoopHeader::Infinite, body)
            });
            return vec![LoweredStatement::Loop(plan).into()];
        }

        self.lower_braced_block_to_var(expression, target, target_ty)
    }

    fn lower_plain_assign(
        &mut self,
        target: &GoExpression,
        target_ty: Option<&Type>,
        expression: &Expression,
    ) -> Vec<Statement> {
        let value = self
            .plan_operand(expression, ExpressionContext::value())
            .map_expression(|setup, value| {
                self.apply_type_coercion(setup, target_ty, expression, value)
            });
        vec![simple_assign(target, value).into()]
    }

    pub(in crate::lowering) fn lower_option_result_assignment(
        &mut self,
        target: &GoExpression,
        target_ty: Option<&Type>,
        expression: &Expression,
    ) -> Vec<Statement> {
        let written_ty = target_ty.cloned().unwrap_or_else(|| expression.get_type());
        let Some(fallible) = self.facts.fallible(&written_ty) else {
            return self.lower_plain_assign(target, target_ty, expression);
        };

        let actual_expression = match expression {
            Expression::Block { items, .. } if items.len() == 1 => &items[0],
            _ => expression,
        };
        let line = self.facts.source_position(&actual_expression.get_span());

        let statements = match actual_expression {
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                let Some((constructor_name, constructor_arg)) =
                    fallible_constructor(&fallible, callee, args)
                else {
                    return directed_first(
                        line,
                        self.lower_plain_assign(target, target_ty, expression),
                    );
                };

                let value =
                    self.plan_fallible_constructor(&fallible, constructor_name, constructor_arg);
                vec![simple_assign(target, value).into()]
            }
            Expression::Identifier { .. } => {
                if fallible.classify_constructor(actual_expression)
                    == Some(ConstructorKind::Failure)
                {
                    let call = self.queries().fallible_call(
                        &fallible,
                        fallible.err_constructor(),
                        Vec::new(),
                    );
                    vec![simple_assign(target, ValuePlan::computed(Vec::new(), call)).into()]
                } else {
                    self.lower_plain_assign(target, target_ty, expression)
                }
            }
            _ => self.lower_block_to_var(expression, target, target_ty),
        };

        directed_first(line, statements)
    }

    fn plan_fallible_constructor(
        &mut self,
        fallible: &Fallible,
        constructor_name: &str,
        constructor_arg: Option<(&Expression, Type)>,
    ) -> ValuePlan {
        let Some((constructor_arg, payload_ty)) = constructor_arg else {
            let call = self
                .queries()
                .fallible_call(fallible, constructor_name, Vec::new());
            return ValuePlan::computed(Vec::new(), call);
        };
        let argument = self.plan_composite_value(constructor_arg, ExpressionContext::value());
        let (mut arg_setup, argument) = argument.into_parts();
        let argument =
            self.apply_type_coercion(&mut arg_setup, Some(&payload_ty), constructor_arg, argument);

        let call = self
            .queries()
            .fallible_call(fallible, constructor_name, vec![argument]);
        ValuePlan::plain_call(arg_setup, call)
    }

    pub(in crate::lowering) fn lower_block_to_var(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        if matches!(expression, Expression::Block { .. }) {
            self.with_binding_frame(|this| this.lower_items_to_var(expression, target, target_ty))
        } else {
            self.lower_items_to_var(expression, target, target_ty)
        }
    }

    fn lower_braced_block_to_var(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        if !matches!(expression, Expression::Block { items, .. } if items.len() > 1) {
            return self.lower_block_to_var(expression, target, target_ty);
        }
        let statements =
            self.with_scope(|this| this.lower_items_to_var(expression, target, target_ty));
        vec![LoweredStatement::Block(LoweredBlock { statements }).into()]
    }

    fn lower_items_to_var(
        &mut self,
        expression: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        let items: &[Expression] = if let Expression::Block { items, .. } = expression {
            items
        } else {
            slice::from_ref(expression)
        };
        let Some((last, rest)) = items.split_last() else {
            if self.queries().widens_unit(target_ty) {
                return self.lower_unit_into(expression, None, target, target_ty);
            }
            return Vec::new();
        };

        self.with_assign_target(target, |this| {
            this.lower_block_items(
                rest,
                last,
                BlockTail::Value(TailTarget::Assign { target, target_ty }),
            )
        })
    }

    pub(in crate::lowering) fn lower_unit_into(
        &mut self,
        source: &Expression,
        effect: Option<&Expression>,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        if let Some(effect) = effect.map(|effect| self.lower_statement(effect))
            && !renders_nothing(&effect)
        {
            statements.push(effect);
        }
        statements.push(assign(
            target.clone(),
            self.unit_value(Some(source), target_ty),
        ));
        statements
    }

    pub(in crate::lowering) fn lower_assign_unit_tail(
        &mut self,
        last: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        if let Expression::Unit { .. } = last {
            return vec![assign(
                target.clone(),
                self.unit_value(Some(last), target_ty),
            )];
        }
        if !self.queries().widens_unit(target_ty) {
            if matches!(last, Expression::Block { .. }) {
                return self.lower_assign(last, target, target_ty);
            }
            if let Some(branching) = Branching::of(last) {
                let place = PlacePlan::Assign {
                    local: target,
                    target_ty,
                };
                return self.lower_branching_to_block(branching, &place).statements;
            }
        }
        self.lower_unit_into(last, Some(last), target, target_ty)
    }

    pub(in crate::lowering) fn lower_assign_value_tail(
        &mut self,
        last: &Expression,
        target: &GoExpression,
        target_ty: Option<&Type>,
    ) -> Vec<Statement> {
        if matches!(last, Expression::Block { .. }) {
            return self.lower_assign(last, target, target_ty);
        }
        if let Some(statements) = self.try_lower_slice_growth_to_var(target, last) {
            return statements;
        }
        if let Some(branching) = Branching::of(last) {
            let place = PlacePlan::Assign {
                local: target,
                target_ty,
            };
            return self.lower_branching_to_block(branching, &place).statements;
        }
        let origin = target_ty.map_or(SlotOrigin::Lisette, |ty| {
            self.facts.function_type_origin(ty, SlotOrigin::Lisette)
        });
        let value = self.plan_value(
            last,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let value = value.map_expression(|setup, expression| {
            self.apply_type_coercion(setup, target_ty, last, expression)
        });
        vec![simple_assign(target, value).into()]
    }

    fn try_lower_slice_growth_to_var(
        &mut self,
        target: &GoExpression,
        last: &Expression,
    ) -> Option<Vec<Statement>> {
        let site = CallSite::of(last)?;
        let (growth, receiver) = slice_growth_method(&self.facts, site.callee)?;

        let receiver_is_lvalue =
            is_lvalue_chain(&self.facts, receiver) && !contains_newtype_access(receiver);

        let (value, mut statements) = if receiver_is_lvalue {
            self.lower_slice_growth_in_place(target, site, growth, receiver)
        } else {
            let (setup, value) = self
                .plan_value(last, ExpressionContext::value())
                .into_parts();
            (value, setup)
        };

        statements.push(simple_assign(target, ValuePlan::computed(Vec::new(), value)).into());
        Some(statements)
    }

    fn lower_slice_growth_in_place(
        &mut self,
        target: &GoExpression,
        site: CallSite<'_>,
        growth: SliceGrowth,
        receiver: &Expression,
    ) -> (GoExpression, Vec<Statement>) {
        let (arguments, ordering) = self.lower_growth_args(site.expression, site.args, site.spread);
        let mut capture: Vec<Statement> = Vec::new();
        let receiver_place =
            self.plan_place(&mut capture, receiver, PlaceOrdering::before(&ordering));
        let target_differs = receiver_place.node() != target.node();
        let receiver_read = ValuePlan::computed(Vec::new(), receiver_place.clone())
            .with_stability(self.queries().path_read_stability(&receiver_place));
        let arguments_effects = arguments
            .iter()
            .fold(Effects::default(), |effects, argument| {
                effects.union(argument.effects())
            });
        let mut later = LaterOperands::sequenced(
            ordering.setup(),
            arguments_effects.with_known_reads(arguments_effects.reads()),
        );
        let receiver = if target_differs
            && !arguments.is_empty()
            && (!is_clip_safe_path(&receiver_place) || later.prepend(&receiver_read))
        {
            GoExpression::identifier(self.hoist_temp(&mut capture, "recv", receiver_place))
        } else {
            receiver_place
        };

        capture.extend(ordering.into_parts().0);
        let value = if target_differs {
            self.detach_slice_growth(&mut capture, growth, receiver, &arguments, site.ty)
        } else {
            slice_growth_call(growth, receiver, &arguments)
        };

        (value, capture)
    }

    fn lower_growth_args(
        &mut self,
        call: &Expression,
        args: &[Expression],
        spread: Option<&Expression>,
    ) -> (Vec<GoExpression>, ValuePlan) {
        let operands: Vec<ValuePlan> = args
            .iter()
            .map(|argument| {
                let plan = self.plan_composite_value(argument, ExpressionContext::value());
                self.convert_to_recorded_slot(argument, plan)
            })
            .collect();

        let spread_operand = spread.map(|spread| self.plan_copied_spread(spread));
        let plan = spread
            .and_then(|_| CallSite::of(call))
            .map(|site| self.queries().plan_call(site));
        let wrap_to_any = plan
            .as_ref()
            .is_some_and(|plan| plan.resolved.abi.spread_wraps_to_any(&self.facts, spread));
        let combine = plan.and_then(|plan| plan.resolved.abi.variadic_combine(0));
        let sequenced = self.sequence_with_spread_values(
            operands,
            spread_operand,
            SpreadSequenceOptions {
                wrap_to_any,
                combine,
                boundary: CaptureBoundary::SiblingSequence,
            },
        );

        let ordering = ValuePlan::computed(sequenced.setup, GoExpression::empty());
        (sequenced.values, ordering)
    }

    pub(in crate::lowering) fn plan_tail_value(
        &mut self,
        last: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let plan = if let Some(literal) = TupleLiteral::of(last) {
            self.plan_tuple_value(literal, TuplePosition::Tail)
        } else {
            self.plan_value(last, ExpressionContext::value())
        };

        plan.into_parts()
    }

    pub(in crate::lowering) fn plan_to_operand_temp(
        &mut self,
        expression: &Expression,
        ty: &Type,
    ) -> ValuePlan {
        if let Expression::Block { .. } = expression {
            if self.facts.has_no_go_value(ty) || matches!(ty, Type::Forall { .. }) {
                return ValuePlan::computed(
                    self.lower_block_as_body(expression).statements,
                    GoExpression::empty(),
                );
            }
            let slot = self.facts.value_use_slot(expression).unwrap_or(ty);
            let (result_var, declaration) = self.operand_temp_declaration(slot);
            let target = GoExpression::identifier(result_var.clone());
            let mut statements = vec![declaration];
            statements.extend(self.lower_braced_block_to_var(expression, &target, Some(slot)));
            return ValuePlan::captured(statements, result_var);
        }
        if let Expression::Loop { body, .. } = expression {
            return self.plan_loop_as_operand_temp(body, ty);
        }
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let mut statements = vec![declaration];
        let target = GoExpression::identifier(result_var.clone());
        statements.extend(self.lower_assign(expression, &target, Some(ty)));
        ValuePlan::captured(statements, result_var)
    }

    pub(in crate::lowering) fn build_break_value_plan(
        &mut self,
        break_value: &Expression,
    ) -> LoweredBlock {
        let loop_target = self.scope.current_loop().and_then(|loop_context| {
            let result_ty = loop_context.result_ty.clone()?;
            Some((loop_context.result.clone()?, result_ty))
        });
        if let Some((target, result_ty)) = loop_target {
            let mut statements = self.lower_assign(break_value, &target, Some(&result_ty));
            statements.push(LoweredStatement::Break(self.queries().current_loop_transfer()).into());
            return LoweredBlock { statements };
        }
        let (mut statements, value) = self
            .plan_value(break_value, ExpressionContext::value())
            .into_parts();
        if value.is_empty() && matches!(break_value, Expression::Propagate { .. }) {
            return LoweredBlock { statements };
        }

        if let Some(loop_context) = self.scope.current_loop().cloned()
            && let Some(target) = loop_context.result
        {
            if self.queries().is_unit_call(break_value) {
                if !value.is_empty() {
                    statements.push(effect_statement(value));
                }
                let unit = self.unit_value(Some(break_value), loop_context.result_ty.as_ref());
                statements.push(assign(target, unit));
            } else if !value.is_empty() {
                let value = self.apply_type_coercion(
                    &mut statements,
                    loop_context.result_ty.as_ref(),
                    break_value,
                    value,
                );

                statements.push(assign(target, value));
            }
        } else if !value.is_empty() {
            statements.push(discard(value));
        }
        statements.push(LoweredStatement::Break(self.queries().current_loop_transfer()).into());
        LoweredBlock { statements }
    }
}

fn slice_growth_method<'e>(
    facts: &EmitFacts<'_>,
    function: &'e Expression,
) -> Option<(SliceGrowth, &'e Expression)> {
    let Expression::DotAccess { expression, .. } = function else {
        return None;
    };
    let growth = SliceGrowth::of(facts.checked(function).callee().native()?)?;
    Some((growth, expression))
}

fn is_side_effect_free_discard<'a>(facts: &EmitFacts<'a>, expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        _ if is_zero_call(facts, expression) => true,
        Expression::Literal { literal, .. } => matches!(
            literal,
            Literal::Integer { .. }
                | Literal::Float { .. }
                | Literal::Imaginary(_)
                | Literal::Boolean(_)
                | Literal::String { .. }
                | Literal::Char(_)
        ),
        _ => false,
    }
}

pub(in crate::lowering) fn is_zero_call<'a>(
    facts: &EmitFacts<'a>,
    expression: &Expression,
) -> bool {
    matches!(facts.call_operation(expression), Some(CallOperation::Zero))
}

pub(in crate::lowering) fn try_elide_tail_let<'a, 'i>(
    facts: &EmitFacts<'a>,
    items: &'i [Expression],
    elidable: ElidableTail,
) -> Option<(&'i Expression, &'i [Expression])> {
    if items.len() < 2 {
        return None;
    }
    let last = items.last()?;
    let Expression::Identifier {
        value: tail_name,
        resolution,
        ..
    } = last
    else {
        return None;
    };

    let penultimate = &items[items.len() - 2];
    let Expression::Let {
        binding,
        value,
        mode,
        ..
    } = penultimate
    else {
        return None;
    };

    if mode.else_block().is_some() || binding.is_mutable() {
        return None;
    }
    let Pattern::Identifier { identifier, .. } = &binding.pattern else {
        return None;
    };
    if identifier != tail_name || facts.converts(last) {
        return None;
    }
    if !elidable.accepts(value.as_ref()) {
        return None;
    }
    let rest = &items[..items.len() - 2];
    if region_blocks_inline(rest.iter(), resolution.binding_id()) {
        return None;
    }
    Some((value.as_ref(), rest))
}

fn renders_nothing(statement: &Statement) -> bool {
    match &statement.kind {
        LoweredStatement::Block(block) | LoweredStatement::Body(block) => {
            block.statements.iter().all(renders_nothing)
        }
        LoweredStatement::ExpressionStatement { expression, .. } => expression.is_empty(),
        _ => false,
    }
}

impl<'p> PlannerQueries<'_, 'p> {
    pub(in crate::lowering) fn is_unit_call(&self, expression: &Expression) -> bool {
        self.facts.resolves_to_unit(&expression.get_type())
            && self
                .facts
                .call_operation(expression)
                .is_some_and(|operation| *operation != CallOperation::Zero)
    }

    pub(in crate::lowering) fn widens_unit(&self, target_ty: Option<&Type>) -> bool {
        target_ty.is_some_and(|ty| !self.facts.resolves_to_unit(ty))
    }
}
