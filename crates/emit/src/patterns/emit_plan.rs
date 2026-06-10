use syntax::ast::MatchArm;

use crate::EmitEffects;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::patterns::binding_decls::{is_catchall_pattern, is_unconditional_catchall};
use crate::patterns::decision_tree::{
    AccessPath, ChainTest, Decision, PatternBinding, SwitchBranch, SwitchKind, SwitchShape,
    decision_is_exhaustive, render_condition, tree_has_unguarded_terminal,
};

#[derive(Debug, Clone)]
pub(crate) struct EmitBinding {
    pub lisette_name: String,
    pub go_name: Option<String>,
    pub rendered_access: String,
    pub composable_access: String,
    pub subject: String,
}

#[derive(Debug)]
pub(crate) enum MatchEmitPlan {
    Switch { tree: EmitDecision },
    SingleCatchall(SingleCatchallPlan),
    Chain(ChainPlan),
    RetryLoop(RetryLoopPlan),
}

#[derive(Debug)]
pub(crate) struct SingleCatchallPlan {
    pub arm_index: usize,
    pub bindings: Vec<EmitBinding>,
    pub pattern_has_collisions: bool,
}

#[derive(Debug)]
pub(crate) struct ChainPlan {
    pub tree: EmitDecision,
    /// Planner emits trailing `panic("unreachable")` iff
    /// `destination.is_tail() && !chain_tail_is_exhaustive`.
    pub chain_tail_is_exhaustive: bool,
}

#[derive(Debug)]
pub(crate) struct RetryLoopPlan {
    pub tree: EmitDecision,
    pub all_arms_diverge: bool,
    pub root_has_unguarded_terminal: bool,
    pub last_arm_is_any_catchall: bool,
}

#[derive(Debug)]
pub(crate) enum EmitDecision {
    Success {
        arm_index: usize,
        bindings: Vec<EmitBinding>,
        leaf_emits_no_output: bool,
    },
    Guard {
        arm_index: usize,
        bindings: Vec<EmitBinding>,
        success: Box<EmitDecision>,
        failure: Box<EmitDecision>,
    },
    IfElse {
        condition: String,
        /// Subject the condition reads, for liveness recording.
        subject: Option<String>,
        then_branch: Box<EmitDecision>,
        else_branch: Option<Box<EmitDecision>>,
    },
    /// Exhaustive single-variant enum: no condition wrapper.
    InlineBranch {
        branch: Box<EmitDecision>,
    },
    Switch {
        expr: String,
        /// Subject the switch projects from, for liveness recording.
        subject: String,
        cases: Vec<EmitCase>,
        default: Option<Box<EmitDecision>>,
    },
    TypeSwitch {
        base: String,
        /// Subject the switch projects from, for liveness recording.
        subject: String,
        cases: Vec<EmitCase>,
        default: Option<Box<EmitDecision>>,
    },
    Chain {
        tests: Vec<EmitChainTest>,
        fallback: Box<EmitDecision>,
        last_is_catchall: bool,
    },
    Unreachable,
}

#[derive(Debug)]
pub(crate) struct EmitChainTest {
    /// `None` marks a catchall test (empty `checks` in the source).
    pub condition: Option<String>,
    /// Subject the condition reads (`None` for catchalls).
    pub subject: Option<String>,
    pub decision: Box<EmitDecision>,
}

#[derive(Debug)]
pub(crate) struct EmitCase {
    pub case_label: String,
    pub decision: Box<EmitDecision>,
}

pub(crate) type MatchPlanEffects = EmitEffects;

#[derive(Debug)]
pub(crate) struct LoweredMatch {
    pub plan: MatchEmitPlan,
    pub effects: MatchPlanEffects,
}

pub(crate) struct LoweringCtx<'a> {
    arms: &'a [MatchArm],
    current_subject: String,
    effects: MatchPlanEffects,
}

