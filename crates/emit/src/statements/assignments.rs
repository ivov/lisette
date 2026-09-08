use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::context::expression::ExpressionContext;
use crate::expressions::staging::LaterStages;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::plan::bodies::{AssignForm, CompoundKind, LoweredBlock, LoweredStatement, define};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::values::{EvaluationEffect, GoExpression, ValuePlan};
use crate::state::bindings::BindingValue;
use syntax::ast::Literal;
use syntax::ast::{BinaryOperator, Expression, IdentifierResolution, UnaryOperator};
use syntax::parse::TUPLE_FIELDS;
use syntax::program::DotAccessResolution;
use syntax::types::Type;

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

    fn alone() -> Self {
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
                LaterStages::sequenced(&value.setup, value.evaluation.effect)
            })
    }

    fn pins(self, plan: &ValuePlan) -> bool {
        let mut later = self.later();
        later.can_change(plan.evaluation.stability)
            || later.prepend(plan)
            || (self.read_twice && plan.expression.does_work())
    }
}

impl Planner<'_> {
    pub(crate) fn build_assignment_plan(
        &mut self,
        target: &Expression,
        value: &Expression,
        compound_operator: Option<&BinaryOperator>,
    ) -> LoweredStatement {
        let raw_body = |statements: Vec<LoweredStatement>| LoweredBlock { statements };

        if value.get_type().is_never() {
            return LoweredStatement::Body(raw_body(vec![self.lower_statement(value)]));
        }

        if let Some((op, rhs)) = detect_compound_assignment(target, value, compound_operator) {
            return LoweredStatement::Assign(self.build_compound_assignment_plan(target, op, rhs));
        }

        if self.target_binds_to_discard(target) {
            return LoweredStatement::Body(raw_body(self.lower_discard_value(value)));
        }

        let go_field_slot: Option<(Type, ValueLayout)> = match target {
            Expression::DotAccess {
                expression,
                member,
                ty,
                resolution,
                ..
            } => self
                .field_slot_layout(
                    &expression.get_type(),
                    resolution.declaring_type(),
                    member,
                    ty,
                )
                .map(|layout| (ty.clone(), layout)),
            _ => None,
        };

        // `target = value`. Stage RHS first (so the target capture knows
        // whether RHS produced setup), capture the target, then fold RHS
        // setup + coercion setup into the value plan in emission order.
        let literal_slot = go_field_slot
            .as_ref()
            .and_then(|(_, layout)| self.lower_option_literal_into_layout(value, layout));
        let is_literal_slot = literal_slot.is_some();
        let right_hand_side = literal_slot.unwrap_or_else(|| {
            self.lower_composite_value(
                value,
                ExpressionContext::value().with_retired_receiver(target),
            )
        });
        let (target_capture, target_place) =
            self.capture_assignment_target(target, PlaceOrdering::before(&right_hand_side));
        let coercion = if is_literal_slot {
            CoercionPlan::Identity
        } else if let Some((_target_ty, target_layout)) = go_field_slot {
            let source_layout = self.value_layout(&value.get_type(), SlotOrigin::Lisette);
            CoercionPlan::bridge(self, &source_layout, &target_layout)
        } else {
            self.value_slot_coercion(value, &target.get_type())
        };
        let value = right_hand_side.map_expression_as_computed(|value_setup, rhs_value| {
            let (coercion_setup, final_value) = coercion.lower(self, rhs_value);
            value_setup.extend(coercion_setup);
            final_value
        });
        LoweredStatement::Assign(AssignForm::Simple {
            target_capture,
            target: target_place,
            value,
        })
    }

    /// Build a compound assignment plan (`+=`, `-=`, `++`, etc.), staging the
    /// right-hand side and capturing the target in evaluation order.
    fn build_compound_assignment_plan(
        &mut self,
        target: &Expression,
        op: &BinaryOperator,
        rhs: &Expression,
    ) -> AssignForm {
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
            return AssignForm::Compound {
                target_capture,
                target: target_place,
                kind,
            };
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
            let tmp = self.fresh_var(Some("left"));
            self.declare(&tmp);
            target_capture.push(define(tmp.clone(), target_place.clone()));
            GoExpression::name(tmp)
        });
        let kind = CompoundKind::OpAssign {
            op_text: format!("{}", op),
            rhs: Box::new(right_hand_side),
            pinned_left,
        };
        AssignForm::Compound {
            target_capture,
            target: target_place,
            kind,
        }
    }

    fn capture_assignment_target(
        &mut self,
        target: &Expression,
        ordering: PlaceOrdering,
    ) -> (Vec<LoweredStatement>, GoExpression) {
        let mut target_capture: Vec<LoweredStatement> = Vec::new();
        let target = self.lower_place(&mut target_capture, target, ordering);
        (target_capture, target)
    }

    fn target_binds_to_discard(&self, target: &Expression) -> bool {
        let Expression::Identifier { value, .. } = target.unwrap_parens() else {
            return false;
        };
        match self.scope.resolve_identifier_binding(value) {
            Some(BindingValue::GoName(go_name) | BindingValue::GoConst(go_name)) => go_name == "_",
            Some(BindingValue::InlineExpr(_)) => false,
            None => value == "_",
        }
    }

    pub(crate) fn lower_place(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::Identifier { value, .. } => GoExpression::name(
                self.scope
                    .resolve_binding_go_name(value)
                    .unwrap_or(value)
                    .to_string(),
            ),
            Expression::DotAccess {
                expression: base,
                member,
                resolution,
                ..
            } => {
                let base_value = if let Some(inner) = base.deref_inner() {
                    self.place_operand(setup, inner, "ref", ordering)
                } else if reads_through_reference(base) || !is_place_expression(base) {
                    self.place_operand(setup, base, "ref", ordering)
                } else {
                    self.lower_place(setup, base, ordering)
                };
                let expression_ty = base.get_type();
                self.format_dot_access_lvalue(base_value, &expression_ty, member, resolution)
            }
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => self.lower_indexed_place(setup, base, index, ordering),
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: pointee,
                ..
            } => self.emit_deref_lvalue(setup, pointee, ordering),
            Expression::Call { .. } if expression.get_type().is_ref() => {
                let (call_setup, call) = self
                    .lower_composite_value(expression, ExpressionContext::value())
                    .into_parts();
                setup.extend(call_setup);
                GoExpression::name(self.hoist_tmp_value_statement(setup, "ref", call))
            }
            _ => GoExpression::name("_".to_string()),
        }
    }

    fn lower_indexed_place(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        base: &Expression,
        index: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let base_expression = if let Some(inner) = base.deref_inner() {
            GoExpression::dereference(self.place_operand(setup, inner, "ref", ordering))
        } else if is_place_expression(base) {
            self.lower_place(setup, base, ordering)
        } else {
            self.place_operand(setup, base, "base", ordering)
        };
        let index_plan = self.lower_composite_value(index, ExpressionContext::value());
        let pin_index = ordering.pins(&index_plan);
        let base_effect = if base_expression.does_work() {
            EvaluationEffect::EffectfulCall
        } else {
            EvaluationEffect::Pure
        };
        let base_plan = ValuePlan::computed(Vec::new(), base_expression, base_effect)
            .with_stability(self.place_read_stability(base));
        let mut later = ordering.later();
        later.prepend(&index_plan);
        let pin_base = ordering.pins(&base_plan)
            || later.prepend(&base_plan)
            || (is_order_sensitive(base)
                && (base_plan.expression.does_work() || index_plan.evaluation.effect.has_call()));
        let base_expression = if pin_base {
            self.pin_place_base(setup, base, base_plan.expression)
        } else {
            base_plan.expression
        };
        setup.extend(index_plan.setup);
        let index_expression = if pin_index {
            GoExpression::name(self.hoist_tmp_value_statement(setup, "idx", index_plan.expression))
        } else {
            index_plan.expression
        };
        GoExpression::index(base_expression, index_expression)
    }

    fn pin_place_base(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        base: &Expression,
        expression: GoExpression,
    ) -> GoExpression {
        let pinned = if !matches!(self.emit_shape_ty(&base.get_type()), Type::Array { .. }) {
            expression
        } else if let GoExpressionNode::Dereference(pointer) = expression.node() {
            GoExpression::from_node(pointer.as_ref().clone())
        } else {
            GoExpression::address_of(expression)
        };
        GoExpression::name(self.hoist_tmp_value_statement(setup, "base", pinned))
    }

    fn place_operand(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        prefix: &str,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let plan = self.lower_composite_value(expression, ExpressionContext::value());
        let pin = ordering.pins(&plan);
        let (value_setup, value) = plan.into_parts();
        setup.extend(value_setup);
        if pin {
            GoExpression::name(self.hoist_tmp_value_statement(setup, prefix, value))
        } else {
            value
        }
    }

    /// Emit `*X` lvalue form, capturing the pointee into a temp if it's a
    /// call (Go requires an addressable operand for deref-assignment) or when
    /// RHS setup could reassign the pointer before the write executes.
    fn emit_deref_lvalue(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        pointee: &Expression,
        ordering: PlaceOrdering,
    ) -> GoExpression {
        let pointee_plan = self.plan_operand(pointee, ExpressionContext::value());
        let needs_capture = matches!(pointee.unwrap_parens(), Expression::Call { .. })
            || ordering.pins(&pointee_plan);
        let (pointee_setup, pointee_value) = pointee_plan.into_parts();
        setup.extend(pointee_setup);
        if needs_capture {
            let tmp = self.hoist_tmp_value_statement(setup, "ref", pointee_value);
            return GoExpression::dereference(GoExpression::name(tmp));
        }
        GoExpression::dereference(pointee_value)
    }

    /// Format a dot-access lvalue (struct field or tuple element) onto the
    /// already-emitted base expression. Numeric members route through the
    /// tuple-struct field helper (newtype unwrap) or positional `Fi` fallback.
    fn format_dot_access_lvalue(
        &mut self,
        base: GoExpression,
        expression_ty: &Type,
        member: &str,
        resolution: &DotAccessResolution,
    ) -> GoExpression {
        if let Ok(index) = member.parse::<usize>() {
            let access =
                self.try_emit_tuple_struct_field_access(base.clone(), expression_ty, index);
            if let Some(access) = access {
                return access;
            }
            let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
            return GoExpression::selector(base, field.to_string());
        }
        let field = if resolution_exports_field(resolution)
            || self.struct_field_is_exported(expression_ty, member)
        {
            go_name::exported_member(expression_ty, member)
        } else if self.field_is_embedded(expression_ty, member) {
            go_name::escape_keyword(member).into_owned()
        } else {
            go_name::unexported_method_go_name(member)
        };
        GoExpression::selector(base, field)
    }
}

