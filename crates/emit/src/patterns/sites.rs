use crate::patterns::binding_decls::pattern_has_bindings;
use std::borrow::Cow;

use syntax::ast::{Expression, MatchArm, Pattern, TypedPattern};
use syntax::types::Type;

use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::names::go_name;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::binding_emit::{
    apply_refutable_root_assertion, apply_root_assertion, compose_refutable_condition,
    drop_inline_overlays, emit_tree_bindings_with_consumers, tree_assignment_statements,
    tree_binding_statements,
};
use crate::patterns::decision_tree::{self, PatternInfo, render_condition};
use crate::plan::bodies::{ElseArm, IfPlan, LoweredBlock, LoweredStatement, PlacePlan};
use crate::state::bindings::BindingValue;
use crate::write_line;

#[derive(Clone, Copy)]
pub(crate) struct AnnotatedPattern<'a> {
    pub(crate) pattern: &'a Pattern,
    pub(crate) typed: Option<&'a TypedPattern>,
}

#[derive(Clone, Copy)]
pub(crate) struct TypedSubject<'a> {
    pub(crate) var: &'a str,
    pub(crate) ty: &'a Type,
}

pub(crate) enum PatternSubject<'a> {
    /// Already in a Go variable named by the caller.
    Existing { var: String },
    /// Pattern-site picks: inline the scrutinee identifier when safe, else
    /// hoist into a fresh temp with the given hint.
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

/// For composite scrutinees, the declaration line is deferred so the caller can
/// pick `var := expr` vs `_ = expr` based on body usage.
enum ResolvedSubject {
    Existing { var: String },
    Composite { var: String, expression: String },
}

impl ResolvedSubject {
    fn var(&self) -> &str {
        match self {
            ResolvedSubject::Existing { var } | ResolvedSubject::Composite { var, .. } => var,
        }
    }

    fn emit_declaration(self, output: &mut String, body: &LoweredBlock) {
        if let ResolvedSubject::Composite { var, expression } = self {
            if body.references_var(&var) {
                write_line!(output, "{} := {}", var, expression);
            } else {
                write_line!(output, "_ = {}", expression);
            }
        }
    }
}

struct LetElseAlternatives<'s> {
    collected: Vec<PatternInfo>,
    hoisted: Vec<(Cow<'s, str>, Option<String>)>,
    irrefutable_index: Option<usize>,
}

