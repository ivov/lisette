mod chains;
mod switches;

use crate::target::ir::go_expression::Position;

use syntax::ast::{BinaryOperator, Expression, Literal, MatchArm, UnaryOperator};
use syntax::program::DotAccessResolution;
use syntax::types::Type;

use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::control_flow::branches::Divergence;
use crate::lowering::patterns::binding_declarations::is_catchall_pattern;
use crate::lowering::patterns::decision_tree::{
    AccessPath, ArmLeaf, ChainTail, Decision, GuardedLeaf, PatternBinding, SubjectRoot,
};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::target::ir::bodies::{
    ElseArm, IfOrigin, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock,
    LoweredStatement, Statement, direct_first, directed_first,
};
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::values::GoExpression;

fn guard_renders_inline(facts: &EmitFacts<'_>, guard: &Expression) -> bool {
    match guard {
        Expression::Literal { literal, ty, .. } => {
            matches!(
                literal,
                Literal::Integer { .. }
                    | Literal::Float { .. }
                    | Literal::Imaginary(_)
                    | Literal::Boolean(_)
                    | Literal::String { .. }
                    | Literal::Char(_)
            ) && facts.simple_kind(ty).is_some()
        }
        Expression::Identifier { ty, .. } => facts.simple_kind(ty).is_some(),
        Expression::Unary {
            operator,
            expression,
            ..
        } => {
            matches!(
                operator,
                UnaryOperator::Not | UnaryOperator::Negative | UnaryOperator::BitwiseNot
            ) && guard_renders_inline(facts, expression)
        }
        Expression::Binary {
            left,
            operator,
            right,
            ..
        } => {
            !matches!(operator, BinaryOperator::Pipeline)
                && guard_renders_inline(facts, left)
                && guard_renders_inline(facts, right)
        }
        Expression::Call {
            expression,
            args,
            spread: None,
            ty,
            ..
        } => {
            facts.simple_kind(ty).is_some()
                && callee_renders_inline(facts, expression)
                && args.iter().all(|arg| guard_renders_inline(facts, arg))
        }
        _ => false,
    }
}

fn callee_renders_inline(facts: &EmitFacts<'_>, callee: &Expression) -> bool {
    match callee {
        Expression::Identifier { .. } => true,
        Expression::DotAccess {
            expression,
            resolution,
            ..
        } => {
            matches!(resolution, DotAccessResolution::PackageMember { .. })
                || guard_renders_inline(facts, expression)
        }
        _ => false,
    }
}

fn contains_call(expression: &Expression) -> bool {
    matches!(expression, Expression::Call { .. })
        || expression.children().into_iter().any(contains_call)
}

fn join_and(conditions: &[GoExpression]) -> Option<GoExpression> {
    conditions
        .iter()
        .cloned()
        .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::And, right))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WalkRole<'a> {
    SwitchCase,
    ChainBody,
    RetryLoop {
        top: bool,
        break_label: Option<&'a str>,
    },
}

#[derive(Clone, Copy)]
struct WalkContext<'a> {
    arm_place: &'a PlacePlan<'a>,
    role: WalkRole<'a>,
}

impl<'a> WalkContext<'a> {
    fn switch_case(arm_place: &'a PlacePlan<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::SwitchCase,
        }
    }

    fn chain_test(arm_place: &'a PlacePlan<'a>) -> Self {
        Self {
            arm_place,
            role: WalkRole::ChainBody,
        }
    }

    fn retry_loop(arm_place: &'a PlacePlan<'a>, break_label: Option<&'a str>) -> Self {
        Self {
            arm_place,
            role: WalkRole::RetryLoop {
                top: true,
                break_label,
            },
        }
    }

    fn nested(self) -> Self {
        let role = match self.role {
            WalkRole::SwitchCase | WalkRole::ChainBody => WalkRole::ChainBody,
            WalkRole::RetryLoop { break_label, .. } => WalkRole::RetryLoop {
                top: false,
                break_label,
            },
        };

        Self { role, ..self }
    }

    fn is_grouped_retry(&self) -> bool {
        matches!(self.role, WalkRole::RetryLoop { .. })
    }

    fn leaf_scope_explicit(&self) -> bool {
        matches!(self.role, WalkRole::RetryLoop { top: true, .. })
    }
}

