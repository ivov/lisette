use crate::abi::fallible_producer::ResultFusePlan;
use std::borrow::Cow;
use std::slice;

use crate::ir::bodies::GoUses;
use crate::ir::go_expression::{BinaryOp, GoExpressionNode, UnaryOp};
use crate::patterns::binding_decls::{has_binding_or_pattern, split_binding_or_patterns};

use syntax::ast::{Expression, IdentifierResolution, MatchArm, Pattern, Span};
use syntax::types::Type;

use crate::Planner;
use crate::calls::bound_value::PairCondition;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoweredBlock, LoweredStatement, PlacePlan, Statement,
    declare, define, discard, expression_statement,
};
use crate::ir::go_expression::CompositeLayout;
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::go_name::GeneratedPackage;
use crate::patterns::binding_decls::pattern_binds_name;
use crate::patterns::binding_statements::AssertedPattern;
use crate::patterns::decision_tree::{
    self, BindingTarget, PatternBinding, PatternInfo, SubjectRoot, render_condition,
};
use crate::patterns::matching::{
    ArmBinding, FusedName, PreludeVariant, field_binding, ok_pattern_field, prelude_variant,
    some_pattern_field,
};
use crate::state::bindings::BindingValue;
use crate::state::expression::ExpressionContext;
use crate::statements::testing::{test_context_call, testkit_operand_type};

#[derive(Clone, Copy)]
pub(crate) struct TypedSubject<'a> {
    pub(crate) var: &'a GoExpression,
    pub(crate) ty: &'a Type,
}

#[derive(Clone, Copy)]
enum RefutableFail<'a> {
    ElseBlock(&'a Expression),
    AssertFail(Span),
}

pub(crate) enum PatternSubject<'a> {
    Existing {
        var: GoExpression,
    },
    Expression {
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'static str>,
    },
}

impl<'a> PatternSubject<'a> {
    pub(crate) fn for_identifier(var: GoIdentifier) -> Self {
        Self::Existing {
            var: GoExpression::identifier(var),
        }
    }

    pub(crate) fn expression(
        scrutinee: &'a Expression,
        pattern: &'a Pattern,
        temp_hint: Option<&'static str>,
    ) -> Self {
        Self::Expression {
            scrutinee,
            pattern,
            temp_hint,
        }
    }
}

pub(super) enum ResolvedSubject {
    Existing(GoExpression),
    Materialized {
        subject: GoExpression,
        local: GoIdentifier,
    },
    Deferred {
        name: GoIdentifier,
        expression: GoExpression,
    },
}

impl ResolvedSubject {
    pub(super) fn var(&self) -> GoExpression {
        match self {
            Self::Existing(subject) | Self::Materialized { subject, .. } => {
                subject.clone().without_positions()
            }
            Self::Deferred { name, .. } => GoExpression::identifier(name.clone()),
        }
    }

    pub(super) fn push_declaration(self, statements: &mut Vec<Statement>, body: &[Statement]) {
        match self {
            Self::Existing(_) => {}
            Self::Materialized { subject, local } => {
                if !GoUses::of(body).contains_identifier(&local) {
                    statements.push(discard(subject));
                }
            }
            Self::Deferred { name, expression } => {
                if GoUses::of(body).contains_identifier(&name) {
                    statements.push(define(name, expression));
                } else {
                    statements.push(discard(expression));
                }
            }
        }
    }
}

fn field_path_root(expression: &Expression) -> Option<(&str, &IdentifierResolution)> {
    let mut expression = expression.unwrap_parens();
    while let Expression::DotAccess {
        expression: inner, ..
    } = expression
    {
        expression = inner.unwrap_parens();
    }
    match expression {
        Expression::Identifier {
            value, resolution, ..
        } => Some((value, resolution)),
        _ => None,
    }
}

fn field_path_go_root(expression: &GoExpression) -> Option<&GoIdentifier> {
    let GoExpressionNode::Selector { base, .. } = expression.node() else {
        return None;
    };
    let mut base = base.as_ref();
    while let GoExpressionNode::Selector { base: inner, .. } = base {
        base = inner.as_ref();
    }
    match base {
        GoExpressionNode::Identifier(name) => Some(name),
        _ => None,
    }
}

