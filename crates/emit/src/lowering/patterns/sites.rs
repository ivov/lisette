use crate::lowering::abi::fallible_producer::ResultFusePlan;
use crate::lowering::expressions::operand_order::is_unmutated_identifier;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::bodies::effect_statement;
use std::borrow::Cow;
use std::slice;

use crate::lowering::patterns::binding_declarations::{
    has_binding_or_pattern, split_binding_or_patterns,
};
use crate::target::ir::bodies::GoUses;
use crate::target::ir::go_expression::{GoBinaryOperator, GoExpressionNode, GoUnaryOperator};

use syntax::ast::{Expression, Pattern, Span};
use syntax::types::Type;

use crate::Planner;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::patterns::binding_statements::AssertedPattern;
use crate::lowering::patterns::decision_tree::{
    self, BindingTarget, Check, PatternBinding, PatternInfo, SubjectRoot, render_condition,
};
use crate::lowering::patterns::matching::{
    ArmBinding, FusedName, field_binding, ok_pattern_field, some_pattern_field,
};
use crate::lowering::statements::asserts::{test_context_call, testkit_operand_type};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::{PlacePlan, ValuePlan};
use crate::target::ir::bodies::PairCondition;
use crate::target::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoweredBlock, LoweredStatement, Statement, declare,
    define,
};
use crate::target::ir::go_expression::CompositeLayout;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::{PreludeVariant, prelude_variant};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;

#[derive(Clone, Copy)]
pub(in crate::lowering) struct TypedSubject<'a> {
    pub(in crate::lowering) var: &'a GoExpression,
    pub(in crate::lowering) ty: &'a Type,
}

#[derive(Clone, Copy)]
enum RefutableFail<'a> {
    ElseBlock(&'a Expression),
    AssertFail(Span),
}

pub(in crate::lowering) enum PatternSubject<'a> {
    Existing {
        var: GoExpression,
    },
    Expression {
        scrutinee: &'a Expression,
        temp_hint: &'static str,
    },
}

impl<'a> PatternSubject<'a> {
    pub(in crate::lowering) fn for_identifier(var: GoIdentifier) -> Self {
        Self::Existing {
            var: GoExpression::identifier(var),
        }
    }

    pub(in crate::lowering) fn expression(
        scrutinee: &'a Expression,
        temp_hint: &'static str,
    ) -> Self {
        Self::Expression {
            scrutinee,
            temp_hint,
        }
    }
}

pub(super) enum ResolvedSubject {
    Existing(GoExpression),
    Deferred {
        name: GoIdentifier,
        expression: GoExpression,
    },
}

impl ResolvedSubject {
    pub(super) fn var(&self) -> GoExpression {
        match self {
            Self::Existing(subject) => subject.clone().without_positions(),
            Self::Deferred { name, .. } => GoExpression::identifier(name.clone()),
        }
    }

