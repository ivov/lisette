use crate::Emitter;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::types::coercion::Coercion;
use crate::types::emitter::Position;
use crate::write_line;
use syntax::ast::{BinaryOperator, Expression, Literal, UnaryOperator};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

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
            } => self.emit_assignment_statement(output, target, value, compound_operator.as_ref()),
            Expression::Break { value, .. } => {
                self.emit_break_statement(output, value.as_deref());
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
            } => self.emit_while_statement(output, condition, body, *needs_label),
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

    fn emit_assignment_statement(
        &mut self,
        output: &mut String,
        target: &Expression,
        value: &Expression,
        compound_operator: Option<&BinaryOperator>,
    ) {
        if value.get_type().is_never() {
            self.emit_statement(output, value);
            return;
        }

        if let Some((op, rhs)) = Self::detect_compound_assignment(target, value, compound_operator)
        {
            self.emit_compound_assignment(output, target, op, rhs);
            return;
        }

        self.emit_simple_assignment(output, target, value);
    }

    /// Recognize compound assignment — either `x += y` syntax (caller supplies
    /// `compound_operator`) or the desugared `x = x + y` pattern where lvalue
    /// equality on both sides lets us collapse to `x += y`.
    fn detect_compound_assignment<'a>(
        target: &Expression,
        value: &'a Expression,
        compound_operator: Option<&'a BinaryOperator>,
    ) -> Option<(&'a BinaryOperator, &'a Expression)> {
        if let Some(op) = compound_operator {
            return Some((op, Self::compound_rhs(value)));
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

    fn emit_compound_assignment(
        &mut self,
        output: &mut String,
        target: &Expression,
        op: &BinaryOperator,
        rhs: &Expression,
    ) {
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
    }

    fn target_binds_to_discard(&self, target: &Expression) -> bool {
        let Expression::Identifier { value, .. } = target.unwrap_parens() else {
            return false;
        };
        match self.scope.bindings.get(value) {
            Some(go_name) => go_name == "_",
            None => value == "_",
        }
    }

    fn emit_simple_assignment(
        &mut self,
        output: &mut String,
        target: &Expression,
        value: &Expression,
    ) {
        // `_ = expr` routes through `emit_discard`, which knows how to
        // drop a lowered multi-return as a side-effect statement.
        if self.target_binds_to_discard(target) {
            self.emit_discard(output, value);
            return;
        }

        let is_go_nullable = matches!(target, Expression::DotAccess { expression, ty, .. }
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
            let coercion = Coercion::resolve_unwrap_go_nullable(self, &value.get_type());
            let unwrapped = coercion.apply(self, output, rhs_staged.value);
            write_line!(output, "{} = {}", target_str, unwrapped);
        } else {
            let coercion = Coercion::resolve(self, &value.get_type(), &target.get_type());
            let adapted = coercion.apply(self, output, rhs_staged.value);
            write_line!(output, "{} = {}", target_str, adapted);
        }
    }

    fn emit_break_statement(&mut self, output: &mut String, value: Option<&Expression>) {
        if let Some(val) = value {
            let val_str = self.emit_value(output, val);
            // When propagation (e.g. `Err(...)? / None?`) emits a direct `return`,
            // emit_value returns "". Skip assignment and break since the function
            // has already returned.
            if val_str.is_empty() && matches!(val, Expression::Propagate { .. }) {
                return;
            }
            self.bind_break_value(output, val, &val_str);
        }
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "break {}", label);
        } else {
            output.push_str("break\n");
        }
    }

    /// Bind a `break` value to the enclosing loop's result var, or discard it.
    /// Unit-typed calls are emitted as a statement before the `struct{}{}` store
    /// to preserve side effects.
    fn bind_break_value(&mut self, output: &mut String, val: &Expression, val_str: &str) {
        let assign_var = self.current_loop_result_var().map(str::to_string);
        let Some(var) = assign_var else {
            if !val_str.is_empty() {
                write_line!(output, "_ = {}", val_str);
            }
            return;
        };
        let is_unit_call =
            val.get_type().is_unit() && matches!(val.unwrap_parens(), Expression::Call { .. });
        if is_unit_call {
            if !val_str.is_empty() {
                write_line!(output, "{}", val_str);
            }
            write_line!(output, "{} = struct{{}}{{}}", var);
        } else if !val_str.is_empty() {
            write_line!(output, "{} = {}", var, val_str);
        }
    }

    fn emit_while_statement(
        &mut self,
        output: &mut String,
        condition: &Expression,
        body: &Expression,
        needs_label: bool,
    ) {
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
            self.emit_labeled_loop(output, &header, body, needs_label);
        } else if matches!(
            condition.unwrap_parens(),
            Expression::Literal {
                literal: Literal::Boolean(true),
                ..
            }
        ) {
            self.emit_labeled_loop(output, "for {\n", body, needs_label);
        } else {
            let cond = wrap_if_struct_literal(cond);
            self.emit_labeled_loop(output, &format!("for {} {{\n", cond), body, needs_label);
        }
        self.pop_loop();
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
                let base_str = if let Expression::Unary {
                    operator: UnaryOperator::Deref,
                    expression: inner,
                    ..
                } = expression.as_ref()
                {
                    self.emit_operand(output, inner)
                } else {
                    self.emit_operand(output, expression)
                };
                let expression_ty = expression.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member)
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
            } => self.emit_deref_lvalue(output, expression),
            Expression::Call { .. } if expression.get_type().is_ref() => {
                let call_str = self.emit_operand(output, expression);
                let tmp = self.fresh_var(Some("ref"));
                self.declare(&tmp);
                write_line!(output, "{} := {}", tmp, call_str);
                tmp
            }
            _ => "_".to_string(),
        }
    }

    /// Emit `*X` lvalue form, capturing the pointee into a temp if it's a
    /// call (Go requires an addressable operand for deref-assignment).
    fn emit_deref_lvalue(&mut self, output: &mut String, pointee: &Expression) -> String {
        let pointee_string = self.emit_operand(output, pointee);
        if matches!(pointee.unwrap_parens(), Expression::Call { .. }) {
            let tmp = self.fresh_var(Some("ref"));
            self.declare(&tmp);
            write_line!(output, "{} := {}", tmp, pointee_string);
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
            if let Some(access) =
                self.try_emit_tuple_struct_field_access(base_str, expression_ty, index)
            {
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
                let expression_ty = base.get_type();
                self.format_dot_access_lvalue(&base_str, &expression_ty, member)
            }
            Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner,
                ..
            } => self.emit_deref_lvalue(output, inner),
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
        Expression::Call { .. } if expression.get_type().is_ref() => true,
        _ => false,
    }
}