impl Planner<'_> {
    pub(crate) fn can_reuse_subject_identifier(
        &self,
        value: &str,
        resolution: &IdentifierResolution,
        binds_name: bool,
    ) -> bool {
        !binds_name
            && !value.contains('.')
            && self
                .scope
                .resolve_identifier_with_resolution(value, resolution)
                .is_none_or(BindingValue::can_reuse_pattern_subject)
    }

    pub(crate) fn field_path_reads_in_place(
        &self,
        subject: &Expression,
        expression: &GoExpression,
        binds_root: impl Fn(&str) -> bool,
    ) -> bool {
        let Some((root, resolution)) = field_path_root(subject) else {
            return false;
        };
        field_path_go_root(expression).is_some()
            && self.can_reuse_subject_identifier(root, resolution, binds_root(root))
    }

    fn resolve_pattern_subject(
        &mut self,
        setup: &mut Vec<Statement>,
        subject: PatternSubject<'_>,
    ) -> ResolvedSubject {
        match subject {
            PatternSubject::Existing { var } => ResolvedSubject::Existing(var),
            PatternSubject::Expression {
                scrutinee,
                pattern,
                temp_hint,
            } => {
                if let Expression::Identifier {
                    value, resolution, ..
                } = scrutinee
                    && self.can_reuse_subject_identifier(
                        value,
                        resolution,
                        pattern_binds_name(pattern, value),
                    )
                {
                    return ResolvedSubject::Existing(GoExpression::identifier(
                        self.reference_go_identifier(value, resolution),
                    ));
                }
                let plan = self.plan_value(scrutinee, ExpressionContext::value());
                let materialized = plan
                    .expression()
                    .as_identifier()
                    .or_else(|| field_path_go_root(plan.expression()))
                    .filter(|local| {
                        plan.binds_local(local) && !self.scope.has_binding_for_local(local)
                    })
                    .cloned();
                let rests_in_own_temp_path =
                    field_path_go_root(plan.expression()).is_some_and(|root| {
                        plan.binds_local(root)
                            && !self.scope.has_binding_for_local(root)
                            && !pattern_binds_name(pattern, root)
                    });
                let rests_in_stable_name =
                    rests_in_own_temp_path || self.plan_rests_in_stable_name(&plan);
                let (op_setup, expression) = plan.into_parts();
                setup.extend(op_setup);
                if rests_in_stable_name
                    || self.field_path_reads_in_place(scrutinee, &expression, |root| {
                        pattern_binds_name(pattern, root)
                    })
                {
                    return match materialized {
                        Some(local) => ResolvedSubject::Materialized {
                            subject: expression,
                            local,
                        },
                        None => ResolvedSubject::Existing(expression),
                    };
                }
                let name = self.fresh_local(temp_hint);
                self.declare(&name);
                ResolvedSubject::Deferred { name, expression }
            }
        }
    }

    pub(crate) fn lower_irrefutable_pattern_site(
        &mut self,
        subject: PatternSubject<'_>,
        pattern: &Pattern,
        subject_ty: &Type,
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        let resolved = self.resolve_pattern_subject(&mut statements, subject);
        let info = self.collect_pattern_info(pattern, subject_ty);

        let mut body = Vec::new();
        let asserted = self.apply_root_assertion(&mut body, info, &resolved.var());
        self.tree_binding_statements(
            &mut body,
            &asserted.bindings,
            SubjectRoot::Var(&asserted.subject),
            &[],
        );

        resolved.push_declaration(&mut statements, &body);
        statements.extend(body);
        statements
    }

    pub(crate) fn lower_let_else_pattern_site(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Vec<Statement> {
        self.lower_refutable_let_site(pattern, scrutinee, RefutableFail::ElseBlock(else_block))
    }

    pub(crate) fn lower_let_assert_pattern_site(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        pattern_span: Span,
    ) -> Vec<Statement> {
        self.lower_refutable_let_site(pattern, scrutinee, RefutableFail::AssertFail(pattern_span))
    }

    fn lower_refutable_let_site(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        fail: RefutableFail,
    ) -> Vec<Statement> {
        if let RefutableFail::ElseBlock(else_block) = fail {
            if let Some(statements) =
                self.try_lower_fused_option_let_else(pattern, scrutinee, else_block)
            {
                return statements;
            }
            if let Some(statements) =
                self.try_lower_fused_result_let_else(pattern, scrutinee, else_block)
            {
                return statements;
            }
        }

        let value_ty = scrutinee.get_type();
        let mut statements = Vec::new();
        let resolved = self.resolve_pattern_subject(
            &mut statements,
            PatternSubject::expression(scrutinee, pattern, Some("subject")),
        );
        let subject = TypedSubject {
            var: &resolved.var(),
            ty: &value_ty,
        };

        let subject_is_fixed = self.facts.is_unmutated_identifier(scrutinee);
        let body = if matches!(pattern, Pattern::Or { .. }) || has_binding_or_pattern(pattern) {
            self.lower_let_else_or_pattern(pattern, subject, fail)
        } else {
            self.lower_let_else_single_pattern(pattern, subject, fail, subject_is_fixed)
        };

        resolved.push_declaration(&mut statements, &body);
        statements.extend(body);
        statements
    }

    fn try_lower_fused_result_let_else(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<Statement>> {
        let fuse = self.result_fuse_plan(scrutinee)?;
        if !fuse.carries_payload() {
            return None;
        }
        let Some(field) = ok_pattern_field(pattern) else {
            return self
                .try_lower_fused_result_let_else_destructure(fuse, pattern, scrutinee, else_block);
        };

        let binding = self.declare_fused_binding(field);
        let slot = match &binding {
            Some((_, go_name)) => CommaOkValueSlot::Named(go_name.clone()),
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot, None);
        let fail_condition = bound.failure_condition();
        Some(self.finish_fused_let_else(bound.statements, fail_condition, binding, else_block))
    }

    fn try_lower_fused_result_let_else_destructure(
        &mut self,
        fuse: ResultFusePlan<'_>,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<Statement>> {
        let (PreludeVariant::Ok, [payload]) = prelude_variant(pattern)? else {
            return None;
        };
        if has_binding_or_pattern(payload) {
            return None;
        }
        let payload_ty = self.facts.peel_alias(&scrutinee.get_type()).ok_type();
        let info = self.collect_pattern_info(payload, &payload_ty);
        if !info.is_irrefutable() {
            return None;
        }
        let binds = info
            .bindings
            .iter()
            .any(|binding| binding.target.is_named());
        let slot = if binds {
            CommaOkValueSlot::Temp
        } else {
            CommaOkValueSlot::Discarded
        };
        let bound = fuse.bind(self, slot, None);
        let value = bound.payload_identifier().cloned();
        let fail_condition = bound.failure_condition();
        let mut statements =
            self.finish_fused_let_else(bound.statements, fail_condition, None, else_block);
        if let Some(value) = value.filter(|_| binds) {
            statements.extend(self.lower_irrefutable_pattern_site(
                PatternSubject::for_identifier(value),
                payload,
                &payload_ty,
            ));
        }
        Some(statements)
    }

    fn try_lower_fused_option_let_else(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<Statement>> {
        let fuse = self.option_fuse_plan(scrutinee)?;
        let field = some_pattern_field(pattern)?;

        let binding = self.declare_fused_binding(field);
        let slot = match &binding {
            Some((_, go_name)) => CommaOkValueSlot::Named(go_name.clone()),
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot);
        let none_condition = bound.failure_condition();
        let late_binding = bound.late_binding();
        let mut statements =
            self.finish_fused_let_else(bound.statements, none_condition, binding, else_block);
        statements.extend(late_binding);
        Some(statements)
    }

    fn finish_fused_let_else(
        &mut self,
        mut statements: Vec<Statement>,
        fail_condition: PairCondition,
        binding: Option<(FusedName<'_>, GoIdentifier)>,
        else_block: &Expression,
    ) -> Vec<Statement> {
        let fail_body = self.lower_block_as_body(else_block);
        statements.push(
            LoweredStatement::If(IfPlan {
                initializer: fail_condition.initializer,
                condition: fail_condition.condition,
                then_body: fail_body,
                else_arm: ElseArm::None,
                else_if_line: None,
                origin: IfOrigin::User,
            })
            .into(),
        );
        if let Some((name, go_name)) = binding {
            self.scope
                .bind_identifier(name.name, name.binding.as_slice(), go_name);
        }
        statements
    }

    fn declare_fused_binding<'p>(
        &mut self,
        field: &'p Pattern,
    ) -> Option<(FusedName<'p>, GoIdentifier)> {
        let name = field_binding(field)
            .flatten()
            .filter(|_| !self.facts.is_unused_binding(field))?;
        let source = name.go_name();
        let preferred = source.preferred();
        let go_name = if self.shadows_declaration(&preferred)
            || self
                .scope
                .binds_go_name_for_other_source_name(&preferred, name.name)
        {
            self.fresh_source(&source)
        } else {
            self.scope.new_local_identifier(preferred)
        };
        self.declare(&go_name);
        Some((name, go_name))
    }

    fn while_let_subject(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
    ) -> (GoExpression, Vec<Statement>) {
        if let Expression::Identifier {
            value, resolution, ..
        } = scrutinee
        {
            let has_collision = pattern_binds_name(pattern, value);
            if self.can_reuse_subject_identifier(value, resolution, has_collision) {
                return (
                    GoExpression::identifier(self.reference_go_identifier(value, resolution)),
                    Vec::new(),
                );
            }
        }
        let staged = self.plan_operand(scrutinee, ExpressionContext::value());
        let (mut setup, value) = staged.into_parts();
        if self
            .field_path_reads_in_place(scrutinee, &value, |root| pattern_binds_name(pattern, root))
        {
            return (value, setup);
        }
        let var = self.fresh_local(Some("subject"));
        setup.push(define(var.clone(), value));
        (GoExpression::identifier(var), setup)
    }

    pub(crate) fn lower_while_let(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        body: &Expression,
    ) -> LoweredBlock {
        if let Some(fused) = self.try_lower_fused_option_while_let(pattern, scrutinee, body) {
            return fused;
        }

        let scrutinee_ty = scrutinee.get_type();
        let (subject_var, subject_setup) = self.while_let_subject(pattern, scrutinee);

        let alternatives = split_binding_or_patterns(pattern);
        let mut loop_body = subject_setup;
        self.lower_while_let_alternatives(
            &mut loop_body,
            &alternatives,
            TypedSubject {
                var: &subject_var,
                ty: &scrutinee_ty,
            },
            body,
        );
        let plan = self.build_source_loop(
            LoopHeader::Infinite,
            LoweredBlock {
                statements: loop_body,
            },
        );
        LoweredBlock {
            statements: vec![LoweredStatement::Loop(plan).into()],
        }
    }

    fn try_lower_fused_option_while_let(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        body: &Expression,
    ) -> Option<LoweredBlock> {
        let fuse = self.option_fuse_plan(scrutinee)?;
        let field = some_pattern_field(pattern)?;
        let binding = self.declare_fused_binding(field);

        let slot = match &binding {
            Some((_, go_name)) => CommaOkValueSlot::Named(go_name.clone()),
            None => CommaOkValueSlot::Unused,
        };
        let bound = fuse.bind(self, slot);
        let none_condition = bound.failure_condition();
        let late_binding = bound.late_binding();

        let mut loop_body = bound.statements;
        loop_body.push(
            LoweredStatement::If(IfPlan {
                initializer: none_condition.initializer,
                condition: none_condition.condition,
                then_body: LoweredBlock {
                    statements: vec![self.loop_break().into()],
                },
                else_arm: ElseArm::None,
                else_if_line: None,
                origin: IfOrigin::User,
            })
            .into(),
        );
        loop_body.extend(late_binding);
        let (body_block, _) = self.lower_fused_arm(
            &[binding
                .as_ref()
                .and_then(|(name, go_name)| ArmBinding::alias_at(Some(*name), Some(go_name)))],
            body,
            &PlacePlan::Statement,
        );
        loop_body.extend(body_block.statements);

        let plan = self.build_source_loop(
            LoopHeader::Infinite,
            LoweredBlock {
                statements: loop_body,
            },
        );
        Some(LoweredBlock {
            statements: vec![LoweredStatement::Loop(plan).into()],
        })
    }

    fn lower_refutable_fail(
        &mut self,
        fail: RefutableFail,
        subject_var: &GoExpression,
    ) -> LoweredBlock {
        match fail {
            RefutableFail::ElseBlock(else_block) => self.lower_block_as_body(else_block),
            RefutableFail::AssertFail(span) => {
                let handle = self
                    .scope
                    .current_test_handle()
                    .expect("let assert without a test handle should be rejected by semantics")
                    .clone();
                let literal = |text: String| GoExpression::literal(text);
                let operand = GoExpression::struct_literal(
                    testkit_operand_type(),
                    vec![(
                        "Value".to_string(),
                        GoExpression::call(
                            GoExpression::generated(GeneratedPackage::Prelude, "Debug"),
                            vec![subject_var.clone()],
                        ),
                    )],
                    CompositeLayout::Inline { padded: false },
                );
                let call = test_context_call(
                    GoExpression::identifier(handle),
                    "FailAssert",
                    span,
                    vec![
                        literal("\"let_assert\"".to_string()),
                        literal("\"pattern did not match\"".to_string()),
                        operand,
                    ],
                );
                LoweredBlock {
                    statements: vec![expression_statement(call)],
                }
            }
        }
    }

    fn lower_let_else_single_pattern(
        &mut self,
        pattern: &Pattern,
        subject: TypedSubject,
        fail: RefutableFail,
        subject_is_fixed: bool,
    ) -> Vec<Statement> {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = self.collect_pattern_info(pattern, subject_ty);

        let mut statements = Vec::new();
        let mut asserted = self.apply_refutable_root_assertion(&mut statements, info, subject_var);

        if subject_is_fixed {
            asserted.checks.retain(|check| {
                let condition = check.render(&mut self.scope, SubjectRoot::Var(&asserted.subject));
                !self.is_condition_established(&condition)
            });
        }

        if asserted.is_irrefutable() {
            self.tree_binding_statements(
                &mut statements,
                &asserted.bindings,
                SubjectRoot::Var(&asserted.subject),
                &[],
            );
            return statements;
        }

        let AssertedPattern {
            subject: effective_subject,
            ok_test,
            checks,
            bindings,
        } = asserted;
        let mut guard_parts: Vec<GoExpression> = Vec::new();
        if let Some(ok) = ok_test {
            guard_parts.push(GoExpression::unary(UnaryOp::Not, ok));
        }
        if !checks.is_empty() {
            let negated = match checks.as_slice() {
                [check] => {
                    check.render_negated(&mut self.scope, SubjectRoot::Var(&effective_subject))
                }
                _ => GoExpression::unary(
                    UnaryOp::Not,
                    render_condition(
                        &mut self.scope,
                        &checks,
                        SubjectRoot::Var(&effective_subject),
                    ),
                ),
            };
            guard_parts.push(negated);
        }
        let guard = guard_parts
            .into_iter()
            .reduce(|left, right| GoExpression::binary(left, BinaryOp::Or, right))
            .expect("a refutable pattern has a check or a type assertion");
        let fail_body = self.lower_refutable_fail(fail, subject_var);
        statements
            .push(LoweredStatement::If(IfPlan::plain(guard, fail_body, ElseArm::None)).into());

        self.tree_binding_statements(
            &mut statements,
            &bindings,
            SubjectRoot::Var(&effective_subject),
            &[],
        );
        statements
    }

    fn lower_let_else_or_pattern(
        &mut self,
        pattern: &Pattern,
        subject: TypedSubject,
        fail: RefutableFail,
    ) -> Vec<Statement> {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let alternatives = match pattern {
            Pattern::Or { patterns, .. } => patterns.as_slice(),
            _ => slice::from_ref(pattern),
        };
        let mut infos: Vec<_> = alternatives
            .iter()
            .flat_map(split_binding_or_patterns)
            .map(|alt| self.collect_pattern_info(&alt, subject_ty))
            .collect();
        let mut bindings: Vec<_> = infos.iter_mut().map(|i| &mut i.bindings).collect();
        decision_tree::share_alternative_bindings(&mut bindings);
        let failure = infos
            .iter()
            .all(|info| !info.is_irrefutable())
            .then(|| self.lower_refutable_fail(fail, subject_var));

        let mut statements = Vec::new();
        let failure_uses = failure
            .as_ref()
            .map(|failure| GoUses::of(&failure.statements));
        self.declare_alternative_bindings(
            &mut statements,
            &infos[0].bindings,
            failure_uses.as_ref(),
        );
        let alternatives = self.prepare_refutable_alternatives(&mut statements, infos, subject_var);
        let mut pieces = Vec::new();
        for alternative in alternatives {
            let mut assigns = Vec::new();
            self.tree_assignment_statements(
                &mut assigns,
                &alternative.bindings,
                &alternative.subject,
            );
            let body = LoweredBlock {
                statements: assigns,
            };
            if alternative.is_irrefutable() {
                if pieces.is_empty() {
                    statements.extend(body.statements);
                } else {
                    statements.push(assemble_if_else_chain(pieces, body).into());
                }
                return statements;
            }
            pieces.push((alternative.condition(&mut self.scope), body));
        }
        statements.push(
            assemble_if_else_chain(pieces, failure.expect("all alternatives are refutable")).into(),
        );
        statements
    }

    fn declare_alternative_bindings(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        failure_uses: Option<&GoUses>,
    ) {
        for binding in bindings {
            let (go_name, ty) = match &binding.target {
                BindingTarget::Unused => {
                    self.scope
                        .bind_source(&binding.lisette_name, &binding.binding_ids, "_");
                    continue;
                }
                BindingTarget::Unit => {
                    self.bind_inline_unit(binding);
                    continue;
                }
                BindingTarget::Named { go_name, ty } => (go_name, ty),
            };
            let go_name = if failure_uses.is_some_and(|uses| uses.contains(&go_name.preferred())) {
                let fresh = self.fresh_declared_source(go_name);
                self.scope
                    .bind_source(&binding.lisette_name, &binding.binding_ids, fresh)
            } else {
                self.claim_declared_binding(&binding.lisette_name, &binding.binding_ids, go_name)
            };
            let go_type = self.go_type(ty);
            statements.push(declare(go_name, go_type));
        }
    }

    fn prepare_refutable_alternatives(
        &mut self,
        statements: &mut Vec<Statement>,
        infos: Vec<PatternInfo>,
        subject: &GoExpression,
    ) -> Vec<AssertedPattern> {
        infos
            .into_iter()
            .map(|info| self.apply_refutable_root_assertion(statements, info, subject))
            .collect()
    }

    fn lower_while_let_alternatives(
        &mut self,
        statements: &mut Vec<Statement>,
        patterns: &[Cow<'_, Pattern>],
        subject: TypedSubject,
        body: &Expression,
    ) {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let mut alternatives: Vec<_> = patterns
            .iter()
            .map(|alt| self.collect_pattern_info(alt, subject_ty))
            .collect();
        let mut bindings: Vec<_> = alternatives.iter_mut().map(|i| &mut i.bindings).collect();
        decision_tree::share_alternative_bindings(&mut bindings);

        let alternatives =
            self.prepare_refutable_alternatives(statements, alternatives, subject_var);
        let mut pieces = Vec::with_capacity(alternatives.len());
        for alternative in alternatives {
            let branch = self.with_scope(|this| {
                let mut branch = Vec::new();
                this.with_tree_bindings(
                    &mut branch,
                    &alternative.bindings,
                    &alternative.subject,
                    body,
                    |this, branch| {
                        branch.extend(this.lower_block_as_body(body).statements);
                    },
                );
                branch
            });

            pieces.push((
                alternative.condition(&mut self.scope),
                LoweredBlock { statements: branch },
            ));
        }

        let terminal = LoweredBlock {
            statements: vec![self.loop_break().into()],
        };
        statements.push(assemble_if_else_chain(pieces, terminal).into());
    }

    pub(crate) fn lower_select_receive_pattern_site(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        body: &Expression,
        default_body: Option<&Expression>,
        place: &PlacePlan,
    ) -> Vec<Statement> {
        self.lower_refutable_arm(subject, pattern, body, place, |this| {
            default_body.map(|default| this.lower_block_to_place(default, place))
        })
    }

    pub(crate) fn lower_select_match_receive_some_site(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        some_body: &Expression,
        match_arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Vec<Statement> {
        self.lower_refutable_arm(subject, pattern, some_body, place, |this| {
            Some(this.lower_none_arm_body(match_arms, place))
        })
    }

    fn lower_refutable_arm(
        &mut self,
        subject: TypedSubject,
        pattern: &Pattern,
        body: &Expression,
        place: &PlacePlan,
        failure: impl FnOnce(&mut Planner) -> Option<LoweredBlock>,
    ) -> Vec<Statement> {
        let TypedSubject {
            var: subject_var,
            ty: subject_ty,
        } = subject;
        let info = self.collect_pattern_info(pattern, subject_ty);
        let mut statements = Vec::new();
        let asserted = self.apply_refutable_root_assertion(&mut statements, info, subject_var);

        if asserted.is_irrefutable() {
            self.with_tree_bindings(
                &mut statements,
                &asserted.bindings,
                &asserted.subject,
                body,
                |this, statements| {
                    let block = this.lower_block_to_place(body, place);
                    statements.extend(block.statements);
                },
            );
            return statements;
        }
        let condition = asserted.condition(&mut self.scope);
        let mut then_body = Vec::new();
        self.with_tree_bindings(
            &mut then_body,
            &asserted.bindings,
            &asserted.subject,
            body,
            |this, then_body| {
                let block = this.lower_block_to_place(body, place);
                then_body.extend(block.statements);
            },
        );
        let else_arm = match failure(self) {
            Some(body) => ElseArm::from_body(body, false),
            None => ElseArm::None,
        };
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                condition,
                LoweredBlock {
                    statements: then_body,
                },
                else_arm,
            ))
            .into(),
        );
        statements
    }

    pub(crate) fn classify_receive_var_pattern(
        &mut self,
        pattern: &Pattern,
    ) -> (GoIdentifier, bool) {
        match pattern {
            Pattern::WildCard { .. } => (GoIdentifier::external("_".to_string()), false),
            Pattern::Identifier {
                identifier,
                binding,
                ..
            } => {
                let Some(go_name) = self.facts.go_name_for_binding(pattern) else {
                    return (GoIdentifier::external("_".to_string()), false);
                };
                let preferred = go_name.preferred();
                if self.scope.resolve_identifier_binding(identifier).is_some()
                    || self
                        .scope
                        .binds_go_name_for_other_source_name(&preferred, identifier)
                    || self.package.is_package_block_name(&preferred)
                {
                    let name = self.fresh_local(Some("recv"));
                    return (name.clone(), true);
                }
                (
                    self.scope
                        .bind_source(identifier, binding.as_slice(), go_name),
                    false,
                )
            }
            _ => {
                let name = self.fresh_local(Some("recv"));
                (name.clone(), true)
            }
        }
    }

    fn loop_break(&self) -> LoweredStatement {
        LoweredStatement::Break(self.current_loop_transfer())
    }

    pub(crate) fn lower_none_arm_body(
        &mut self,
        match_arms: &[MatchArm],
        place: &PlacePlan,
    ) -> LoweredBlock {
        for match_arm in match_arms {
            if matches!(
                prelude_variant(&match_arm.pattern),
                Some((PreludeVariant::None, _))
            ) {
                return self.lower_block_to_place(&match_arm.expression, place);
            }
        }
        LoweredBlock {
            statements: Vec::new(),
        }
    }
}

pub(crate) fn unwrap_some_pattern(pattern: &Pattern) -> &Pattern {
    let pattern = peel_as_binding(pattern);
    some_payload_pattern(pattern).unwrap_or(pattern)
}

pub(crate) fn some_payload_pattern(pattern: &Pattern) -> Option<&Pattern> {
    match prelude_variant(pattern)? {
        (PreludeVariant::Some, [payload]) => Some(payload),
        _ => None,
    }
}

pub(crate) fn is_some_pattern(pattern: &Pattern) -> bool {
    some_payload_pattern(peel_as_binding(pattern)).is_some()
}

fn peel_as_binding(pattern: &Pattern) -> &Pattern {
    match pattern {
        Pattern::AsBinding { pattern, .. } => pattern.as_ref(),
        p => p,
    }
}

fn assemble_if_else_chain(
    mut pieces: Vec<(GoExpression, LoweredBlock)>,
    terminal: LoweredBlock,
) -> LoweredStatement {
    let mut else_arm = ElseArm::from_body(terminal, false);
    while pieces.len() > 1 {
        let (condition, then_body) = pieces.pop().expect("len > 1");
        else_arm = ElseArm::ElseIf(Box::new(IfPlan::plain(condition, then_body, else_arm)));
    }
    let (condition, then_body) = pieces.pop().expect("pieces is non-empty");
    LoweredStatement::If(IfPlan::plain(condition, then_body, else_arm))
}
