use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::context::expression::ExpressionContext;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::plan::bodies::{AssignForm, AssignPlan, CompoundKind, LoweredBlock, LoweredStatement};
use crate::plan::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::state::bindings::BindingValue;
use syntax::ast::{BinaryOperator, Expression, UnaryOperator};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

impl Planner<'_> {
    /// Build an `AssignPlan`, dispatching on shape: never-typed, compound,
    /// discard, or simple `target = value`.
    pub(crate) fn build_assignment_plan(
        &mut self,
        target: &Expression,
        value: &Expression,
        compound_operator: Option<&BinaryOperator>,
    ) -> AssignPlan {
        let raw_body = |statements: Vec<LoweredStatement>| LoweredBlock { statements };

        if value.get_type().is_never() {
            return AssignPlan {
                form: AssignForm::NeverTyped {
                    body: raw_body(vec![self.lower_statement(value)]),
                },
            };
        }

        if let Some((op, rhs)) = detect_compound_assignment(target, value, compound_operator) {
            return self.build_compound_assignment_plan(target, op, rhs);
        }

        if self.target_binds_to_discard(target) {
            return AssignPlan {
                form: AssignForm::Discard {
                    body: raw_body(self.lower_discard_value(value)),
                },
            };
        }

        let go_field_slot: Option<(Type, ValueLayout)> = match target {
            Expression::DotAccess {
                expression,
                member,
                ty,
                ..
            } => self
                .field_slot_layout(&expression.get_type(), member, ty)
                .map(|layout| (ty.clone(), layout)),
            _ => None,
        };

        // `target = value`. Stage RHS first (so the target capture knows
        // whether RHS produced setup), capture the target, then fold RHS
        // setup + coercion setup into the value plan in emission order.
        let right_hand_side = self.stage_composite(value, ExpressionContext::value());
        let (target_capture, target_str) =
            self.capture_assignment_target(target, Some(&right_hand_side));
        let coercion = if let Some((_target_ty, target_layout)) = go_field_slot {
            let source_layout = self.value_layout(&value.get_type(), SlotOrigin::Lisette);
            CoercionPlan::bridge(self, &source_layout, &target_layout)
        } else {
            CoercionPlan::internal(self, &value.get_type(), &target.get_type())
        };
        let value = right_hand_side.map_rendered_as_computed(
            |value_setup, rhs_value, contains_deferred_evaluation| {
                let (coercion_setup, final_value) = coercion.lower(self, rhs_value);
                value_setup.extend(coercion_setup);
                GoExpression::opaque_with_deferred_evaluation(
                    final_value,
                    contains_deferred_evaluation,
                )
            },
        );
        AssignPlan {
            form: AssignForm::Simple {
                target_capture,
                target_str,
                value,
            },
        }
    }

    /// Build a compound assignment plan (`+=`, `-=`, `++`, etc.), staging the
    /// right-hand side and capturing the target in evaluation order.
    fn build_compound_assignment_plan(
        &mut self,
        target: &Expression,
        op: &BinaryOperator,
        rhs: &Expression,
    ) -> AssignPlan {
        let is_inc_dec = is_literal_one(rhs)
            && matches!(op, BinaryOperator::Addition | BinaryOperator::Subtraction);
        if is_inc_dec {
            let kind = if *op == BinaryOperator::Addition {
                CompoundKind::Increment
            } else {
                CompoundKind::Decrement
            };
            let (target_capture, target_str) = self.capture_assignment_target(target, None);
            return AssignPlan {
                form: AssignForm::Compound {
                    target_capture,
                    target_str,
                    kind,
                },
            };
        }

        let right_hand_side = self.stage_operand(rhs, ExpressionContext::value());
        let right_hand_side_has_setup = !right_hand_side.setup.is_empty();
        let right_hand_side_has_effectful_call =
            right_hand_side.evaluation.effect.has_effectful_call();
        let (mut target_capture, target_str) =
            self.capture_assignment_target(target, Some(&right_hand_side));
        let needs_left_pin = right_hand_side_has_setup
            || (right_hand_side_has_effectful_call
                && !self.identifier_immune_to_calls(target.unwrap_parens()));
        let pinned_left = needs_left_pin.then(|| {
            let tmp = self.fresh_var(Some("_left"));
            self.declare(&tmp);
            target_capture.push(LoweredStatement::TempBind {
                name: tmp.clone(),
                value: target_str.clone(),
            });
            tmp
        });
        let parenthesize_rhs =
            pinned_left.is_some() && matches!(rhs.unwrap_parens(), Expression::Binary { .. });
        let mut right_hand_side =
            right_hand_side.map_rendered(|_, staged_value, contains_deferred_evaluation| {
                let rhs_value = if parenthesize_rhs {
                    format!("({})", staged_value)
                } else {
                    staged_value
                };
                GoExpression::opaque_with_deferred_evaluation(
                    rhs_value,
                    contains_deferred_evaluation,
                )
            });
        if parenthesize_rhs {
            right_hand_side.make_observable_computed();
        }
        let kind = CompoundKind::OpAssign {
            op_text: format!("{}", op),
            rhs: right_hand_side,
            pinned_left,
        };
        AssignPlan {
            form: AssignForm::Compound {
                target_capture,
                target_str,
                kind,
            },
        }
    }

    /// Emit a left-value target, capturing order-sensitive sub-expressions into
    /// preceding statements when the target reads must be pinned before the RHS.
    fn capture_assignment_target(
        &mut self,
        target: &Expression,
        right_hand_side: Option<&ValuePlan>,
    ) -> (Vec<LoweredStatement>, String) {
        let mut target_capture: Vec<LoweredStatement> = Vec::new();
        let target_str = if is_order_sensitive(target) {
            self.emit_left_value_capturing(&mut target_capture, target, right_hand_side)
        } else {
            self.emit_left_value(&mut target_capture, target)
        };
        (target_capture, target_str)
    }

    fn target_binds_to_discard(&self, target: &Expression) -> bool {
        let Expression::Identifier { value, .. } = target.unwrap_parens() else {
            return false;
        };
        match self.scope.resolve_identifier_binding(value) {
            Some(BindingValue::GoName(go_name)) => go_name == "_",
            Some(BindingValue::InlineExpr(_)) => false,
            None => value == "_",
        }
    }

    pub(crate) fn emit_left_value(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
    ) -> String {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::Identifier { value, .. } => self
                .scope
                .resolve_binding_go_name(value)
                .unwrap_or(value)
                .to_string(),
            Expression::DotAccess {
                expression, member, ..
            } => {
                let base = expression.deref_inner().unwrap_or(expression);
                let base_str = self.capture_operand_into(setup, base);
                let expression_ty = expression.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member)
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                let expression_string = if let Some(inner) = expression.deref_inner() {
                    let inner_str = self.capture_operand_into(setup, inner);
                    format!("(*{})", inner_str)
                } else {
                    self.capture_operand_into(setup, expression)
                };
                let index_str = self.capture_operand_into(setup, index);
                format!("{}[{}]", expression_string, index_str)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => self.emit_deref_lvalue(setup, expression, false),
            Expression::Call { .. } if expression.get_type().is_ref() => {
                let call_str = self.capture_operand_into(setup, expression);
                self.hoist_tmp_value_statement(setup, "ref", &call_str)
            }
            _ => "_".to_string(),
        }
    }

    /// Emit `*X` lvalue form, capturing the pointee into a temp if it's a
    /// call (Go requires an addressable operand for deref-assignment) or when
    /// RHS setup could reassign the pointer before the write executes.
    fn emit_deref_lvalue(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        pointee: &Expression,
        rhs_has_setup: bool,
    ) -> String {
        let pointee_plan = self.plan_operand(pointee, ExpressionContext::value());
        let pointee_is_observable = pointee_plan.evaluation.stability.is_observable();
        let (pointee_setup, pointee_string) = pointee_plan.into_parts();
        setup.extend(pointee_setup);
        let needs_capture = matches!(pointee.unwrap_parens(), Expression::Call { .. })
            || (rhs_has_setup && pointee_is_observable);
        if needs_capture {
            let tmp = self.hoist_tmp_value_statement(setup, "ref", &pointee_string);
            return format!("*{}", tmp);
        }
        format!("*{}", pointee_string)
    }

    /// Format a dot-access lvalue (struct field or tuple element) onto the
    /// already-emitted base expression. Numeric members route through the
    /// tuple-struct field helper (newtype unwrap) or positional `Fi` fallback.
    fn format_dot_access_lvalue(
        &mut self,
        base_str: &str,
        expression_ty: &Type,
        member: &str,
    ) -> String {
        if let Ok(index) = member.parse::<usize>() {
            let access = self.try_emit_tuple_struct_field_access(base_str, expression_ty, index);
            if let Some(access) = access {
                return access;
            }
            let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
            return format!("{}.{}", base_str, field);
        }
        let field = if self.struct_field_is_exported(expression_ty, member) {
            go_name::make_exported(member)
        } else {
            go_name::escape_keyword(member).into_owned()
        };
        format!("{}.{}", base_str, field)
    }

    /// Emit a left-value, capturing side-effecting subexpressions (index, base)
    /// to temp vars so they evaluate before any RHS temps, but leaving the
    /// structural lvalue intact (so assigning to it mutates the original).
    pub(crate) fn emit_left_value_capturing(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        right_hand_side: Option<&ValuePlan>,
    ) -> String {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => {
                if assignment_requires_target_capture(right_hand_side) {
                    let base_str = self.emit_indexed_base_lvalue(setup, base, right_hand_side);
                    let index_str = self.capture_value_at_boundary(
                        setup,
                        index,
                        "idx",
                        CaptureBoundary::AssignmentRightHandSide,
                    );
                    format!("{}[{}]", base_str, index_str)
                } else {
                    self.emit_indexed_lvalue_inline(setup, base, index)
                }
            }
            Expression::DotAccess {
                expression: base,
                member,
                ..
            } => {
                let base_str = if let Some(inner) = base.deref_inner() {
                    if assignment_requires_target_capture(right_hand_side) {
                        self.capture_value_at_boundary(
                            setup,
                            inner,
                            "ref",
                            CaptureBoundary::AssignmentRightHandSide,
                        )
                    } else {
                        self.capture_operand_into(setup, inner)
                    }
                } else if is_order_sensitive(base) {
                    self.emit_left_value_capturing(setup, base, right_hand_side)
                } else if assignment_requires_target_capture(right_hand_side)
                    && base.get_type().is_ref()
                {
                    self.capture_value_at_boundary(
                        setup,
                        base,
                        "ref",
                        CaptureBoundary::AssignmentRightHandSide,
                    )
                } else {
                    self.emit_left_value(setup, base)
                };
                let expression_ty = base.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner,
                ..
            } => self.emit_deref_lvalue(
                setup,
                inner,
                assignment_requires_target_capture(right_hand_side),
            ),
            _ => self.emit_left_value(setup, expression),
        }
    }

    fn emit_indexed_lvalue_inline(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        base: &Expression,
        index: &Expression,
    ) -> String {
        let base_staged = self.stage_base_with_deref(base);
        let (seq_setup, value) = self
            .sequence_indexed_access(base, base_staged, index, "base")
            .into_parts();
        setup.extend(seq_setup);
        value
    }

    fn emit_indexed_base_lvalue(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        base: &Expression,
        right_hand_side: Option<&ValuePlan>,
    ) -> String {
        if let Some(inner) = base.deref_inner() {
            let inner_str = self.emit_base_operand(setup, inner, true);
            format!("(*{})", inner_str)
        } else {
            let base_plan = self.stage_composite(base, ExpressionContext::value());
            let force = base_plan.evaluation.stability.is_observable()
                && (assignment_has_setup(right_hand_side)
                    || !self.identifier_immune_to_calls(base.unwrap_parens()));
            let (base_setup, base_value) = base_plan.into_parts();
            setup.extend(base_setup);
            if force {
                return self.hoist_tmp_value_statement(setup, "base", &base_value);
            }
            base_value
        }
    }

    fn emit_base_operand(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        expression: &Expression,
        force_capture: bool,
    ) -> String {
        if force_capture {
            self.capture_value_at_boundary(
                setup,
                expression,
                "base",
                CaptureBoundary::AssignmentRightHandSide,
            )
        } else {
            self.capture_operand_into(setup, expression)
        }
    }
}

