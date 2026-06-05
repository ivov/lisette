use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::plan::bodies::{AssignForm, AssignPlan, CompoundKind, LoweredBlock, LoweredStatement};
use crate::plan::values::value_plan_from_statements;
use crate::state::bindings::BindingValue;
use crate::utils::observable_after_mutation;
use syntax::ast::{BinaryOperator, Expression, FormatStringPart, Literal, UnaryOperator};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

impl Planner<'_> {
    /// String-context bridge over `lower_statement`.
    pub(crate) fn emit_statement(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) {
        let statement = self.lower_statement(expression, fx);
        let block = LoweredBlock {
            statements: vec![statement],
        };
        Renderer.render_lowered_block(output, &block);
    }

    /// Build an `AssignPlan`, dispatching on shape: never-typed, compound,
    /// discard, Go-nullable-field clear, or simple `target = value`.
    pub(crate) fn build_assignment_plan(
        &mut self,
        target: &Expression,
        value: &Expression,
        compound_operator: Option<&BinaryOperator>,
        directive: String,
        fx: &mut EmitEffects,
    ) -> AssignPlan {
        let raw_body = |statements: Vec<LoweredStatement>| LoweredBlock { statements };
        let capture_to_vec = |buffer: String| {
            if buffer.is_empty() {
                Vec::new()
            } else {
                vec![LoweredStatement::RawGo(buffer)]
            }
        };

        if value.get_type().is_never() {
            let mut buffer = String::new();
            self.emit_statement(&mut buffer, value, fx);
            return AssignPlan {
                directive,
                form: AssignForm::NeverTyped {
                    body: raw_body(vec![LoweredStatement::RawGo(buffer)]),
                },
            };
        }

        if let Some((op, rhs)) = detect_compound_assignment(target, value, compound_operator) {
            let is_inc_dec = is_literal_one(rhs)
                && matches!(op, BinaryOperator::Addition | BinaryOperator::Subtraction);
            let (kind, rhs_has_setup) = if is_inc_dec {
                let kind = if *op == BinaryOperator::Addition {
                    CompoundKind::Increment
                } else {
                    CompoundKind::Decrement
                };
                (kind, false)
            } else {
                let staged = self.stage_operand(rhs, ExpressionContext::value(), fx);
                let rhs_has_setup =
                    !staged.setup.is_empty() || self.rhs_contains_effectful_call(rhs);
                let kind = CompoundKind::OpAssign {
                    op_text: format!("{}", op),
                    rhs: value_plan_from_statements(staged.setup, staged.value),
                };
                (kind, rhs_has_setup)
            };
            let mut target_setup = String::new();
            let target_str = if is_order_sensitive(target) {
                self.emit_left_value_capturing(&mut target_setup, target, rhs_has_setup, fx)
            } else {
                self.emit_left_value(&mut target_setup, target, fx)
            };
            let target_capture = capture_to_vec(target_setup);
            return AssignPlan {
                directive,
                form: AssignForm::Compound {
                    target_capture,
                    target_str,
                    kind,
                },
            };
        }

        if self.target_binds_to_discard(target) {
            let mut buffer = String::new();
            self.emit_discard(&mut buffer, value, fx);
            return AssignPlan {
                directive,
                form: AssignForm::Discard {
                    body: raw_body(vec![LoweredStatement::RawGo(buffer)]),
                },
            };
        }

        let go_field_ty: Option<Type> = match target {
            Expression::DotAccess { expression, ty, .. }
                if self.go_imported_shape(&expression.get_type()).is_some()
                    && (self.is_go_nullable(ty) || ty.resolves_to_unknown()) =>
            {
                Some(ty.clone())
            }
            _ => None,
        };

        if let Some(ref target_ty) = go_field_ty
            && target_ty.resolves_to_unknown()
            && value.is_none_literal()
        {
            let mut target_setup = String::new();
            let target_str = if is_order_sensitive(target) {
                self.emit_left_value_capturing(&mut target_setup, target, false, fx)
            } else {
                self.emit_left_value(&mut target_setup, target, fx)
            };
            return AssignPlan {
                directive,
                form: AssignForm::NilClear {
                    target_capture: capture_to_vec(target_setup),
                    target_str,
                },
            };
        }

        // `target = value`. Stage RHS first (so the target capture knows
        // whether RHS produced setup), capture the target, then fold RHS
        // setup + coercion setup into the value plan in emission order.
        let rhs_staged = self.stage_composite(value, ExpressionContext::value(), fx);
        let rhs_has_setup = !rhs_staged.setup.is_empty() || self.rhs_contains_effectful_call(value);
        let mut target_setup = String::new();
        let target_str = if is_order_sensitive(target) {
            self.emit_left_value_capturing(&mut target_setup, target, rhs_has_setup, fx)
        } else {
            self.emit_left_value(&mut target_setup, target, fx)
        };
        let mut value_setup = rhs_staged.setup;
        let coercion = if let Some(target_ty) = go_field_ty {
            Coercion::resolve(
                self,
                &value.get_type(),
                &target_ty,
                CoercionDirection::ToGoBoundary,
            )
        } else {
            Coercion::resolve(
                self,
                &value.get_type(),
                &target.get_type(),
                CoercionDirection::Internal,
            )
        };
        let (coercion_setup, final_value) = coercion.lower(self, rhs_staged.value, fx);
        value_setup.extend(coercion_setup);
        AssignPlan {
            directive,
            form: AssignForm::Simple {
                target_capture: capture_to_vec(target_setup),
                target_str,
                value: value_plan_from_statements(value_setup, final_value),
            },
        }
    }

    fn rhs_contains_effectful_call(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Call {
                expression: callee,
                args,
                spread,
                ..
            } => {
                if self.is_pure_constructor_callee(callee) {
                    args.iter().any(|a| self.rhs_contains_effectful_call(a))
                        || (**spread)
                            .as_ref()
                            .is_some_and(|s| self.rhs_contains_effectful_call(s))
                } else {
                    true
                }
            }
            Expression::Binary { left, right, .. } => {
                self.rhs_contains_effectful_call(left) || self.rhs_contains_effectful_call(right)
            }
            Expression::Unary { expression, .. }
            | Expression::DotAccess { expression, .. }
            | Expression::Cast { expression, .. }
            | Expression::Reference { expression, .. } => {
                self.rhs_contains_effectful_call(expression)
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.rhs_contains_effectful_call(expression)
                    || self.rhs_contains_effectful_call(index)
            }
            Expression::Tuple { elements, .. } => {
                elements.iter().any(|e| self.rhs_contains_effectful_call(e))
            }
            Expression::StructCall {
                field_assignments,
                spread,
                ..
            } => {
                field_assignments
                    .iter()
                    .any(|f| self.rhs_contains_effectful_call(&f.value))
                    || spread
                        .as_expression()
                        .is_some_and(|s| self.rhs_contains_effectful_call(s))
            }
            Expression::Literal {
                literal: Literal::Slice(elements),
                ..
            } => elements.iter().any(|e| self.rhs_contains_effectful_call(e)),
            Expression::Literal {
                literal: Literal::FormatString(parts),
                ..
            } => parts.iter().any(|part| match part {
                FormatStringPart::Expression(e) => self.rhs_contains_effectful_call(e),
                FormatStringPart::Text(_) => false,
            }),
            Expression::Range { start, end, .. } => {
                start
                    .as_deref()
                    .is_some_and(|e| self.rhs_contains_effectful_call(e))
                    || end
                        .as_deref()
                        .is_some_and(|e| self.rhs_contains_effectful_call(e))
            }
            _ => false,
        }
    }

    fn is_pure_constructor_callee(&self, callee: &Expression) -> bool {
        let name = match callee.unwrap_parens() {
            Expression::Identifier { value, .. } => Some(value.as_str()),
            Expression::DotAccess { member, .. } => Some(member.as_str()),
            _ => None,
        };
        if matches!(name, Some("Some" | "Ok" | "Err" | "None")) {
            return true;
        }
        self.callee_definition(callee)
            .is_some_and(|definition| definition.is_type_definition())
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
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
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
                let base_str = if let Some(inner) = expression.deref_inner() {
                    self.emit_operand(output, inner, ExpressionContext::value(), fx)
                } else {
                    self.emit_operand(output, expression, ExpressionContext::value(), fx)
                };
                let expression_ty = expression.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member, fx)
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                let expression_string = if let Some(inner) = expression.deref_inner() {
                    let inner_str =
                        self.emit_operand(output, inner, ExpressionContext::value(), fx);
                    format!("(*{})", inner_str)
                } else {
                    self.emit_operand(output, expression, ExpressionContext::value(), fx)
                };
                let index_str = self.emit_operand(output, index, ExpressionContext::value(), fx);
                format!("{}[{}]", expression_string, index_str)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => self.emit_deref_lvalue(output, expression, false, fx),
            Expression::Call { .. } if expression.get_type().is_ref() => {
                let call_str =
                    self.emit_operand(output, expression, ExpressionContext::value(), fx);
                self.hoist_tmp_value(output, "ref", &call_str)
            }
            _ => "_".to_string(),
        }
    }

    /// Emit `*X` lvalue form, capturing the pointee into a temp if it's a
    /// call (Go requires an addressable operand for deref-assignment) or when
    /// RHS setup could reassign the pointer before the write executes.
    fn emit_deref_lvalue(
        &mut self,
        output: &mut String,
        pointee: &Expression,
        rhs_has_setup: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let pointee_string = self.emit_operand(output, pointee, ExpressionContext::value(), fx);
        let needs_capture = matches!(pointee.unwrap_parens(), Expression::Call { .. })
            || (rhs_has_setup && observable_after_mutation(pointee));
        if needs_capture {
            let tmp = self.hoist_tmp_value(output, "ref", &pointee_string);
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
        fx: &mut EmitEffects,
    ) -> String {
        if let Ok(index) = member.parse::<usize>() {
            let access =
                self.try_emit_tuple_struct_field_access(base_str, expression_ty, index, fx);
            if let Some(access) = access {
                return access;
            }
            let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
            return format!("{}.{}", base_str, field);
        }
        let field = if self.field_is_public(expression_ty, member) {
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
        output: &mut String,
        expression: &Expression,
        rhs_has_setup: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => {
                let base_str = self.emit_indexed_base_lvalue(output, base, fx);
                let index_str = self.emit_index_lvalue(output, index, rhs_has_setup, fx);
                format!("{}[{}]", base_str, index_str)
            }
            Expression::DotAccess {
                expression: base,
                member,
                ..
            } => {
                let base_str = if let Some(inner) = base.deref_inner() {
                    if rhs_has_setup {
                        self.emit_force_capture(output, inner, "ref", fx)
                    } else {
                        self.emit_operand(output, inner, ExpressionContext::value(), fx)
                    }
                } else if is_order_sensitive(base) {
                    self.emit_left_value_capturing(output, base, rhs_has_setup, fx)
                } else if rhs_has_setup && base.get_type().is_ref() {
                    self.emit_force_capture(output, base, "ref", fx)
                } else {
                    self.emit_left_value(output, base, fx)
                };
                let expression_ty = base.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member, fx)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner,
                ..
            } => self.emit_deref_lvalue(output, inner, rhs_has_setup, fx),
            _ => self.emit_left_value(output, expression, fx),
        }
    }

    /// Emit the base of an `IndexedAccess` lvalue, peeling an explicit deref
    /// so `(*ptr)[i]` evaluates the pointer separately from the index, and
    /// capturing the base to a temp when ordering matters.
    fn emit_indexed_base_lvalue(
        &mut self,
        output: &mut String,
        base: &Expression,
        fx: &mut EmitEffects,
    ) -> String {
        let force = is_order_sensitive(base);
        if let Some(inner) = base.deref_inner() {
            let inner_str = self.emit_base_operand(output, inner, force, fx);
            format!("(*{})", inner_str)
        } else {
            self.emit_base_operand(output, base, force, fx)
        }
    }

    fn emit_base_operand(
        &mut self,
        output: &mut String,
        expression: &Expression,
        force_capture: bool,
        fx: &mut EmitEffects,
    ) -> String {
        if force_capture {
            self.emit_force_capture(output, expression, "base", fx)
        } else {
            self.emit_operand(output, expression, ExpressionContext::value(), fx)
        }
    }

    /// Emit the index of an `IndexedAccess` lvalue, capturing to a temp when
    /// the RHS will emit setup statements that could mutate the index variable,
    /// or when the index expression itself is order-sensitive.
    fn emit_index_lvalue(
        &mut self,
        output: &mut String,
        index: &Expression,
        rhs_has_setup: bool,
        fx: &mut EmitEffects,
    ) -> String {
        let needs_capture = if rhs_has_setup {
            !matches!(index.unwrap_parens(), Expression::Literal { .. })
        } else {
            is_order_sensitive(index)
        };
        if needs_capture {
            self.emit_force_capture(output, index, "idx", fx)
        } else {
            self.emit_operand(output, index, ExpressionContext::value(), fx)
        }
    }
}

/// Recognize compound assignment — either `x += y` syntax (caller supplies
/// `compound_operator`) or the desugared `x = x + y` pattern.
fn detect_compound_assignment<'a>(
    target: &Expression,
    value: &'a Expression,
    compound_operator: Option<&'a BinaryOperator>,
) -> Option<(&'a BinaryOperator, &'a Expression)> {
    if let Some(op) = compound_operator {
        return Some((op, compound_rhs(value)));
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

/// Extract the original RHS from a desugared compound assignment.
/// `x += rhs` is parsed as `Assignment { value: Binary(x, +, rhs), .. }`.
fn compound_rhs(value: &Expression) -> &Expression {
    if let Expression::Binary { right, .. } = value {
        right
    } else {
        value
    }
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
