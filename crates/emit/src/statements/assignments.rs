use crate::EmitFacts;
use crate::Planner;
use crate::abi::layout::ValueLayout;
use crate::expressions::staging::LaterStages;
use crate::ir::bodies::with_setup;
use crate::ir::bodies::{
    AssignForm, CompoundKind, LoweredBlock, LoweredStatement, Statement, define,
};
use crate::ir::go_expression::GoExpressionNode;
use crate::ir::values::{GoExpression, ValuePlan};
use crate::is_order_sensitive;
use crate::state::bindings::BindingValue;
use crate::state::expression::ExpressionContext;
use syntax::ast::Literal;
use syntax::ast::{BinaryOperator, Expression, IdentifierResolution, UnaryOperator};
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;

#[derive(Clone, Copy)]
pub(crate) struct PlaceOrdering<'a> {
    right_hand_side: Option<&'a ValuePlan>,
    read_twice: bool,
}

impl<'a> PlaceOrdering<'a> {
    pub(crate) fn before(right_hand_side: &'a ValuePlan) -> Self {
        Self {
            right_hand_side: Some(right_hand_side),
            read_twice: false,
        }
    }

    pub(crate) fn alone() -> Self {
        Self {
            right_hand_side: None,
            read_twice: false,
        }
    }

    fn read_twice(self) -> Self {
        Self {
            read_twice: true,
            ..self
        }
    }

    fn later(self) -> LaterStages {
        self.right_hand_side
            .map_or_else(LaterStages::default, |value| {
                LaterStages::sequenced(value.setup(), value.effects())
            })
    }

    fn pins(self, plan: &ValuePlan) -> bool {
        self.later().prepend(plan, false) || (self.read_twice && !plan.effects().can_duplicate())
    }
}