    pub(super) fn push_declaration(self, statements: &mut Vec<Statement>) {
        if let Self::Deferred { name, expression } = self {
            statements.push(define(name, expression));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SubjectGuards {
    Absent,
    Present,
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
    fn resolve_pattern_subject(
        &mut self,
        setup: &mut Vec<Statement>,
        subject: PatternSubject<'_>,
    ) -> ResolvedSubject {
        match subject {
            PatternSubject::Existing { var } => ResolvedSubject::Existing(var),
            PatternSubject::Expression {
                scrutinee,
                temp_hint,
            } => self.resolve_subject(setup, scrutinee, SubjectGuards::Absent, temp_hint),
        }
    }

    pub(super) fn resolve_subject(
        &mut self,
        setup: &mut Vec<Statement>,
        scrutinee: &Expression,
        guards: SubjectGuards,
        temp_hint: &'static str,
    ) -> ResolvedSubject {
        let plan = self.plan_composite_value(scrutinee, ExpressionContext::value());
        let reads_in_place = self.queries().subject_reads_in_place(&plan, guards);
        let (subject_setup, expression) = plan.into_parts();
        setup.extend(subject_setup);
        if reads_in_place {
            return ResolvedSubject::Existing(expression);
        }
        let name = self.fresh_declared(temp_hint);
        ResolvedSubject::Deferred { name, expression }
    }

    pub(in crate::lowering) fn lower_irrefutable_pattern_site(
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

        resolved.push_declaration(&mut statements);
        statements.extend(body);
        statements
    }

    pub(in crate::lowering) fn lower_let_else_pattern_site(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Vec<Statement> {
        self.lower_refutable_let_site(pattern, scrutinee, RefutableFail::ElseBlock(else_block))
    }

    pub(in crate::lowering) fn lower_let_assert_pattern_site(
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
            PatternSubject::expression(scrutinee, "subject"),
        );
        let subject = TypedSubject {
            var: &resolved.var(),
            ty: &value_ty,
        };

        let subject_is_fixed = is_unmutated_identifier(&self.facts, scrutinee);
        let body = if matches!(pattern, Pattern::Or { .. }) || has_binding_or_pattern(pattern) {
            self.lower_let_else_or_pattern(pattern, subject, fail)
        } else {
            self.lower_let_else_single_pattern(pattern, subject, fail, subject_is_fixed)
        };

        resolved.push_declaration(&mut statements);
        statements.extend(body);
        statements
    }

    fn try_lower_fused_result_let_else(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        else_block: &Expression,
    ) -> Option<Vec<Statement>> {
        let fuse = self.queries().result_fuse_plan(scrutinee)?;
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
        let payload_ty = self.facts.ok_payload(&scrutinee.get_type());
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
        let fuse = self.queries().option_fuse_plan(scrutinee)?;
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
            LoweredStatement::If(IfPlan::pair_if(
                fail_condition,
                fail_body,
                ElseArm::None,
                IfOrigin::User,
            ))
            .into(),
        );
        if let Some((name, go_name)) = binding {
            self.scope
                .bind_existing_identifier(name.name, name.binding.as_slice(), go_name);
        }
        statements
    }

    fn declare_fused_binding<'p>(
        &mut self,
        field: &'p Pattern,
    ) -> Option<(FusedName<'p>, GoIdentifier)> {
        let name = field_binding(field).flatten()?;
        let source = name.go_name();
        let preferred = source.preferred();
        let go_name = if self.queries().shadows_declaration(&preferred)
            || self
                .scope
                .go_name_bound_by_other_lisette_name(&preferred, name.name)
        {
            self.fresh_local(&source)
        } else {
            self.scope.new_local_identifier(preferred)
        };

        self.scope.declare_go_name(&go_name);
        Some((name, go_name))
    }

    pub(in crate::lowering) fn lower_while_let(
        &mut self,
        pattern: &Pattern,
        scrutinee: &Expression,
        body: &Expression,
    ) -> LoweredBlock {
        if let Some(fused) = self.try_lower_fused_option_while_let(pattern, scrutinee, body) {
            return fused;
        }

        let scrutinee_ty = scrutinee.get_type();
        let mut loop_body = Vec::new();
        let subject =
            self.resolve_subject(&mut loop_body, scrutinee, SubjectGuards::Absent, "subject");
        let subject_var = subject.var();
        subject.push_declaration(&mut loop_body);

        let alternatives = split_binding_or_patterns(pattern);
        self.lower_while_let_alternatives(
            &mut loop_body,
            &alternatives,
            TypedSubject {
                var: &subject_var,
                ty: &scrutinee_ty,
            },
            body,
        );
        let plan = self.queries().build_source_loop(
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
        let fuse = self.queries().option_fuse_plan(scrutinee)?;
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
            LoweredStatement::If(IfPlan::pair_if(
                none_condition,
                LoweredBlock {
                    statements: vec![self.queries().loop_break().into()],
                },
                ElseArm::None,
                IfOrigin::User,
            ))
            .into(),
        );
        loop_body.extend(late_binding);
        let body_block = self.lower_fused_arm(
            &[binding
                .as_ref()
                .and_then(|(name, go_name)| ArmBinding::alias_at(Some(*name), Some(go_name)))],
            body,
            &PlacePlan::Statement,
        );

        loop_body.extend(body_block.statements);

        let plan = self.queries().build_source_loop(
            LoopHeader::Infinite,
            LoweredBlock {
                statements: loop_body,
            },
        );

        Some(LoweredBlock {
            statements: vec![LoweredStatement::Loop(plan).into()],
        })
    }

    fn record_dropped_fail(&self, fail: &RefutableFail) {
        if let RefutableFail::ElseBlock(else_block) = fail {
            self.conversions.record_dropped_code(else_block.get_span());
        }
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
                let literal = |text: String| GoExpression::token(text);
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
                    statements: vec![effect_statement(call)],
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
                !self.scope.is_condition_established(&condition)
            });
        }

        let AssertedPattern {
            subject: effective_subject,
            ok_test,
            checks,
            bindings,
        } = asserted;

        match self.refutable_failure_guard(ok_test, &checks, &effective_subject) {
            Some(guard) => {
                let fail_body = self.lower_refutable_fail(fail, subject_var);
                statements.push(
                    LoweredStatement::If(IfPlan::plain(guard, fail_body, ElseArm::None)).into(),
                );
            }
            None => self.record_dropped_fail(&fail),
        }

        self.tree_binding_statements(
            &mut statements,
            &bindings,
            SubjectRoot::Var(&effective_subject),
            &[],
        );
        statements
    }

    fn refutable_failure_guard(
        &mut self,
        ok_test: Option<GoExpression>,
        checks: &[Check],
        subject: &GoExpression,
    ) -> Option<GoExpression> {
        let negated_ok = ok_test.map(|ok| GoExpression::unary(GoUnaryOperator::Not, ok));
        let negated_checks = match checks {
            [] => None,
            [check] => Some(check.render_negated(&mut self.scope, SubjectRoot::Var(subject))),
            _ => Some(GoExpression::unary(
                GoUnaryOperator::Not,
                render_condition(&mut self.scope, checks, SubjectRoot::Var(subject)),
            )),
        };
        negated_ok
            .into_iter()
            .chain(negated_checks)
            .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::Or, right))
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
            .map(|alternative_pattern| self.collect_pattern_info(&alternative_pattern, subject_ty))
            .collect();
        let mut bindings: Vec<_> = infos.iter_mut().map(|i| &mut i.bindings).collect();
        decision_tree::share_alternative_bindings(&mut bindings);
        let refutable = infos.iter().all(|info| !info.is_irrefutable());
        if !refutable {
            self.record_dropped_fail(&fail);
        }
        let failure = refutable.then(|| self.lower_refutable_fail(fail, subject_var));

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
                statements.extend(assemble_if_else_chain(pieces, body));
                return statements;
            }
            pieces.push((alternative.condition(&mut self.scope), body));
        }
        statements.extend(assemble_if_else_chain(
            pieces,
            failure.expect("all alternatives are refutable"),
        ));
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
                        .bind_new_local(&binding.lisette_name, &binding.binding_ids, "_");
                    continue;
                }
                BindingTarget::Unit => {
                    self.bind_inline_unit(binding);
                    continue;
                }
                BindingTarget::Named { go_name, ty } => (go_name, ty),
            };

            let go_name = if failure_uses.is_some_and(|uses| uses.contains(&go_name.preferred())) {
                let fresh = self.fresh_declared(go_name);
                self.scope
                    .bind_new_local(&binding.lisette_name, &binding.binding_ids, fresh)
            } else {
                self.claim_declared_binding(&binding.lisette_name, &binding.binding_ids, go_name)
            };

            let go_type = self.queries().go_type(ty);
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
            .map(|alternative_pattern| self.collect_pattern_info(alternative_pattern, subject_ty))
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
            statements: vec![self.queries().loop_break().into()],
        };
        statements.extend(assemble_if_else_chain(pieces, terminal));
    }
}