pub(in crate::lowering) enum MatchSubject {
    Var(GoExpression),
    Elements(Vec<GoExpression>),
}

impl MatchSubject {
    pub(in crate::lowering) fn root(&self) -> SubjectRoot<'_> {
        match self {
            Self::Var(var) => SubjectRoot::Var(var),
            Self::Elements(elements) => SubjectRoot::Elements(elements),
        }
    }
}

pub(in crate::lowering) struct TreePlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    arms: &'a [MatchArm],
    subject: MatchSubject,
    subject_ty: Type,
}

impl<'a, 'e> TreePlanner<'a, 'e> {
    pub(in crate::lowering) fn new(
        planner: &'a mut Planner<'e>,
        arms: &'a [MatchArm],
        subject_var: MatchSubject,
        subject_ty: Type,
    ) -> Self {
        Self {
            planner,
            arms,
            subject: subject_var,
            subject_ty,
        }
    }

    pub(in crate::lowering) fn lower(mut self, place: &PlacePlan) -> LoweredBlock {
        let tree = self.planner.compile_match_arms(self.arms, &self.subject_ty);

        let mut statements: Vec<Statement> = Vec::new();
        match &tree {
            Decision::Switch(_) | Decision::TypeSwitch { .. } => {
                let ctx = WalkContext::switch_case(place);
                self.walk(&mut statements, &tree, &ctx);
            }
            Decision::Success(leaf) => {
                self.render_single_catchall(&mut statements, leaf, place);
            }
            _ if self.arms.iter().any(|arm| arm.has_guard()) => {
                self.render_retry_loop(&mut statements, &tree, place);
            }
            Decision::Chain { tests, tail } => {
                self.render_chain_root(&mut statements, tests, tail, place);
            }
            Decision::Unreachable => {
                self.render_chain_root(&mut statements, &[], &ChainTail::Open, place);
            }
            Decision::Guard(_) => unreachable!("a guard root implies a guarded arm"),
        }
        LoweredBlock { statements }
    }