impl Planner<'_> {
    pub(crate) fn build_assignment_plan(
        &mut self,
        target: &Expression,
        value: &Expression,
        compound_operator: Option<&BinaryOperator>,
    ) -> LoweredStatement {
        let raw_body = |statements: Vec<Statement>| LoweredBlock { statements };

        if value.get_type().is_never() {
            let mut statements = self.lower_target_operands(target);
            statements.push(self.lower_statement(value));
            return LoweredStatement::Body(raw_body(statements));
        }

        if let Some((op, rhs)) = detect_compound_assignment(target, value, compound_operator) {
            return self.build_compound_assignment_plan(target, op, rhs);
        }

        if self.target_binds_to_discard(target) {
            return LoweredStatement::Body(raw_body(self.lower_discard_value(value)));
        }

        let go_field_slot = self
            .go_place_layout(target)
            .or_else(|| self.go_physical_expression_layout(target));

        let literal_slot = go_field_slot
            .as_ref()
            .and_then(|layout| self.try_plan_option_literal_into_layout(value, layout));
        let is_literal_slot = literal_slot.is_some();
        let origin = if go_field_slot.is_some() {
            SlotOrigin::Lisette
        } else {
            self.function_type_origin(&target.get_type(), SlotOrigin::Lisette)
        };
        let value_context = ExpressionContext::value()
            .with_function_slot_origin(origin)
            .with_retired_receiver(target);
        let right_hand_side =
            literal_slot.unwrap_or_else(|| self.plan_composite_value(value, value_context));
        let (target_capture, target_place) =
            self.capture_assignment_target(target, PlaceOrdering::before(&right_hand_side));
        let (value_setup, value_go) = right_hand_side.into_parts();
        let mut setup = target_capture;
        setup.extend(value_setup);
        let value = if is_literal_slot {
            value_go
        } else {
            let go_slot = go_field_slot
                .as_ref()
                .map(|target| (self.emitted_value_layout(value, value_context), target));
            self.coerce_into_slot(
                &mut setup,
                (value, value_go),
                &target.get_type(),
                go_slot.as_ref().map(|(source, target)| (source, *target)),
            )
        };
        with_setup(
            setup,
            LoweredStatement::Assign(AssignForm::Simple {
                target: target_place,
                value,
            }),
        )
    }

    fn go_place_layout(&self, target: &Expression) -> Option<ValueLayout> {
        match target {
            Expression::DotAccess {
                expression,
                member,
                ty,
                ..
            } => self.field_slot_layout(
                &expression.get_type(),
                self.facts.checked(target).field_selection(),
                member,
                ty,
            ),
            Expression::IndexedAccess { expression, .. } => {
                self.go_place_layout(expression)?.indexed_element().cloned()
            }
            _ => None,
        }
    }

    fn lower_target_operands(&mut self, target: &Expression) -> Vec<Statement> {
        match target {
            Expression::Identifier { .. } | Expression::Literal { .. } => Vec::new(),
            Expression::DotAccess { expression, .. }
            | Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => self.lower_target_operands(expression),
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                let mut statements = self.lower_target_operands(expression);
                statements.extend(self.lower_target_operands(index));
                statements
            }
            operand => self.lower_discard_value(operand),
        }
    }

    fn build_compound_assignment_plan(
        &mut self,
        target: &Expression,
        op: &BinaryOperator,
        rhs: &Expression,
    ) -> LoweredStatement {
        let is_inc_dec = is_literal_one(rhs)
            && matches!(op, BinaryOperator::Addition | BinaryOperator::Subtraction);
        if is_inc_dec {
            let kind = if *op == BinaryOperator::Addition {
                CompoundKind::Increment
            } else {
                CompoundKind::Decrement
            };
            let (target_capture, target_place) =
                self.capture_assignment_target(target, PlaceOrdering::alone());
            return with_setup(
                target_capture,
                LoweredStatement::Assign(AssignForm::Compound {
                    target: target_place,
                    kind,
                }),
            );
        }

        let right_hand_side = self.plan_operand(rhs, ExpressionContext::value());
        let mut ordering = PlaceOrdering::before(&right_hand_side);
        let needs_left_pin = ordering
            .later()
            .can_change(self.place_read_stability(target));
        if needs_left_pin {
            ordering = ordering.read_twice();
        }
        let (mut target_capture, target_place) = self.capture_assignment_target(target, ordering);
        let pinned_left = needs_left_pin.then(|| {
            let tmp = self.fresh_declared("left");
            target_capture.push(define(tmp.clone(), target_place.clone()));
            GoExpression::identifier(tmp)
        });
        let (rhs_setup, rhs) = right_hand_side.into_parts();
        target_capture.extend(rhs_setup);
        let operator = op.into();
        let assignment = match pinned_left {
            Some(left) => AssignForm::Simple {
                target: target_place,
                value: GoExpression::binary(left, operator, rhs),
            },
            None => AssignForm::Compound {
                target: target_place,
                kind: CompoundKind::OpAssign { operator, rhs },
            },
        };
        with_setup(target_capture, LoweredStatement::Assign(assignment))
    }

    fn capture_assignment_target(
        &mut self,
        target: &Expression,
        ordering: PlaceOrdering,
    ) -> (Vec<Statement>, GoExpression) {
        let mut target_capture: Vec<Statement> = Vec::new();
        let target = self.plan_place(&mut target_capture, target, ordering);
        (target_capture, target)
    }

    fn target_binds_to_discard(&self, target: &Expression) -> bool {
        let Expression::Identifier {
            value, resolution, ..
        } = target
        else {
            return false;
        };
        match self
            .scope
            .resolve_identifier_with_resolution(value, resolution)
        {
            Some(binding) => binding.is_discard(),
            None => value == "_",
        }
    }

    pub(crate) fn plan_place(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        match expression {
            Expression::Identifier {
                value, resolution, ..
            } => match self
                .scope
                .resolve_identifier_with_resolution(value, resolution)
                .and_then(BindingValue::as_go_identifier)
            {
                Some(identifier) => GoExpression::identifier(identifier.clone()),
                None => {
                    assert!(
                        resolution.binding_id().is_none(),
                        "assignment target has no local identity: {value}"
                    );
                    GoExpression::external_name(value.to_string())
                }
            },
            Expression::DotAccess {
                expression: base,
                member,
                resolution,
                ..
            } => {
                let expression_ty = base.get_type();
                if let Some(package) = expression_ty.as_import_namespace() {
                    let field =
                        self.dot_member_go_name(base, &expression_ty, member, resolution, false);
                    return GoExpression::qualified(self.package_use_for_package(package), field);
                }
                let elided_deref = base
                    .deref_inner()
                    .filter(|inner| !self.facts.points_to_ref(&inner.get_type()));
                let base_value = if let Some(inner) = elided_deref {
                    self.place_operand(setup, inner, "ref", ordering)
                } else if self.facts.reads_through_reference(base)
                    || !self.facts.is_place_expression(base)
                {
                    self.place_operand(setup, base, "ref", ordering)
                } else {
                    self.plan_place(setup, base, ordering)
                };
                if let Some(access) =
                    self.try_plan_tuple_member_dot(&base_value, &expression_ty, member, resolution)
                {
                    return access;
                }
                let field =
                    self.dot_member_go_name(base, &expression_ty, member, resolution, false);
                self.facts.field_access(base_value, &expression_ty, field)
            }
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => self.plan_indexed_place(setup, base, index, ordering),
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: pointee,
                ..
            } => self.plan_deref_lvalue(setup, pointee, ordering),
            Expression::Call { .. } if self.facts.is_reference(&expression.get_type()) => {
                self.place_operand(setup, expression, "ref", ordering)
            }
            _ => unreachable!(
                "assignment target has no Go place at {:?}: {:?}",
                expression.get_span(),
                expression
            ),
        }
    }

    fn plan_indexed_place(
        &mut self,
        setup: &mut Vec<Statement>,
        base: &Expression,
        index: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let base_expression = if let Some(inner) = base.deref_inner() {
            GoExpression::dereference(self.place_operand(setup, inner, "ref", ordering))
        } else if self.facts.is_place_expression(base) {
            self.plan_place(setup, base, ordering)
        } else {
            self.place_operand(setup, base, "base", ordering)
        };
        let index_plan = self.plan_composite_value(index, ExpressionContext::value());
        let pin_index = ordering.pins(&index_plan);
        let base_plan = ValuePlan::computed(Vec::new(), base_expression)
            .with_stability(self.place_read_stability(base));
        let mut later = ordering.later();
        later.prepend(&index_plan, false);
        let pin_base = ordering.pins(&base_plan)
            || later.prepend(&base_plan, false)
            || (is_order_sensitive(base)
                && (base_plan.expression().effects().runs_code()
                    || index_plan.effects().runs_code()));
        let base_expression = base_plan.into_parts().1;
        let base_expression = if pin_base {
            self.pin_place_base(setup, base, base_expression)
        } else {
            base_expression
        };
        let (index_setup, index_expression) = index_plan.into_parts();
        setup.extend(index_setup);
        let index_expression = if pin_index {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, "idx", index_expression))
        } else {
            index_expression
        };
        GoExpression::index(base_expression, index_expression)
    }

    fn pin_place_base(
        &mut self,
        setup: &mut Vec<Statement>,
        base: &Expression,
        expression: GoExpression,
    ) -> GoExpression {
        let pinned = if self.facts.native_kind(&base.get_type()) != Some(NativeTypeKind::Array) {
            expression
        } else if let GoExpressionNode::Dereference(pointer) = expression.node() {
            expression.peeled(pointer)
        } else {
            GoExpression::address_of(expression)
        };
        GoExpression::identifier(self.hoist_tmp_value_statement(setup, "base", pinned))
    }

    fn place_operand(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        prefix: &'static str,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let plan = self.plan_composite_value(expression, ExpressionContext::value());
        let pin = ordering.pins(&plan);
        let (value_setup, value) = plan.into_parts();
        setup.extend(value_setup);
        if pin {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, prefix, value))
        } else {
            value
        }
    }

    fn plan_deref_lvalue(
        &mut self,
        setup: &mut Vec<Statement>,
        pointee: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let pointee_plan = self.plan_operand(pointee, ExpressionContext::value());
        let needs_capture =
            matches!(pointee, Expression::Call { .. }) || ordering.pins(&pointee_plan);
        let (pointee_setup, pointee_value) = pointee_plan.into_parts();
        setup.extend(pointee_setup);
        if needs_capture {
            let tmp = self.hoist_tmp_value_statement(setup, "ref", pointee_value);
            return GoExpression::dereference(GoExpression::identifier(tmp));
        }
        GoExpression::dereference(pointee_value)
    }
}

