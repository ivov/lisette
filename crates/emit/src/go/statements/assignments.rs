use crate::Emitter;
use crate::go::control_flow::branching::wrap_if_struct_literal;
use crate::go::is_order_sensitive;
use crate::go::names::go_name;
use crate::go::types::emitter::Position;
use crate::go::write_line;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::parse::TUPLE_FIELDS;

impl Emitter<'_> {
    pub(crate) fn emit_statement(&mut self, output: &mut String, expression: &Expression) {
        if !matches!(expression, Expression::Block { .. }) {
            let span = expression.get_span();
            output.push_str(&self.maybe_line_directive(&span));
        }

        match expression {
            Expression::Let {
                binding,
                value,
                mutable,
                else_block,
                ..
            } => self.emit_let(output, binding, value, else_block.as_deref(), *mutable),
            Expression::Return { expression, .. } => {
                self.emit_return(output, expression);
            }
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                if value.get_type().is_never() {
                    self.emit_statement(output, value);
                    return;
                }

                let detected_compound = if let Some(op) = compound_operator.as_ref() {
                    Some((op, Self::compound_rhs(value)))
                } else if let Expression::Binary {
                    left,
                    operator,
                    right,
                    ..
                } = value.as_ref()
                    && is_compound_eligible(operator)
                    && lvalues_match(target, left)
                {
                    Some((operator, right.as_ref()))
                } else {
                    None
                };

                if let Some((op, rhs)) = detected_compound {
                    // false: compound RHS is emitted via emit_operand (inline),
                    // so its temp statements land in output after the target.
                    let target_str = if is_order_sensitive(target) {
                        self.emit_left_value_capturing(output, target, false)
                    } else {
                        self.emit_left_value(output, target)
                    };
                    let is_inc_dec = Self::is_literal_one(rhs)
                        && matches!(op, BinaryOperator::Addition | BinaryOperator::Subtraction);
                    if is_inc_dec {
                        let inc_op = if *op == BinaryOperator::Addition {
                            "++"
                        } else {
                            "--"
                        };
                        write_line!(output, "{}{}", target_str, inc_op);
                    } else {
                        let rhs_str = self.emit_operand(output, rhs);
                        write_line!(output, "{} {}= {}", target_str, op, rhs_str);
                    }
                    return;
                }

                let is_go_nullable = matches!(target.as_ref(), Expression::DotAccess { expression, ty, .. }
                        if Self::is_go_imported_type(&expression.get_type())
                            && self.is_go_nullable(ty));

                let rhs_staged = self.stage_composite(value);
                let rhs_has_setup = !rhs_staged.setup.is_empty();

                let target_str = if is_order_sensitive(target) {
                    self.emit_left_value_capturing(output, target, rhs_has_setup)
                } else {
                    self.emit_left_value(output, target)
                };
                output.push_str(&rhs_staged.setup);

                if is_go_nullable {
                    let unwrapped = self.maybe_unwrap_go_nullable(
                        output,
                        &rhs_staged.value,
                        &value.get_type().resolve(),
                    );
                    write_line!(output, "{} = {}", target_str, unwrapped);
                } else {
                    let adapted = self.maybe_wrap_as_go_interface(
                        rhs_staged.value,
                        &value.get_type(),
                        &target.get_type(),
                    );
                    write_line!(output, "{} = {}", target_str, adapted);
                }
            }
            Expression::Break { value, .. } => {
                if let Some(val) = value {
                    let assign_var = self.current_loop_result_var().map(str::to_string);
                    let is_unit_call = val.get_type().resolve().is_unit()
                        && matches!(val.unwrap_parens(), Expression::Call { .. });
                    let val_str = self.emit_value(output, val);

                    // When propagation (e.g. `Err(...)? / None?`) emits a direct
                    // `return`, emit_value returns "". Skip assignment and break
                    // since the function has already returned.
                    if val_str.is_empty() && matches!(**val, Expression::Propagate { .. }) {
                        return;
                    }

                    if let Some(var) = assign_var {
                        if is_unit_call {
                            if !val_str.is_empty() {
                                write_line!(output, "{}", val_str);
                            }
                            write_line!(output, "{} = struct{{}}{{}}", var);
                        } else if !val_str.is_empty() {
                            write_line!(output, "{} = {}", var, val_str);
                        }
                    } else if !val_str.is_empty() {
                        write_line!(output, "_ = {}", val_str);
                    }
                }
                if let Some(label) = self.current_loop_label() {
                    write_line!(output, "break {}", label);
                } else {
                    output.push_str("break\n");
                }
            }
            Expression::Continue { .. } => {
                if let Some(label) = self.current_loop_label() {
                    write_line!(output, "continue {}", label);
                } else {
                    output.push_str("continue\n");
                }
            }
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.with_position(Position::Statement, |this| {
                    this.emit_if(output, condition, consequence, alternative)
                });
            }
            Expression::IfLet { .. } => {
                unreachable!("IfLet should be desugared to Match before emit")
            }
            Expression::Match {
                subject, arms, ty, ..
            } => {
                self.with_position(Position::Statement, |this| {
                    this.emit_match(output, subject, arms, ty)
                });
            }
            Expression::Loop {
                body, needs_label, ..
            } => {
                self.push_loop("_");
                self.emit_labeled_loop(output, "for {\n", body, *needs_label);
                self.pop_loop();
            }
            Expression::While {
                condition,
                body,
                needs_label,
                ..
            } => {
                self.push_loop("_");
                let pre_len = output.len();
                let cond = self.emit_condition_operand(output, condition);
                let has_setup = output.len() > pre_len;
                if has_setup {
                    // Condition produced setup statements (temps) — they must
                    // re-run each iteration, so move everything inside the loop.
                    let setup = output[pre_len..].to_string();
                    output.truncate(pre_len);
                    let header = format!("for {{\n{}if !({}) {{ break }}\n", setup, cond);
                    self.emit_labeled_loop(output, &header, body, *needs_label);
                } else if matches!(
                    condition.unwrap_parens(),
                    Expression::Literal {
                        literal: Literal::Boolean(true),
                        ..
                    }
                ) {
                    self.emit_labeled_loop(output, "for {\n", body, *needs_label);
                } else {
                    let cond = wrap_if_struct_literal(cond);
                    self.emit_labeled_loop(
                        output,
                        &format!("for {} {{\n", cond),
                        body,
                        *needs_label,
                    );
                }
                self.pop_loop();
            }
            Expression::WhileLet {
                pattern,
                typed_pattern,
                scrutinee,
                body,
                needs_label,
                ..
            } => {
                self.push_loop("_");
                self.emit_while_let(
                    output,
                    pattern,
                    typed_pattern.as_ref(),
                    scrutinee,
                    body,
                    *needs_label,
                );
                self.pop_loop();
            }
            Expression::For {
                binding,
                iterable,
                body,
                needs_label,
                ..
            } => {
                self.push_loop("_");
                self.emit_for_loop(output, binding, iterable, body, *needs_label);
                self.pop_loop();
            }
            Expression::Select { arms, .. } => {
                self.with_position(Position::Statement, |this| this.emit_select(output, arms));
            }
            Expression::Block { .. } => {
                output.push_str("{\n");
                self.enter_scope();
                self.emit_block(output, expression);
                self.exit_scope();
                output.push_str("}\n");
            }
            Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::ValueEnum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::ImplBlock { .. } => {
                let code = self.emit_top_item(expression);
                if !code.is_empty() {
                    output.push_str(&code);
                    output.push('\n');
                }
            }
            Expression::Const {
                identifier,
                expression: value,
                ty,
                ..
            } => {
                let code = self.emit_const(identifier, value, ty);
                output.push_str(&code);
                output.push('\n');
            }
            _ => {
                let is_call = matches!(
                    expression.unwrap_parens(),
                    Expression::Call { .. } | Expression::Task { .. } | Expression::Defer { .. }
                );
                let unwrapped = expression.unwrap_parens();
                let emitted = if let Expression::Call { .. } = unwrapped
                    && let Some(raw) = self.emit_go_call_discarded(output, unwrapped)
                {
                    raw
                } else if is_call {
                    self.emit_operand(output, unwrapped)
                } else {
                    self.emit_operand(output, expression)
                };
                if !emitted.is_empty() {
                    if is_call && !emitted.starts_with("append(") {
                        write_line!(output, "{}", emitted);
                    } else if emitted != "struct{}{}" {
                        write_line!(output, "_ = {}", emitted);
                    }
                }
            }
        }
    }

    pub(crate) fn emit_left_value(
        &mut self,
        output: &mut String,
        expression: &Expression,
    ) -> String {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::Identifier { value, .. } => self
                .scope
                .bindings
                .get(value)
                .map(|s| s.to_string())
                .unwrap_or_else(|| value.to_string()),
            Expression::DotAccess {
                expression, member, ..
            } => {
                let expression_string = if let Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } = expression.as_ref()
                {
                    self.emit_operand(output, inner)
                } else {
                    self.emit_operand(output, expression)
                };
                let expression_ty = expression.get_type().resolve();

                if let Ok(index) = member.parse::<usize>() {
                    if let Some(access) = self.try_emit_tuple_struct_field_access(
                        &expression_string,
                        &expression_ty,
                        index,
                    ) {
                        return access;
                    }
                    let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
                    return format!("{}.{}", expression_string, field);
                }

                let field = if self.field_is_public(&expression_ty, member) {
                    go_name::make_exported(member)
                } else {
                    go_name::escape_keyword(member).into_owned()
                };
                format!("{}.{}", expression_string, field)
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                let expression_string = if let Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } = expression.as_ref()
                {
                    let inner_str = self.emit_operand(output, inner);
                    format!("(*{})", inner_str)
                } else {
                    self.emit_operand(output, expression)
                };
                let index_str = self.emit_operand(output, index);
                format!("{}[{}]", expression_string, index_str)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression,
                ..
            } => {
                let expression_string = self.emit_operand(output, expression);
                if matches!(expression.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, expression_string);
                    format!("*{}", tmp)
                } else {
                    format!("*{}", expression_string)
                }
            }
            Expression::Call { .. } if expression.get_type().resolve().is_ref() => {
                let call_str = self.emit_operand(output, expression);
                let tmp = self.fresh_var(Some("ref"));
                self.declare(&tmp);
                write_line!(output, "{} := {}", tmp, call_str);
                tmp
            }
            _ => "_".to_string(),
        }
    }

    /// Emit a left-value, capturing side-effecting subexpressions (index, base)
    /// to temp vars so they evaluate before any RHS temps, but leaving the
    /// structural lvalue intact (so assigning to it mutates the original).
    pub(crate) fn emit_left_value_capturing(
        &mut self,
        output: &mut String,
        expression: &Expression,
        rhs_has_setup: bool,
    ) -> String {
        let expression = expression.unwrap_parens();
        match expression {
            Expression::IndexedAccess {
                expression: base,
                index,
                ..
            } => {
                let base_str = if is_order_sensitive(base) {
                    if let Expression::Unary {
                        operator: UnaryOperator::Deref,
                        expression: inner,
                        ..
                    } = base.as_ref()
                    {
                        let inner_str = self.emit_force_capture(output, inner, "base");
                        format!("(*{})", inner_str)
                    } else {
                        self.emit_force_capture(output, base, "base")
                    }
                } else if let Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } = base.as_ref()
                {
                    let inner_str = self.emit_operand(output, inner);
                    format!("(*{})", inner_str)
                } else {
                    self.emit_operand(output, base)
                };
                // When the RHS produces temp statements (if/match/block used as value),
                // the index must be captured even for simple identifiers — the RHS
                // setup (emitted later) could mutate the index variable.
                let index_needs_capture = if rhs_has_setup {
                    !matches!(index.unwrap_parens(), Expression::Literal { .. })
                } else {
                    is_order_sensitive(index)
                };
                let index_str = if index_needs_capture {
                    self.emit_force_capture(output, index, "idx")
                } else {
                    self.emit_operand(output, index)
                };
                format!("{}[{}]", base_str, index_str)
            }
            Expression::DotAccess {
                expression: base,
                member,
                ..
            } => {
                let base_str = if let Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } = base.as_ref()
                {
                    self.emit_operand(output, inner)
                } else if is_order_sensitive(base) {
                    self.emit_left_value_capturing(output, base, rhs_has_setup)
                } else {
                    self.emit_left_value(output, base)
                };
                let expression_ty = base.get_type().resolve();
                if let Ok(index) = member.parse::<usize>() {
                    if let Some(access) =
                        self.try_emit_tuple_struct_field_access(&base_str, &expression_ty, index)
                    {
                        return access;
                    }
                    let field = TUPLE_FIELDS.get(index).expect("oversize tuple arity");
                    return format!("{}.{}", base_str, field);
                }
                let field = if self.field_is_public(&expression_ty, member) {
                    go_name::make_exported(member)
                } else {
                    go_name::escape_keyword(member).into_owned()
                };
                format!("{}.{}", base_str, field)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner,
                ..
            } => {
                let inner_str = self.emit_operand(output, inner);
                if matches!(inner.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, inner_str);
                    format!("*{}", tmp)
                } else {
                    format!("*{}", inner_str)
                }
            }
            _ => self.emit_left_value(output, expression),
        }
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
        Expression::Call { .. } if expression.get_type().resolve().is_ref() => true,
        _ => false,
    }
}