fn assignment_has_setup(right_hand_side: Option<&ValuePlan>) -> bool {
    right_hand_side.is_some_and(|value| !value.setup.is_empty())
}

fn assignment_has_effectful_call(right_hand_side: Option<&ValuePlan>) -> bool {
    right_hand_side.is_some_and(|value| value.evaluation.effect.has_effectful_call())
}

fn assignment_requires_target_capture(right_hand_side: Option<&ValuePlan>) -> bool {
    assignment_has_setup(right_hand_side) || assignment_has_effectful_call(right_hand_side)
}

/// Recognize compound assignment — either `x += y` syntax (caller supplies
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
            literal: syntax::ast::Literal::Integer { value: 1, .. },
            ..
        }
    )
}

/// Check if two lvalue expressions refer to the same location.
/// Used to detect `x = x + y` → `x += y` patterns.
/// Compares by binding_id for identifiers, recursively for DotAccess/Deref.
/// Deliberately skips IndexedAccess (side-effect hazard from index evaluation).
fn lvalues_match(a: &Expression, b: &Expression) -> bool {
    let a = a.unwrap_parens();
    let b = b.unwrap_parens();
    match (a, b) {
        (
            Expression::Identifier {
                binding_id: Some(id_a),
                ..
            },
            Expression::Identifier {
                binding_id: Some(id_b),
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
