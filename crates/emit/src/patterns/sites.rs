//! Pattern-site owner for non-match emission.
//!
//! `decision_tree` builds runtime checks and bindings from a single pattern;
//! `MatchEmitPlan` (via `tree_emitter`) is the match-construct pipeline. This
//! module sits between them for every other site that contains a pattern.
//!
//! Callers describe the construct, this module owns the pattern mechanics:
//! subject materialization, root assertions, refutable condition assembly,
//! binding emission, and or-pattern scope policy.

use syntax::ast::{Binding, Expression, MatchArm, Pattern, TypedPattern};
use syntax::types::Type;

use crate::Emitter;
use crate::bindings::BindingValue;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::expressions::context::ExpressionContext;
use crate::names::go_name;
use crate::patterns::bindings::pattern_binds_name;
use crate::patterns::decision_tree::{
    self, apply_refutable_root_assertion, apply_root_assertion, compose_refutable_condition,
    drop_inline_overlays, emit_tree_assignments, emit_tree_bindings,
    emit_tree_bindings_with_consumers, render_condition,
};
use crate::placement::BodyPlace;
use crate::utils::{DiscardGuard, ValueTempDiscard};
use crate::write_line;

/// How a pattern's subject value reaches the site.
pub(crate) enum PatternSubject<'a> {
    /// Subject already lives in a Go variable named by the caller.
    Existing { var: String },
    /// Pattern-site decides whether to inline the scrutinee identifier
    /// (when safe) or hoist it into a fresh temp with the given hint.
    Expression {
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'a str>,
    },
}

impl<'a> PatternSubject<'a> {
    pub(crate) fn for_value(var: impl Into<String>) -> Self {
        Self::Existing { var: var.into() }
    }

    pub(crate) fn expression(
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'a str>,
    ) -> Self {
        Self::Expression {
            scrutinee,
            pattern,
            temp_hint,
        }
    }
}

struct ResolvedSubject {
    var: String,
    guard: Option<ValueTempDiscard>,
}