fn assemble_if_else_chain(
    pieces: Vec<(GoExpression, LoweredBlock)>,
    terminal: LoweredBlock,
) -> Vec<Statement> {
    let mut pieces = pieces.into_iter();
    let Some((condition, then_body)) = pieces.next() else {
        return terminal.statements;
    };
    let else_arm = pieces.rev().fold(
        ElseArm::block(terminal),
        |else_arm, (condition, then_body)| {
            ElseArm::ElseIf(Box::new(IfPlan::plain(condition, then_body, else_arm)))
        },
    );
    vec![LoweredStatement::If(IfPlan::plain(condition, then_body, else_arm)).into()]
}

impl PlannerQueries<'_, '_> {
    fn subject_reads_in_place(&self, plan: &ValuePlan, guards: SubjectGuards) -> bool {
        let Some(expression) = plan.expression() else {
            return false;
        };
        if expression.is_literal() && !expression.is_composite_literal() {
            return true;
        }
        match expression.node() {
            GoExpressionNode::Identifier(name) if name.id().is_some() => {
                guards == SubjectGuards::Absent || self.plan_rests_in_stable_name(plan)
            }
            GoExpressionNode::Identifier(_) | GoExpressionNode::Qualified { .. } => {
                self.plan_rests_in_stable_name(plan)
            }
            _ => {
                guards == SubjectGuards::Absent
                    && field_path_go_root(expression).is_some_and(|root| root.id().is_some())
            }
        }
    }

    fn loop_break(&self) -> LoweredStatement {
        LoweredStatement::Break(self.current_loop_transfer())
    }
}