fn detect_compound_assignment<'a>(
    target: &Expression,
    value: &'a Expression,
    compound_operator: Option<&'a BinaryOperator>,
) -> Option<(&'a BinaryOperator, &'a Expression)> {
    if let Some(op) = compound_operator {
        return Some((op, value));
    }
    let Expression::Binary {
        left,
        operator,
        right,
        ..
    } = value
    else {
        return None;
    };
    if !is_compound_eligible(operator) || !lvalues_match(target, left) {
        return None;
    }
    Some((operator, right.as_ref()))
}

fn is_literal_one(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Literal {
            literal: Literal::Integer { value: 1, .. },
            ..
        }
    )
}

pub(crate) fn lvalues_match(a: &Expression, b: &Expression) -> bool {
    match (a, b) {
        (
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id_a),
                ..
            },
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id_b),
                ..
            },
        ) => id_a == id_b,
        (
            Expression::DotAccess {
                expression: base_a,
                member: member_a,
                ..
            },
            Expression::DotAccess {
                expression: base_b,
                member: member_b,
                ..
            },
        ) => member_a == member_b && lvalues_match(base_a, base_b),
        (
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner_a,
                ..
            },
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner_b,
                ..
            },
        ) => lvalues_match(inner_a, inner_b),
        _ => false,
    }
}

fn is_compound_eligible(op: &BinaryOperator) -> bool {
    matches!(
        op,
        BinaryOperator::Addition
            | BinaryOperator::Subtraction
            | BinaryOperator::Multiplication
            | BinaryOperator::Division
            | BinaryOperator::Remainder
    )
}

impl EmitFacts<'_> {
    fn is_place_expression(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier { .. }
            | Expression::DotAccess { .. }
            | Expression::IndexedAccess { .. } => true,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => true,
            Expression::Call { .. } => self.is_reference(&expression.get_type()),
            _ => false,
        }
    }

    fn reads_through_reference(&self, base: &Expression) -> bool {
        matches!(base, Expression::Identifier { .. }) && self.is_reference(&base.get_type())
    }

    pub(crate) fn is_lvalue_chain(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Identifier { .. } => true,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => true,
            Expression::IndexedAccess { expression, .. }
            | Expression::DotAccess { expression, .. } => self.is_lvalue_chain(expression),
            Expression::Call { .. } => self.is_reference(&expression.get_type()),
            _ => false,
        }
    }
}