impl Planner<'_> {
    fn resolve_pattern_subject(
        &mut self,
        output: &mut String,
        subject: PatternSubject<'_>,
        fx: &mut EmitEffects,
    ) -> ResolvedSubject {
        match subject {
            PatternSubject::Existing { var } => ResolvedSubject::Existing { var },
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
                    return ResolvedSubject::Existing { var };
                }
                let var = self.fresh_var(temp_hint);
                self.declare(&var);
                let expression = self.emit_value(output, scrutinee, ExpressionContext::value(), fx);
                ResolvedSubject::Composite { var, expression }
            }
        }
    }

    /// Lower an irrefutable pattern site (no branching): subject setup +
    /// declaration, root type assertion, per-field binding leaves.
    pub(crate) fn lower_irrefutable_pattern_site(
        &mut self,
        subject: PatternSubject<'_>,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        subject_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let mut prologue = String::new();
        let resolved = self.resolve_pattern_subject(&mut prologue, subject, fx);
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        fx.extend(&info.effects);

        let mut body = Vec::new();
        let mut assertion = String::new();
        let effective = apply_root_assertion(self, &mut assertion, &info, resolved.var());
        if !assertion.is_empty() {
            body.push(LoweredStatement::RawGo(assertion));
        }
        tree_binding_statements(self, &mut body, &info.bindings, &effective, &[]);
        let body_block = LoweredBlock { statements: body };

        let mut statements = Vec::new();
        if !prologue.is_empty() {
            statements.push(LoweredStatement::RawGo(prologue));
        }
        let mut declaration = String::new();
        resolved.emit_declaration(&mut declaration, &body_block);
        if !declaration.is_empty() {
            statements.push(LoweredStatement::RawGo(declaration));
        }
        statements.extend(body_block.statements);
        statements
    }

    pub(crate) fn emit_irrefutable_pattern_site(
        &mut self,
        output: &mut String,
        subject: PatternSubject<'_>,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        subject_ty: &Type,
        fx: &mut EmitEffects,
    ) {
        let statements =
            self.lower_irrefutable_pattern_site(subject, pattern, typed, subject_ty, fx);
        let block = LoweredBlock { statements };
        Renderer.render_lowered_block(output, &block);
    }

    pub(crate) fn lower_let_else_pattern_site(
        &mut self,
        ap: AnnotatedPattern,
        binding_ty: &Type,
        scrutinee: &Expression,
        else_block: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let value_ty = scrutinee.get_type();
        let mut prologue = String::new();
        let resolved = self.resolve_pattern_subject(
            &mut prologue,
            PatternSubject::expression(scrutinee, ap.pattern, Some("subject")),
            fx,
        );
        let subject = TypedSubject {
            var: resolved.var(),
            ty: &value_ty,
        };

        let body = if matches!(ap.pattern, Pattern::Or { .. }) {
            self.lower_let_else_or_pattern(ap, binding_ty, subject, else_block, fx)
        } else {
            self.lower_let_else_single_pattern(ap, subject, else_block, fx)
        };
        let body_block = LoweredBlock { statements: body };

        let mut statements = Vec::new();
        if !prologue.is_empty() {
            statements.push(LoweredStatement::RawGo(prologue));
        }
        let mut declaration = String::new();
        resolved.emit_declaration(&mut declaration, &body_block);
        if !declaration.is_empty() {
            statements.push(LoweredStatement::RawGo(declaration));
        }
        statements.extend(body_block.statements);
        statements
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn emit_while_let(
        &mut self,
        output: &mut String,
        pattern: &Pattern,
        typed: Option<&TypedPattern>,
        scrutinee: &Expression,
        body: &Expression,
        needs_label: bool,
        fx: &mut EmitEffects,
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
            let expression = self.emit_operand(output, scrutinee, ExpressionContext::value(), fx);
            write_line!(output, "{} := {}", var, expression);
            var
        });

        let scrutinee_ty = scrutinee.get_type();
        if let Pattern::Or { patterns, .. } = pattern
            && pattern_has_bindings(pattern)
        {
            self.emit_while_let_or_pattern(
                output,
                patterns,
                TypedSubject {
                    var: &subject_var,
                    ty: &scrutinee_ty,
                },
                body,
                fx,
            );
            return;
        }

        let info = decision_tree::collect_pattern_info(self, pattern, typed, &scrutinee_ty);
        fx.extend(&info.effects);
        let (effective, ok_var) = apply_refutable_root_assertion(self, output, &info, &subject_var);
        let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, &effective);
        write_line!(output, "if {} {{", condition);
        self.enter_scope();

        if !matches!(pattern, Pattern::Or { .. }) {
            emit_tree_bindings_with_consumers(self, output, &info.bindings, &effective, &[body]);
        }

        let block = self.lower_block_as_body(body, fx);
        Renderer.render_lowered_block(output, &block);

        self.emit_while_let_break_else(output);
    }

    fn lower_let_else_single_pattern(
        &mut self,
        ap: AnnotatedPattern,
        subject: TypedSubject,
        else_block: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern, typed } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        fx.extend(&info.effects);

        let mut statements = Vec::new();
        let mut assert_buffer = String::new();
        let (effective_subject, assert_ok_var) =
            apply_refutable_root_assertion(self, &mut assert_buffer, &info, subject_var);
        if !assert_buffer.is_empty() {
            statements.push(LoweredStatement::RawGo(assert_buffer));
        }

        if info.checks.is_empty() && assert_ok_var.is_none() {
            tree_binding_statements(
                self,
                &mut statements,
                &info.bindings,
                &effective_subject,
                &[],
            );
            return statements;
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
        let else_lowered = self.lower_block_as_body(else_block, fx);
        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition: guard,
            then_body: else_lowered,
            else_arm: ElseArm::None,
        }));

        tree_binding_statements(
            self,
            &mut statements,
            &info.bindings,
            &effective_subject,
            &[],
        );
        statements
    }

    fn lower_let_else_or_pattern(
        &mut self,
        ap: AnnotatedPattern,
        binding_ty: &Type,
        subject: TypedSubject,
        else_block: &Expression,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern, typed } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let Pattern::Or { patterns, .. } = pattern else {
            unreachable!("lower_let_else_or_pattern requires an Or pattern");
        };
        let pre_let_snapshot = self.scope.binding_snapshot();
        let mut declarations = String::new();
        self.emit_binding_declarations_with_type(&mut declarations, pattern, binding_ty, typed, fx);
        let post_declaration_snapshot = self.scope.binding_snapshot();

        let mut asserts = String::new();
        let alts =
            self.collect_let_else_alternatives(&mut asserts, patterns, subject_ty, subject_var, fx);

        let mut statements = Vec::new();
        if !declarations.is_empty() {
            statements.push(LoweredStatement::RawGo(declarations));
        }
        if !asserts.is_empty() {
            statements.push(LoweredStatement::RawGo(asserts));
        }

        let chain_len = alts.irrefutable_index.unwrap_or(alts.collected.len());

        // An irrefutable first alternative always matches: no chain, just its
        // assignments.
        if chain_len == 0 {
            let (effective, _) = &alts.hoisted[0];
            tree_assignment_statements(
                self,
                &mut statements,
                &alts.collected[0].bindings,
                effective,
            );
            return statements;
        }

        // Forward pass (preserves scope-op order): condition plus assignment
        // block per chain alternative.
        let mut pieces: Vec<(String, LoweredBlock)> = Vec::with_capacity(chain_len);
        for (i, info) in alts.collected.iter().take(chain_len).enumerate() {
            let (effective, ok_var) = &alts.hoisted[i];
            let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, effective);
            let mut assigns = Vec::new();
            tree_assignment_statements(self, &mut assigns, &info.bindings, effective);
            pieces.push((
                condition,
                LoweredBlock {
                    statements: assigns,
                },
            ));
        }

        // Terminal `else`: the irrefutable alternative's assignments, or the
        // lowered `else` block (with the or-pattern bindings out of scope).
        let terminal = match alts.irrefutable_index {
            Some(index) => {
                let (effective, _) = &alts.hoisted[index];
                let mut assigns = Vec::new();
                tree_assignment_statements(
                    self,
                    &mut assigns,
                    &alts.collected[index].bindings,
                    effective,
                );
                LoweredBlock {
                    statements: assigns,
                }
            }
            None => {
                self.scope.restore_binding_snapshot(pre_let_snapshot);
                let else_lowered = self.lower_block_as_body(else_block, fx);
                self.scope
                    .restore_binding_snapshot(post_declaration_snapshot);
                else_lowered
            }
        };

        statements.push(assemble_if_else_chain(pieces, terminal));
        statements
    }

    fn collect_let_else_alternatives<'s>(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        subject_ty: &Type,
        subject_var: &'s str,
        fx: &mut EmitEffects,
    ) -> LetElseAlternatives<'s> {
        let collected: Vec<PatternInfo> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, None, subject_ty))
            .collect();
        for info in &collected {
            fx.extend(&info.effects);
        }
        let hoisted: Vec<(Cow<'s, str>, Option<String>)> = collected
            .iter()
            .map(|info| apply_refutable_root_assertion(self, output, info, subject_var))
            .collect();
        let irrefutable_index = collected
            .iter()
            .zip(hoisted.iter())
            .position(|(info, (_, ok_var))| info.checks.is_empty() && ok_var.is_none());
        LetElseAlternatives {
            collected,
            hoisted,
            irrefutable_index,
        }
    }

    fn emit_while_let_or_pattern(
        &mut self,
        output: &mut String,
        patterns: &[Pattern],
        subject: TypedSubject,
        body: &Expression,
        fx: &mut EmitEffects,
    ) {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let mut alternatives: Vec<_> = patterns
            .iter()
            .map(|alt| decision_tree::collect_pattern_info(self, alt, None, subject_ty))
            .collect();
        for info in &alternatives {
            fx.extend(&info.effects);
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
            let block = self.lower_block_as_body(body, fx);
            Renderer.render_lowered_block(output, &block);
            drop_inline_overlays(self, &overlays);
        }

        self.emit_while_let_break_else(output);
    }

    pub(crate) fn lower_select_receive_pattern_site(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        body: &Expression,
        default_body: Option<&Expression>,
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_arm(subject, ap, body, place, fx, |this, fx| {
            default_body.map(|default| this.lower_block_to_place(default, place, fx))
        })
    }

    pub(crate) fn lower_select_match_receive_some_site(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        some_body: &Expression,
        match_arms: &[MatchArm],
        place: &PlacePlan,
        fx: &mut EmitEffects,
    ) -> Vec<LoweredStatement> {
        self.lower_refutable_arm(subject, ap, some_body, place, fx, |this, fx| {
            Some(lower_none_arm_body(this, match_arms, place, fx))
        })
    }

    /// Lower a refutable site whose checks gate `body` into structured IR. The
    /// `failure` callback produces the `else` block (run only on the guarded
    /// path) for the caller's failure continuation; `None` means no `else`.
    fn lower_refutable_arm(
        &mut self,
        subject: TypedSubject,
        ap: AnnotatedPattern,
        body: &Expression,
        place: &PlacePlan,
        fx: &mut EmitEffects,
        failure: impl FnOnce(&mut Planner, &mut EmitEffects) -> Option<LoweredBlock>,
    ) -> Vec<LoweredStatement> {
        let AnnotatedPattern { pattern, typed } = ap;
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = decision_tree::collect_pattern_info(self, pattern, typed, subject_ty);
        fx.extend(&info.effects);
        let mut statements = Vec::new();
        let mut asserts = String::new();
        let (effective, ok_var) =
            apply_refutable_root_assertion(self, &mut asserts, &info, subject_var);
        if !asserts.is_empty() {
            statements.push(LoweredStatement::RawGo(asserts));
        }

        if info.checks.is_empty() && ok_var.is_none() {
            tree_binding_statements(self, &mut statements, &info.bindings, &effective, &[body]);
            let block = self.lower_block_to_place(body, place, fx);
            statements.extend(block.statements);
            return statements;
        }

        let condition = compose_refutable_condition(ok_var.as_deref(), &info.checks, &effective);
        let mut then_body = Vec::new();
        let overlays =
            tree_binding_statements(self, &mut then_body, &info.bindings, &effective, &[body]);
        let block = self.lower_block_to_place(body, place, fx);
        then_body.extend(block.statements);
        drop_inline_overlays(self, &overlays);
        let else_arm = match failure(self, fx) {
            Some(body) => ElseArm::Else {
                body,
                inline: false,
            },
            None => ElseArm::None,
        };
        statements.push(LoweredStatement::If(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition,
            then_body: LoweredBlock {
                statements: then_body,
            },
            else_arm,
        }));
        statements
    }
}