    fn with_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.planner.scope.enter_block();
        let result = f(self);
        self.planner.scope.exit_block();
        result
    }

    fn with_binding_frame<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.planner.scope.push_binding_frame();
        let result = f(self);
        self.planner.scope.pop_binding_frame();
        result
    }

    fn with_optional_scope<R>(&mut self, scoped: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        if scoped { self.with_scope(f) } else { f(self) }
    }

    fn render_single_catchall(
        &mut self,
        statements: &mut Vec<Statement>,
        leaf: &ArmLeaf,
        place: &PlacePlan,
    ) {
        let ArmLeaf {
            arm_index,
            bindings,
        } = leaf;
        let arm_index = *arm_index;
        let pattern_has_collisions = self
            .planner
            .queries()
            .pattern_has_binding_collisions(&self.arms[arm_index].pattern);
        let arm_body = &*self.arms[arm_index].expression;

        let (inner, needs_block) = self.with_scope(|this| {
            let mut inner: Vec<Statement> = Vec::new();
            this.with_bindings(&mut inner, bindings, &[arm_body], |this, inner| {
                this.lower_arm_body(inner, arm_index, place)
            });
            let needs_block =
                this.planner.scope.current_block_declared_nonempty() || pattern_has_collisions;
            (inner, needs_block)
        });

        if needs_block {
            statements.push(LoweredStatement::Block(LoweredBlock { statements: inner }).into());
        } else {
            statements.extend(inner);
        }
    }

    fn arm_line(&self, arm_index: usize) -> Option<Position> {
        self.planner
            .facts
            .source_position(&self.arms[arm_index].pattern.get_span())
    }

    fn decision_line(&self, decision: Option<&Decision>) -> Option<Position> {
        decision
            .and_then(Decision::first_arm)
            .and_then(|arm_index| self.arm_line(arm_index))
    }

    fn render_retry_loop(
        &mut self,
        statements: &mut Vec<Statement>,
        tree: &Decision,
        place: &PlacePlan,
    ) {
        let all_arms_diverge = self
            .arms
            .iter()
            .all(|arm| self.planner.facts.is_never(&arm.expression.get_type()));
        let root_has_unguarded_terminal = tree.has_unguarded_terminal();
        let last_arm_is_any_catchall = self
            .arms
            .last()
            .is_some_and(|arm| !arm.has_guard() && is_catchall_pattern(&arm.pattern));

        let use_direct_return = place.is_return();
        let unguarded_exit = root_has_unguarded_terminal || last_arm_is_any_catchall;
        let skip_wrapper = !use_direct_return && unguarded_exit && all_arms_diverge;

        if use_direct_return || skip_wrapper {
            let ctx = WalkContext::retry_loop(place, None);
            self.walk(statements, tree, &ctx);
            if use_direct_return && !root_has_unguarded_terminal {
                statements.push(LoweredStatement::UnreachablePanic.into());
            }
            return;
        }

        if self.guarded_tree_flattens(tree)
            && self.render_conditional_switch(statements, tree, place)
        {
            return;
        }

        let label = self
            .planner
            .scope
            .fresh_binding_go_name("match", self.planner.names);
        let ctx = WalkContext::retry_loop(place, Some(label.as_str()));
        let mut body: Vec<Statement> = Vec::new();
        self.walk(&mut body, tree, &ctx);
        if !unguarded_exit {
            body.push(LoweredStatement::Break(LoopTransfer::Labeled(label.clone())).into());
        }
        statements.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: Some(label) },
                header: LoopHeader::Infinite,
                body: LoweredBlock { statements: body },
            })
            .into(),
        );
    }

    fn walk(&mut self, statements: &mut Vec<Statement>, decision: &Decision, ctx: &WalkContext) {
        match decision {
            Decision::Success(leaf) => self.walk_leaf(statements, leaf, ctx),
            Decision::Guard(guard) => self.walk_guard(statements, guard, ctx),
            Decision::Switch(switch) => self.walk_switch(statements, switch, ctx),
            Decision::TypeSwitch { branches, fallback } => {
                let subject = AccessPath::root().render(self.subject.root());
                let switch =
                    self.lower_type_switch(subject, branches, fallback.as_deref(), ctx.arm_place);
                let divergence = capture_diverge(switch, statements);
                apply_leaf_terminator(statements, ctx, divergence);
            }
            Decision::Chain { tests, tail } => {
                if ctx.is_grouped_retry() {
                    self.lower_chain_grouped(statements, tests, tail, ctx);
                } else {
                    self.lower_chain_branch(statements, tests, tail, ctx.arm_place);
                }
            }
            Decision::Unreachable => {}
        }
    }

    fn walk_leaf(&mut self, statements: &mut Vec<Statement>, leaf: &ArmLeaf, ctx: &WalkContext) {
        let wrap = ctx.leaf_scope_explicit();
        let arm_body = &*self.arms[leaf.arm_index].expression;
        let (lowered, declares) = self.with_optional_scope(wrap, |this| {
            let mut lowered: Vec<Statement> = Vec::new();
            this.with_bindings(
                &mut lowered,
                &leaf.bindings,
                &[arm_body],
                |this, lowered| {
                    this.lower_arm_leaf(lowered, leaf.arm_index, ctx);
                },
            );
            (
                lowered,
                this.planner.scope.current_block_declared_nonempty(),
            )
        });

        if wrap && declares {
            statements.push(
                LoweredStatement::Block(LoweredBlock {
                    statements: lowered,
                })
                .into(),
            );
        } else {
            statements.extend(lowered);
        }
    }

    fn lower_arm_leaf(
        &mut self,
        statements: &mut Vec<Statement>,
        arm_index: usize,
        ctx: &WalkContext,
    ) {
        let mut body_statements: Vec<Statement> = Vec::new();
        self.lower_arm_body(&mut body_statements, arm_index, ctx.arm_place);
        let divergence = capture_diverge(body_statements, statements);
        apply_leaf_terminator(statements, ctx, divergence);
    }

    fn walk_condition_branch(
        &mut self,
        statements: &mut Vec<Statement>,
        test: BranchTest,
        then_branch: &Decision,
        else_branch: &Decision,
        ctx: &WalkContext,
    ) {
        let inner = WalkContext::switch_case(ctx.arm_place);
        let then_statements = self.with_scope(|this| {
            this.planner
                .scope
                .establish_condition(test.condition.clone());
            let mut then_statements: Vec<Statement> = Vec::new();
            this.walk(&mut then_statements, then_branch, &inner);
            then_statements
        });

        let then_body = LoweredBlock {
            statements: then_statements,
        };
        let then_divergence = Divergence::of(&then_body);
        let else_arm = self.lower_else_or_flat(else_branch, &inner, then_divergence);
        let plan = IfPlan {
            initializer: None,
            condition: test.condition,
            then_body,
            else_arm,
            else_if_line: None,
            origin: IfOrigin::Match {
                else_label: test.else_label,
            },
        };

        let divergence = capture_diverge(vec![LoweredStatement::If(plan).into()], statements);
        apply_leaf_terminator(statements, ctx, divergence);
    }

    fn walk_guard(
        &mut self,
        statements: &mut Vec<Statement>,
        guard: &GuardedLeaf,
        ctx: &WalkContext,
    ) {
        let needs_pre_scope = ctx.leaf_scope_explicit() && !guard.leaf.bindings.is_empty();
        let guard_statements =
            self.with_optional_scope(needs_pre_scope, |this| this.lower_guarded_leaf(guard, ctx));

        if needs_pre_scope {
            statements.push(
                LoweredStatement::Block(LoweredBlock {
                    statements: guard_statements,
                })
                .into(),
            );
        } else {
            statements.extend(guard_statements);
        }
        if ctx.leaf_scope_explicit() {
            self.walk(statements, &guard.failure, ctx);
        }
    }

    fn lower_guarded_leaf(&mut self, guard: &GuardedLeaf, ctx: &WalkContext) -> Vec<Statement> {
        let GuardedLeaf {
            leaf: ArmLeaf {
                arm_index,
                bindings,
            },
            failure,
        } = guard;

        let arm_index = *arm_index;
        let arm = &self.arms[arm_index];
        let arm_body = &*arm.expression;
        let mut guard_consumers: Vec<&Expression> = Vec::with_capacity(2);
        if let Some(guard) = arm.guard.as_deref() {
            guard_consumers.push(guard);
        }
        guard_consumers.push(arm_body);

        let mut guard_statements: Vec<Statement> = Vec::new();
        let guarded = self.with_bindings(
            &mut guard_statements,
            bindings,
            &guard_consumers,
            |this, _| {
                let (condition_setup, condition) = this.try_plan_guard_condition(arm_index)?;
                let then_body = this.with_scope(|this| {
                    let mut then_statements: Vec<Statement> = Vec::new();
                    this.lower_arm_leaf(&mut then_statements, arm_index, &ctx.nested());
                    LoweredBlock {
                        statements: then_statements,
                    }
                });

                Some((condition_setup, condition, then_body))
            },
        );

        if let Some((condition_setup, condition, then_body)) = guarded {
            let success_divergence = Divergence::of(&then_body);
            let else_arm = if ctx.role == WalkRole::SwitchCase {
                self.lower_else_or_flat(failure, ctx, success_divergence)
            } else {
                ElseArm::None
            };
            let mut guarded = condition_setup;
            guarded.push(
                LoweredStatement::If(IfPlan::for_match(condition, then_body, else_arm)).into(),
            );
            guard_statements.extend(directed_first(self.arm_line(arm_index), guarded));
        }
        guard_statements
    }

    fn lower_else_or_flat(
        &mut self,
        decision: &Decision,
        ctx: &WalkContext,
        preceding: Divergence,
    ) -> ElseArm {
        match decision {
            Decision::Success(leaf) => self.lower_leaf_else_or_flat(leaf, ctx, preceding),
            _ => self.lower_else_body(preceding, decision.first_arm(), |this, body| {
                this.walk(body, decision, ctx)
            }),
        }
    }

    fn lower_leaf_else_or_flat(
        &mut self,
        leaf: &ArmLeaf,
        ctx: &WalkContext,
        preceding: Divergence,
    ) -> ElseArm {
        if leaf.bindings.is_empty() && body_is_unit_or_empty(&self.arms[leaf.arm_index].expression)
        {
            return ElseArm::None;
        }
        self.lower_else_body(preceding, Some(leaf.arm_index), |this, body| {
            this.walk_leaf(body, leaf, ctx)
        })
    }

    fn lower_else_body(
        &mut self,
        preceding: Divergence,
        arm_index: Option<usize>,
        lower: impl FnOnce(&mut Self, &mut Vec<Statement>),
    ) -> ElseArm {
        let line = arm_index.and_then(|arm_index| self.arm_line(arm_index));
        if preceding == Divergence::Diverges {
            let mut body: Vec<Statement> = Vec::new();
            lower(self, &mut body);
            let statements = directed_first(line, body);
            return ElseArm::inline(LoweredBlock { statements });
        }
        let body = self.with_scope(|this| {
            let mut body: Vec<Statement> = Vec::new();
            lower(this, &mut body);
            body
        });
        let statements = directed_first(line, body);
        ElseArm::block(LoweredBlock { statements })
    }

    fn with_bindings<R>(
        &mut self,
        statements: &mut Vec<Statement>,
        bindings: &[PatternBinding],
        consumers: &[&Expression],
        f: impl FnOnce(&mut Self, &mut Vec<Statement>) -> R,
    ) -> R {
        self.with_binding_frame(|this| {
            if !bindings.is_empty() {
                let start = statements.len();
                this.planner.tree_binding_statements(
                    statements,
                    bindings,
                    this.subject.root(),
                    consumers,
                );
                let line = consumers
                    .first()
                    .and_then(|consumer| this.planner.facts.source_position(&consumer.get_span()));
                direct_first(&mut statements[start..], line);
            }
            f(this, statements)
        })
    }

    fn lower_arm_body(
        &mut self,
        statements: &mut Vec<Statement>,
        arm_index: usize,
        place: &PlacePlan,
    ) {
        let arm = &self.arms[arm_index];
        let block = self.planner.lower_block_to_place(&arm.expression, place);
        statements.extend(block.statements);
    }

    fn try_plan_guard_condition(
        &mut self,
        arm_index: usize,
    ) -> Option<(Vec<Statement>, GoExpression)> {
        let guard_expression = self.arms[arm_index].guard.as_deref()?;
        let plan = self
            .planner
            .plan_operand(guard_expression, ExpressionContext::value());
        let (setup, value) = plan.into_parts();
        Some((setup, value))
    }
}