impl<'a> LoweringCtx<'a> {
    fn new(arms: &'a [MatchArm], subject_var: String) -> Self {
        Self {
            arms,
            current_subject: subject_var,
            effects: MatchPlanEffects::default(),
        }
    }

    /// Scope a subject swap to the closure so a nested type switch's
    /// `base` cannot leak into a later outer branch.
    fn with_subject<R>(&mut self, new_subject: String, f: impl FnOnce(&mut Self) -> R) -> R {
        let prev = std::mem::replace(&mut self.current_subject, new_subject);
        let result = f(self);
        self.current_subject = prev;
        result
    }
}

/// Precedence: Switch, SingleCatchall, RetryLoop (if any guard), else Chain.
///
/// `single_catchall_has_collisions` is precomputed at the call site (it
/// needs emitter scope state). Only read on the SingleCatchall path.
pub(crate) fn lower_match(
    arms: &[MatchArm],
    subject_var: String,
    decision: &Decision,
    single_catchall_has_collisions: bool,
) -> LoweredMatch {
    let mut ctx = LoweringCtx::new(arms, subject_var);
    let plan = lower_match_inner(&mut ctx, decision, single_catchall_has_collisions);
    LoweredMatch {
        plan,
        effects: ctx.effects,
    }
}

fn lower_match_inner(
    ctx: &mut LoweringCtx,
    decision: &Decision,
    single_catchall_has_collisions: bool,
) -> MatchEmitPlan {
    if matches!(decision, Decision::Switch { .. }) {
        return MatchEmitPlan::Switch {
            tree: lower_decision(ctx, decision),
        };
    }

    if let Decision::Success {
        arm_index,
        bindings,
    } = decision
    {
        let bindings = lower_bindings(ctx, bindings);
        return MatchEmitPlan::SingleCatchall(SingleCatchallPlan {
            arm_index: *arm_index,
            bindings,
            pattern_has_collisions: single_catchall_has_collisions,
        });
    }

    let has_guards = ctx.arms.iter().any(|arm| arm.has_guard());
    if has_guards {
        let all_arms_diverge = ctx
            .arms
            .iter()
            .all(|arm| arm.expression.diverges().is_some());
        let root_has_unguarded_terminal = tree_has_unguarded_terminal(decision);
        let last_arm_is_any_catchall = ctx
            .arms
            .last()
            .is_some_and(|arm| !arm.has_guard() && is_catchall_pattern(&arm.pattern));
        let tree = lower_decision(ctx, decision);
        return MatchEmitPlan::RetryLoop(RetryLoopPlan {
            tree,
            all_arms_diverge,
            root_has_unguarded_terminal,
            last_arm_is_any_catchall,
        });
    }

    let chain_tail_is_exhaustive = decision_is_exhaustive(decision)
        || ctx
            .arms
            .last()
            .is_some_and(|arm| !arm.has_guard() && is_unconditional_catchall(&arm.pattern));
    let tree = lower_decision(ctx, decision);
    MatchEmitPlan::Chain(ChainPlan {
        tree,
        chain_tail_is_exhaustive,
    })
}

pub(crate) fn lower_decision(ctx: &mut LoweringCtx, decision: &Decision) -> EmitDecision {
    match decision {
        Decision::Unreachable => EmitDecision::Unreachable,

        Decision::Success {
            arm_index,
            bindings,
        } => {
            let bindings = lower_bindings(ctx, bindings);
            let arm = &ctx.arms[*arm_index];
            let leaf_emits_no_output =
                bindings.is_empty() && body_is_unit_or_empty(&arm.expression);
            EmitDecision::Success {
                arm_index: *arm_index,
                bindings,
                leaf_emits_no_output,
            }
        }

        Decision::Guard {
            arm_index,
            bindings,
            success,
            failure,
        } => {
            let bindings = lower_bindings(ctx, bindings);
            let success = Box::new(lower_decision(ctx, success));
            let failure = Box::new(lower_decision(ctx, failure));
            EmitDecision::Guard {
                arm_index: *arm_index,
                bindings,
                success,
                failure,
            }
        }

        Decision::Chain { tests, fallback } => {
            let lowered_tests = tests
                .iter()
                .map(|test| lower_chain_test(ctx, test))
                .collect::<Vec<_>>();
            let fallback = Box::new(lower_decision(ctx, fallback));
            let last_is_catchall =
                matches!(fallback.as_ref(), EmitDecision::Unreachable) && tests.len() > 1;
            EmitDecision::Chain {
                tests: lowered_tests,
                fallback,
                last_is_catchall,
            }
        }

        Decision::Switch {
            path,
            kind,
            shape,
            branches,
            fallback,
        } => lower_switch(ctx, path, kind, shape, branches, fallback),
    }
}