fn is_place_expression(expression: &Expression) -> bool {
    let expression = expression.unwrap_parens();
    match expression {
        Expression::Identifier { .. }
        | Expression::DotAccess { .. }
        | Expression::IndexedAccess { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => true,
        Expression::Call { .. } => expression.get_type().is_ref(),
        _ => false,
    }
}

fn reads_through_reference(base: &Expression) -> bool {
    matches!(base.unwrap_parens(), Expression::Identifier { .. }) && base.get_type().is_ref()
}

fn resolution_exports_field(resolution: &DotAccessResolution) -> bool {
    matches!(
        resolution,
        DotAccessResolution::StructField {
            is_exported: true,
            ..
        }
    )
}

/// Recognize compound assignment: either `x += y` syntax (caller supplies
/// `compound_operator`) or the desugared `x = x + y` pattern.
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
        expression.unwrap_parens(),
        Expression::Literal {
            literal: Literal::Integer { value: 1, .. },
            ..
        }
    )
}

/// Check if two lvalue expressions refer to the same location.
/// Used to detect `x = x + y` → `x += y` patterns.
/// Compares by binding_id for identifiers, recursively for DotAccess/Deref.
/// Deliberately skips IndexedAccess (side-effect hazard from index evaluation).
pub(crate) fn lvalues_match(a: &Expression, b: &Expression) -> bool {
    let a = a.unwrap_parens();
    let b = b.unwrap_parens();
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

pub(crate) fn is_lvalue_chain(expression: &Expression) -> bool {
    let expression = expression.unwrap_parens();
    match expression {
        Expression::Identifier { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Deref,
            ..
        } => true,
        Expression::IndexedAccess { expression, .. } => is_lvalue_chain(expression),
        Expression::DotAccess { expression, .. } => is_lvalue_chain(expression),
        Expression::Call { .. } if expression.get_type().is_ref() => true,
        _ => false,
    }
}