fn body_is_unit_or_empty(expression: &Expression) -> bool {
    matches!(expression, Expression::Unit { .. })
        || matches!(expression, Expression::Block { items, .. } if items.is_empty())
}

fn merge_binding_ids(bindings: &mut [PatternBinding], alternatives: &[PatternBinding]) {
    for alternative in alternatives {
        let Some(binding) = bindings
            .iter_mut()
            .find(|binding| binding.lisette_name == alternative.lisette_name)
        else {
            continue;
        };

        for id in &alternative.binding_ids {
            if !binding.binding_ids.contains(id) {
                binding.binding_ids.push(*id);
            }
        }
    }
}

fn capture_diverge(body_statements: Vec<Statement>, statements: &mut Vec<Statement>) -> Divergence {
    let block = LoweredBlock {
        statements: body_statements,
    };
    let divergence = Divergence::of(&block);
    statements.extend(block.statements);
    divergence
}

fn apply_leaf_terminator(
    statements: &mut Vec<Statement>,
    ctx: &WalkContext,
    divergence: Divergence,
) {
    if let WalkRole::RetryLoop {
        break_label: Some(label),
        ..
    } = ctx.role
        && divergence == Divergence::FallsThrough
    {
        statements.push(LoweredStatement::Break(LoopTransfer::Labeled(label.to_string())).into());
    }
}

struct BranchTest {
    condition: GoExpression,
    else_label: Option<GoExpression>,
}