fn lower_chain_test(ctx: &mut LoweringCtx, test: &ChainTest) -> EmitChainTest {
    let (condition, subject) = if test.checks.is_empty() {
        (None, None)
    } else {
        (
            Some(render_condition(&test.checks, &ctx.current_subject)),
            Some(ctx.current_subject.clone()),
        )
    };
    let decision = Box::new(lower_decision(ctx, &test.decision));
    EmitChainTest {
        condition,
        subject,
        decision,
    }
}

fn lower_bindings(ctx: &mut LoweringCtx, bindings: &[PatternBinding]) -> Vec<EmitBinding> {
    bindings
        .iter()
        .map(|b| EmitBinding {
            lisette_name: b.lisette_name.clone(),
            go_name: b.go_name.clone(),
            rendered_access: b.path.render(&ctx.current_subject),
            composable_access: b.path.render_composable(&ctx.current_subject),
            subject: ctx.current_subject.clone(),
        })
        .collect()
}

fn lower_switch(
    ctx: &mut LoweringCtx,
    path: &AccessPath,
    kind: &SwitchKind,
    shape: &SwitchShape,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> EmitDecision {
    propagate_stdlib(ctx, branches);
    let rendered_path = path.render(&ctx.current_subject);

    match shape {
        SwitchShape::TypeSwitch => lower_type_switch(ctx, rendered_path, branches, fallback),
        SwitchShape::Bool => lower_bool_switch(ctx, rendered_path, branches),
        SwitchShape::Binary => lower_binary_switch(ctx, &rendered_path, kind, branches),
        SwitchShape::SingleArm => {
            lower_single_arm_switch(ctx, &rendered_path, kind, branches, fallback)
        }
        SwitchShape::Multi => lower_multi_switch(ctx, &rendered_path, kind, branches, fallback),
    }
}

fn lower_type_switch(
    ctx: &mut LoweringCtx,
    base: String,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> EmitDecision {
    let subject = ctx.current_subject.clone();
    ctx.with_subject(base.clone(), |ctx| {
        let (cases, default) =
            lower_cases_with_default_lift(ctx, branches, fallback, /*lift=*/ true);
        EmitDecision::TypeSwitch {
            base,
            subject,
            cases,
            default,
        }
    })
}

fn lower_bool_switch(
    ctx: &mut LoweringCtx,
    rendered_path: String,
    branches: &[SwitchBranch],
) -> EmitDecision {
    // Select branches by label so `then_branch` is always the semantic-true
    // subtree regardless of source order.
    let true_branch = branches
        .iter()
        .find(|b| b.case_label == "true")
        .expect("Bool shape requires a true-labeled branch");
    let false_branch = branches
        .iter()
        .find(|b| b.case_label == "false")
        .expect("Bool shape requires a false-labeled branch");
    EmitDecision::IfElse {
        condition: wrap_if_struct_literal(rendered_path),
        subject: Some(ctx.current_subject.clone()),
        then_branch: Box::new(lower_decision(ctx, &true_branch.decision)),
        else_branch: Some(Box::new(lower_decision(ctx, &false_branch.decision))),
    }
}

fn lower_binary_switch(
    ctx: &mut LoweringCtx,
    rendered_path: &str,
    kind: &SwitchKind,
    branches: &[SwitchBranch],
) -> EmitDecision {
    let first = &branches[0];
    let second = &branches[1];
    let expr = render_switch_expression(rendered_path, kind);
    EmitDecision::IfElse {
        condition: format!("{} == {}", expr, first.case_label),
        subject: Some(ctx.current_subject.clone()),
        then_branch: Box::new(lower_decision(ctx, &first.decision)),
        else_branch: Some(Box::new(lower_decision(ctx, &second.decision))),
    }
}

fn lower_single_arm_switch(
    ctx: &mut LoweringCtx,
    rendered_path: &str,
    kind: &SwitchKind,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> EmitDecision {
    let branch = &branches[0];
    let Some(fb) = fallback else {
        return EmitDecision::InlineBranch {
            branch: Box::new(lower_decision(ctx, &branch.decision)),
        };
    };
    let expr = render_switch_expression(rendered_path, kind);
    let lowered_fallback = lower_decision(ctx, fb);
    let else_branch = if is_empty_emit_decision(&lowered_fallback) {
        None
    } else {
        Some(Box::new(lowered_fallback))
    };
    EmitDecision::IfElse {
        condition: format!("{} == {}", expr, branch.case_label),
        subject: Some(ctx.current_subject.clone()),
        then_branch: Box::new(lower_decision(ctx, &branch.decision)),
        else_branch,
    }
}

fn lower_multi_switch(
    ctx: &mut LoweringCtx,
    rendered_path: &str,
    kind: &SwitchKind,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> EmitDecision {
    let expr = render_switch_expression(rendered_path, kind);
    let (cases, default) =
        lower_cases_with_default_lift(ctx, branches, fallback, /*lift=*/ true);
    EmitDecision::Switch {
        expr,
        subject: ctx.current_subject.clone(),
        cases,
        default,
    }
}

fn render_switch_expression(rendered_path: &str, kind: &SwitchKind) -> String {
    match kind {
        SwitchKind::EnumTag => wrap_if_struct_literal(format!("{}.Tag", rendered_path)),
        SwitchKind::Value => wrap_if_struct_literal(rendered_path.to_string()),
        SwitchKind::TypeSwitch => unreachable!("TypeSwitch handled separately"),
    }
}

fn propagate_stdlib(ctx: &mut LoweringCtx, branches: &[SwitchBranch]) {
    if branches.iter().any(|b| b.needs_stdlib) {
        ctx.effects.needs_stdlib = true;
    }
}

fn lower_cases_with_default_lift(
    ctx: &mut LoweringCtx,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
    lift: bool,
) -> (Vec<EmitCase>, Option<Box<EmitDecision>>) {
    let use_last_as_default = lift && fallback.is_none() && !branches.is_empty();
    let (regular, default_branch) = if use_last_as_default {
        let (last, rest) = branches.split_last().expect("non-empty");
        (rest, Some(last))
    } else {
        (branches, None)
    };

    let cases = regular
        .iter()
        .map(|b| EmitCase {
            case_label: b.case_label.clone(),
            decision: Box::new(lower_decision(ctx, &b.decision)),
        })
        .collect();

    let default = match (default_branch, fallback) {
        (Some(last), _) => Some(Box::new(lower_decision(ctx, &last.decision))),
        (None, Some(fb)) => Some(Box::new(lower_decision(ctx, fb))),
        (None, None) => None,
    };

    (cases, default)
}

pub(crate) fn is_empty_emit_decision(d: &EmitDecision) -> bool {
    matches!(
        d,
        EmitDecision::Success {
            leaf_emits_no_output: true,
            ..
        }
    )
}

fn body_is_unit_or_empty(expression: &syntax::ast::Expression) -> bool {
    use syntax::ast::Expression;
    matches!(expression, Expression::Unit { .. })
        || matches!(expression, Expression::Block { items, .. } if items.is_empty())
}