impl Emitter<'_> {
    fn resolve_pattern_subject(
        &mut self,
        output: &mut String,
        subject: PatternSubject<'_>,
    ) -> ResolvedSubject {
        match subject {
            PatternSubject::Existing { var } => ResolvedSubject { var, guard: None },
            PatternSubject::Expression {
                scrutinee,
                pattern,
                temp_hint,
            } => {
                if let Expression::Identifier { value, .. } = scrutinee
                    && !value.contains('.')
                    && !pattern_binds_name(pattern, value)
                    && !matches!(
                        self.scope.resolve_identifier_binding(value),
                        Some(BindingValue::InlineExpr(_))
                    )
                {
                    let var = self.scope.resolve_or_escape_go_name(value);
                    return ResolvedSubject { var, guard: None };
                }
                let var = self.fresh_var(temp_hint);
                self.declare(&var);
                let expression = self.emit_value(output, scrutinee, ExpressionContext::value());
                let decl_start = output.len();
                write_line!(output, "{} := {}", var, expression);
                let guard = ValueTempDiscard::new(output, decl_start, &var, &expression);
                ResolvedSubject {
                    var,
                    guard: Some(guard),
                }
            }
        }
    }

    pub(crate) fn emit_irrefutable_pattern_site(
        &mut self,
        output: &mut String,
        subject: PatternSubject<'_>,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        subject_ty: &Type,
    ) {
        let resolved = self.resolve_pattern_subject(output, subject);
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        self.requirements.apply_effects(&info.effects);
        let effective = apply_root_assertion(self, output, &info, &resolved.var);
        emit_tree_bindings(self, output, &info.bindings, &effective);
        if let Some(guard) = resolved.guard {
            guard.finish(output);
        }
    }

    pub(crate) fn emit_let_else_pattern_site(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        binding_ty: &Type,
        scrutinee: &Expression,
        else_block: &Expression,
    ) {
        let value_ty = scrutinee.get_type();
        let resolved = self.resolve_pattern_subject(
            output,
            PatternSubject::expression(scrutinee, pattern, Some("subject")),
        );

        if let Pattern::Or { patterns, .. } = pattern {
            self.emit_let_else_or_pattern(
                output,
                pattern,
                patterns,
                typed,
                binding_ty,
                &resolved.var,
                &value_ty,
                else_block,
            );
        } else {
            self.emit_let_else_single_pattern(
                output,
                pattern,
                typed,
                &resolved.var,
                &value_ty,
                else_block,
            );
        }

        if let Some(guard) = resolved.guard {
            guard.finish(output);
        }
    }

    pub(crate) fn emit_while_let(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        scrutinee: &Expression,
        body: &Expression,
        needs_label: bool,
    ) {
        self.set_current_loop_label_if_needed(needs_label);
        if let Some(label) = self.current_loop_label() {
            write_line!(output, "{}:", label);
        }
        output.push_str("for {\n");

        let inline_var = if let Expression::Identifier { value, .. } = scrutinee {
            let has_collision = pattern_binds_name(pattern, value);
            let bound_to_inline = matches!(
                self.scope.resolve_identifier_binding(value),
                Some(BindingValue::InlineExpr(_))
            );
            if !has_collision && !value.contains('.') && !bound_to_inline {
                Some(self.scope.resolve_or_escape_go_name(value))
            } else {
                None
            }
        } else {
            None
        };
        let subject_var = inline_var.unwrap_or_else(|| {
            let var = self.fresh_var(Some("subject"));
            let expression = self.emit_operand(output, scrutinee, ExpressionContext::value());
            write_line!(output, "{} := {}", var, expression);
            var
        });

        let scrutinee_ty = scrutinee.get_type();
        if let Pattern::Or { patterns, .. } = pattern
            && Self::pattern_has_bindings(pattern)
        {
            self.emit_while_let_or_pattern(output, patterns, &subject_var, &scrutinee_ty, body);
            return;
        }

        let info = decision_tree::collect_pattern_info(self, pattern, typed, &scrutinee_ty);
        self.requirements.apply_effects(&info.effects);
        let (effective, ok_var) = apply_refutable_root_assertion(self, output, &info, &subject_var);
        let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, &effective);
        write_line!(output, "if {} {{", condition);
        self.enter_scope();

        if !matches!(pattern, Pattern::Or { .. }) {
            emit_tree_bindings_with_consumers(self, output, &info.bindings, &effective, &[body]);
        }

        self.emit_block(output, body);

        self.emit_while_let_break_else(output);
    }

    fn emit_let_else_single_pattern(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        subject_var: &str,
        subject_ty: &Type,
        else_block: &Expression,
    ) {
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        self.requirements.apply_effects(&info.effects);

        let (effective_subject, assert_ok_var) =
            apply_refutable_root_assertion(self, output, &info, subject_var);

        if info.checks.is_empty() && assert_ok_var.is_none() {
            emit_tree_bindings(self, output, &info.bindings, &effective_subject);
            return;
        }

        let mut guard_parts: Vec<String> = Vec::new();
        if let Some(ref ok) = assert_ok_var {
            guard_parts.push(format!("!{}", ok));
        }
        if !info.checks.is_empty() {
            let negated = match info.checks.as_slice() {
                [check] => check.render_negated(&effective_subject),
                _ => format!("!({})", render_condition(&info.checks, &effective_subject)),
            };
            guard_parts.push(wrap_if_struct_literal(negated));
        }
        let guard = guard_parts.join(" || ");
        write_line!(output, "if {} {{", guard);
        self.emit_block(output, else_block);
        output.push_str("}\n");

        emit_tree_bindings(self, output, &info.bindings, &effective_subject);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_let_else_or_pattern(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        patterns: &[Pattern],
        typed: Option<&TypedPattern>,
        binding_ty: &Type,
        subject_var: &str,
        subject_ty: &Type,
        else_block: &Expression,
    ) {
        let outer_snapshot = self.scope.binding_snapshot();

        self.emit_binding_declarations_with_type(output, pattern, binding_ty, typed);

        let pattern_snapshot = self.scope.binding_snapshot();

        let collected: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, None, subject_ty))
            .collect();
        for info in &collected {
            self.requirements.apply_effects(&info.effects);
        }

        let hoisted: Vec<_> = collected
            .iter()
            .map(|info| apply_refutable_root_assertion(self, output, info, subject_var))
            .collect();

        let irrefutable_idx = collected
            .iter()
            .zip(hoisted.iter())
            .position(|(info, (_, ok_var))| info.checks.is_empty() && ok_var.is_none());
        let chain_len = irrefutable_idx.unwrap_or(collected.len());

        for (i, info) in collected.iter().take(chain_len).enumerate() {
            let (effective, ok_var) = &hoisted[i];
            let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, effective);
            if i == 0 {
                write_line!(output, "if {} {{", condition);
            } else {
                write_line!(output, "}} else if {} {{", condition);
            }

            emit_tree_assignments(self, output, &info.bindings, effective);
        }

        if let Some(idx) = irrefutable_idx {
            let info = &collected[idx];
            let (effective, _) = &hoisted[idx];
            if idx == 0 {
                emit_tree_assignments(self, output, &info.bindings, effective);
            } else {
                output.push_str("} else {\n");
                emit_tree_assignments(self, output, &info.bindings, effective);
                output.push_str("}\n");
            }
            return;
        }

        self.scope.restore_binding_snapshot(outer_snapshot);
        output.push_str("} else {\n");
        self.emit_block(output, else_block);
        output.push_str("}\n");

        self.scope.restore_binding_snapshot(pattern_snapshot);
    }

    fn emit_while_let_or_pattern(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        subject_var: &str,
        subject_ty: &Type,
        body: &Expression,
    ) {
        let mut alternatives: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, None, subject_ty))
            .collect();
        for info in &alternatives {
            self.requirements.apply_effects(&info.effects);
        }

        let unused_names: rustc_hash::FxHashSet<String> = alternatives
            .iter()
            .flat_map(|info| info.bindings.iter())
            .filter(|b| b.go_name.is_none())
            .map(|b| b.lisette_name.clone())
            .collect();
        for info in alternatives.iter_mut() {
            for binding in info.bindings.iter_mut() {
                if unused_names.contains(&binding.lisette_name) {
                    binding.go_name = None;
                }
            }
        }

        let hoisted: Vec<_> = alternatives
            .iter()
            .map(|info| apply_refutable_root_assertion(self, output, info, subject_var))
            .collect();

        for (i, info) in alternatives.iter().enumerate() {
            let (effective, ok_var) = &hoisted[i];
            let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, effective);

            self.emit_branch_header(output, &condition, false, i == 0);

            let overlays =
                emit_tree_bindings_with_consumers(self, output, &info.bindings, effective, &[body]);
            self.emit_block(output, body);
            drop_inline_overlays(self, &overlays);
        }

        self.emit_while_let_break_else(output);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_select_receive_pattern_site(
        &mut self,
        output: &mut String,
        receiver_var: &str,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        element_ty: &Type,
        body: &Expression,
        default_body: Option<&Expression>,
        place: &BodyPlace,
    ) {
        self.emit_refutable_arm(
            output,
            receiver_var,
            pattern,
            typed,
            element_ty,
            body,
            place,
            |this, output| {
                if let Some(default_body) = default_body {
                    output.push_str("} else {\n");
                    this.emit_body_to_place(output, default_body, place);
                }
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_select_match_receive_some_site(
        &mut self,
        output: &mut String,
        case_var: &str,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        element_ty: &Type,
        some_body: &Expression,
        match_arms: &[MatchArm],
        place: &BodyPlace,
    ) {
        self.emit_refutable_arm(
            output,
            case_var,
            pattern,
            typed,
            element_ty,
            some_body,
            place,
            |this, output| {
                output.push_str("} else {\n");
                emit_none_arm_body(this, output, match_arms, place);
            },
        );
    }

    /// Emit a refutable site whose checks gate `body`. The `failure` closure
    /// runs only on the guarded path (after the body, before the closing
    /// brace) so callers can emit `} else { ... }` for their failure continuation.
    #[allow(clippy::too_many_arguments)]
    fn emit_refutable_arm(
        &mut self,
        output: &mut String,
        subject_var: &str,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        subject_ty: &Type,
        body: &Expression,
        place: &BodyPlace,
        failure: impl FnOnce(&mut Emitter, &mut String),
    ) {
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        self.requirements.apply_effects(&info.effects);
        let (effective, ok_var) = apply_refutable_root_assertion(self, output, &info, subject_var);
        if info.checks.is_empty() && ok_var.is_none() {
            emit_tree_bindings_with_consumers(self, output, &info.bindings, &effective, &[body]);
            self.emit_body_to_place(output, body, place);
            return;
        }
        let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, &effective);
        write_line!(output, "if {} {{", condition);
        let overlays =
            emit_tree_bindings_with_consumers(self, output, &info.bindings, &effective, &[body]);
        self.emit_body_to_place(output, body, place);
        drop_inline_overlays(self, &overlays);
        failure(self, output);
        output.push_str("}\n");
    }
}

pub(crate) fn emit_none_arm_body(
    emitter: &mut Emitter,
    output: &mut String,
    match_arms: &[MatchArm],
    place: &BodyPlace,
) {
    for match_arm in match_arms {
        if let Pattern::EnumVariant { identifier, .. } = &match_arm.pattern {
            let variant_name = go_name::unqualified_name(identifier);
            if variant_name == "None" {
                emitter.emit_body_to_place(output, &match_arm.expression, place);
                return;
            }
        }
    }
}

/// True when `Some(_)`-shaped (peeling any outer `as`-binding), with exactly
/// one payload field. Used by select receive arms to decide whether the
/// receive needs an `ok` check on channel closure.
pub(crate) fn is_some_pattern(pattern: &Pattern) -> bool {
    let pattern = peel_as_binding(pattern);
    if let Pattern::EnumVariant {
        identifier, fields, ..
    } = pattern
    {
        let variant_name = go_name::unqualified_name(identifier);
        return variant_name == "Some" && fields.len() == 1;
    }
    false
}

/// Peel `Some(inner)` to expose `inner`; returns the original pattern when
/// the outer is not `Some(_)`. Pairs with `is_some_pattern` for select receive.
pub(crate) fn unwrap_some_pattern(pattern: &Pattern) -> &Pattern {
    let pattern = peel_as_binding(pattern);
    if let Pattern::EnumVariant {
        identifier, fields, ..
    } = pattern
        && go_name::unqualified_name(identifier) == "Some"
        && fields.len() == 1
    {
        return &fields[0];
    }
    pattern
}

pub(crate) fn unwrap_some_typed_pattern(typed: Option<&TypedPattern>) -> Option<&TypedPattern> {
    if let Some(TypedPattern::EnumVariant {
        variant_name,
        fields,
        ..
    }) = typed
        && variant_name == "Some"
        && fields.len() == 1
    {
        return Some(&fields[0]);
    }
    None
}

fn peel_as_binding(pattern: &Pattern) -> &Pattern {
    match pattern {
        Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
        p => p,
    }
}

impl Emitter<'_> {
    /// Map a `Some(pattern)` payload to a case-variable name and whether the
    /// payload needs decision-tree destructuring inside the arm body (rather
    /// than being bound directly by the `case v := <-ch:` header).
    pub(crate) fn classify_receive_var_pattern(&mut self, pattern: &Pattern) -> (String, bool) {
        match pattern {
            Pattern::WildCard { .. } => ("_".to_string(), false),
            Pattern::Identifier { identifier, .. } => {
                let Some(go_name) = self.go_name_for_binding(pattern) else {
                    return ("_".to_string(), false);
                };
                if self.scope.resolve_identifier_binding(identifier).is_some() {
                    return (self.fresh_var(Some("recv")), true);
                }
                (self.scope.bind(identifier, go_name), false)
            }
            _ => (self.fresh_var(Some("recv")), true),
        }
    }

    /// Emit a for-loop element destructure. Owns the header form decision:
    /// when the pattern destructures nothing, emit `for range xs`; otherwise
    /// capture into a fresh `item` var and route through the irrefutable site.
    pub(crate) fn emit_for_loop_pattern_site(
        &mut self,
        output: &mut String,
        binding: &Binding,
        iter_expression: &str,
        is_channel: bool,
        body: &Expression,
    ) {
        if !Self::pattern_has_bindings(&binding.pattern) {
            write_line!(output, "for range {} {{", iter_expression);
            self.emit_block(output, body);
            output.push_str("}\n");
            return;
        }
        let item_var = self.fresh_var(Some("item"));
        if is_channel {
            write_line!(output, "for {} := range {} {{", item_var, iter_expression);
        } else {
            write_line!(
                output,
                "for _, {} := range {} {{",
                item_var,
                iter_expression
            );
        }
        let guard = DiscardGuard::new(output, &item_var);
        self.emit_irrefutable_pattern_site(
            output,
            PatternSubject::for_value(item_var),
            &binding.pattern,
            binding.typed_pattern.as_ref(),
            &binding.ty,
        );
        self.emit_block(output, body);
        guard.finish(output);
        output.push_str("}\n");
    }
}