pub(crate) fn lower_none_arm_body(
    planner: &mut Planner,
    match_arms: &[MatchArm],
    place: &PlacePlan,
    fx: &mut EmitEffects,
) -> LoweredBlock {
    for match_arm in match_arms {
        if let Pattern::EnumVariant { identifier, .. } = &match_arm.pattern {
            let variant_name = go_name::unqualified_name(identifier);
            if variant_name == "None" {
                return planner.lower_block_to_place(&match_arm.expression, place, fx);
            }
        }
    }
    LoweredBlock {
        statements: Vec::new(),
    }
}

/// True when `Some(_)`-shaped (peeling any outer `as`-binding), with exactly
/// one payload field.
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
/// the outer is not `Some(_)`.
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

impl Planner<'_> {
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
}

/// Fold the `(condition, body)` pieces plus a terminal `else` block into a
/// nested if/else-if statement, built from the back. `pieces` must be
/// non-empty.
fn assemble_if_else_chain(
    mut pieces: Vec<(String, LoweredBlock)>,
    terminal: LoweredBlock,
) -> LoweredStatement {
    let mut else_arm = ElseArm::Else {
        body: terminal,
        inline: false,
    };
    while pieces.len() > 1 {
        let (condition, then_body) = pieces.pop().expect("len > 1");
        else_arm = ElseArm::ElseIf(Box::new(IfPlan {
            directive: String::new(),
            condition_setup: String::new(),
            condition,
            then_body,
            else_arm,
        }));
    }
    let (condition, then_body) = pieces.pop().expect("pieces is non-empty");
    LoweredStatement::If(IfPlan {
        directive: String::new(),
        condition_setup: String::new(),
        condition,
        then_body,
        else_arm,
    })
}
