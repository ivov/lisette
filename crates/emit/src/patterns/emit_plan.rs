use syntax::ast::MatchArm;

use crate::Emitter;
use crate::control_flow::branching::wrap_if_struct_literal;
use crate::patterns::decision_tree::{
    AccessPath, ChainTest, Decision, PatternBinding, SwitchKind, SwitchShape,
    decision_is_exhaustive, render_condition, tree_has_unguarded_terminal,
};

#[derive(Debug, Clone)]
pub(crate) struct EmitBinding {
    pub lisette_name: String,
    pub go_name: Option<String>,
    pub rendered_access: String,
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
    /// Renderer emits trailing `panic("unreachable")` iff
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
        cond: String,
        then_branch: Box<EmitDecision>,
        else_branch: Option<Box<EmitDecision>>,
    },
    /// Exhaustive single-variant enum: no condition wrapper.
    InlineBranch {
        branch: Box<EmitDecision>,
    },
    Switch {
        expr: String,
        cases: Vec<EmitCase>,
        default: Option<Box<EmitDecision>>,
    },
    TypeSwitch {
        base: String,
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
    pub cond: Option<String>,
    pub decision: Box<EmitDecision>,
}

#[derive(Debug)]
pub(crate) struct EmitCase {
    pub case_label: String,
    pub decision: Box<EmitDecision>,
}

pub(crate) struct LoweringCtx<'a, 'e> {
    pub emitter: &'a mut Emitter<'e>,
    pub arms: &'a [MatchArm],
    pub current_subject: String,
}

impl<'a, 'e> LoweringCtx<'a, 'e> {
    pub(crate) fn new(
        emitter: &'a mut Emitter<'e>,
        arms: &'a [MatchArm],
        subject_var: String,
    ) -> Self {
        Self {
            emitter,
            arms,
            current_subject: subject_var,
        }
    }

    /// Scope a subject swap to the closure so a nested type switch's
    /// `base` cannot leak into a later outer branch.
    pub(crate) fn with_subject<R>(
        &mut self,
        new_subject: String,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let prev = std::mem::replace(&mut self.current_subject, new_subject);
        let result = f(self);
        self.current_subject = prev;
        result
    }
}

/// Precedence: Switch, SingleCatchall, RetryLoop (if any guard), else Chain.
pub(crate) fn lower_match(ctx: &mut LoweringCtx, decision: &Decision) -> MatchEmitPlan {
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
        let arm = &ctx.arms[*arm_index];
        let pattern_has_collisions = ctx.emitter.pattern_has_binding_collisions(&arm.pattern);
        let bindings = lower_bindings(ctx, bindings);
        return MatchEmitPlan::SingleCatchall(SingleCatchallPlan {
            arm_index: *arm_index,
            bindings,
            pattern_has_collisions,
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
            .is_some_and(|arm| !arm.has_guard() && Emitter::is_catchall_pattern(&arm.pattern));
        let tree = lower_decision(ctx, decision);
        return MatchEmitPlan::RetryLoop(RetryLoopPlan {
            tree,
            all_arms_diverge,
            root_has_unguarded_terminal,
            last_arm_is_any_catchall,
        });
    }

    let chain_tail_is_exhaustive = decision_is_exhaustive(decision)
        || ctx.arms.last().is_some_and(|arm| {
            !arm.has_guard() && Emitter::is_unconditional_catchall(&arm.pattern)
        });
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
            // Last test is reachable as `else` when fallback is unreachable.
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
    let cond = if test.checks.is_empty() {
        None
    } else {
        Some(render_condition(&test.checks, &ctx.current_subject))
    };
    let decision = Box::new(lower_decision(ctx, &test.decision));
    EmitChainTest { cond, decision }
}

fn lower_bindings(ctx: &mut LoweringCtx, bindings: &[PatternBinding]) -> Vec<EmitBinding> {
    bindings
        .iter()
        .map(|b| EmitBinding {
            lisette_name: b.lisette_name.clone(),
            go_name: b.go_name.clone(),
            rendered_access: b.path.render(&ctx.current_subject),
        })
        .collect()
}

fn lower_switch(
    ctx: &mut LoweringCtx,
    path: &AccessPath,
    kind: &SwitchKind,
    shape: &SwitchShape,
    branches: &[crate::patterns::decision_tree::SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> EmitDecision {
    propagate_stdlib(ctx, branches);

    let rendered_path = path.render(&ctx.current_subject);

    match shape {
        SwitchShape::TypeSwitch => {
            let base = rendered_path;
            ctx.with_subject(base.clone(), |ctx| {
                let (cases, default) =
                    lower_cases_with_default_lift(ctx, branches, fallback, /*lift=*/ true);
                EmitDecision::TypeSwitch {
                    base,
                    cases,
                    default,
                }
            })
        }
        SwitchShape::Bool => {
            // Select branches by label so `then_branch` is always the
            // semantic-true subtree regardless of source order.
            let true_branch = branches
                .iter()
                .find(|b| b.case_label == "true")
                .expect("Bool shape requires a true-labeled branch");
            let false_branch = branches
                .iter()
                .find(|b| b.case_label == "false")
                .expect("Bool shape requires a false-labeled branch");
            let cond = wrap_if_struct_literal(rendered_path);
            EmitDecision::IfElse {
                cond,
                then_branch: Box::new(lower_decision(ctx, &true_branch.decision)),
                else_branch: Some(Box::new(lower_decision(ctx, &false_branch.decision))),
            }
        }
        SwitchShape::Binary => {
            let first = &branches[0];
            let second = &branches[1];
            let expr = render_switch_expression(&rendered_path, kind);
            let cond = format!("{} == {}", expr, first.case_label);
            EmitDecision::IfElse {
                cond,
                then_branch: Box::new(lower_decision(ctx, &first.decision)),
                else_branch: Some(Box::new(lower_decision(ctx, &second.decision))),
            }
        }
        SwitchShape::SingleArm => {
            let branch = &branches[0];
            let expr = render_switch_expression(&rendered_path, kind);
            match fallback {
                None => {
                    // Exhaustive enum: inline body, no condition wrapper.
                    EmitDecision::InlineBranch {
                        branch: Box::new(lower_decision(ctx, &branch.decision)),
                    }
                }
                Some(fb) => {
                    let lowered_fallback = lower_decision(ctx, fb);
                    let cond = format!("{} == {}", expr, branch.case_label);
                    let else_branch = if is_empty_emit_decision(&lowered_fallback) {
                        None
                    } else {
                        Some(Box::new(lowered_fallback))
                    };
                    EmitDecision::IfElse {
                        cond,
                        then_branch: Box::new(lower_decision(ctx, &branch.decision)),
                        else_branch,
                    }
                }
            }
        }
        SwitchShape::Multi => {
            let expr = render_switch_expression(&rendered_path, kind);
            let (cases, default) =
                lower_cases_with_default_lift(ctx, branches, fallback, /*lift=*/ true);
            EmitDecision::Switch {
                expr,
                cases,
                default,
            }
        }
    }
}

fn render_switch_expression(rendered_path: &str, kind: &SwitchKind) -> String {
    match kind {
        SwitchKind::EnumTag => wrap_if_struct_literal(format!("{}.Tag", rendered_path)),
        SwitchKind::Value => wrap_if_struct_literal(rendered_path.to_string()),
        SwitchKind::TypeSwitch => unreachable!("TypeSwitch handled separately"),
    }
}

fn propagate_stdlib(
    ctx: &mut LoweringCtx,
    branches: &[crate::patterns::decision_tree::SwitchBranch],
) {
    for b in branches {
        if b.needs_stdlib {
            ctx.emitter.flags.needs_stdlib = true;
        }
    }
}

fn lower_cases_with_default_lift(
    ctx: &mut LoweringCtx,
    branches: &[crate::patterns::decision_tree::SwitchBranch],
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

pub(crate) fn emit_lowered_bindings(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[EmitBinding],
) {
    use crate::write_line;
    for binding in bindings {
        let Some(ref go_name) = binding.go_name else {
            emitter.scope.bindings.add(&binding.lisette_name, "");
            continue;
        };

        let access_expression = &binding.rendered_access;

        if emitter.scope.bindings.has_go_name(go_name) {
            let fresh = emitter.fresh_var(Some(&binding.lisette_name));
            emitter.scope.bindings.add(&binding.lisette_name, &fresh);
            emitter.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, access_expression);
        } else {
            let name = emitter
                .scope
                .bindings
                .add(&binding.lisette_name, go_name.clone());
            if emitter.try_declare(&name) {
                write_line!(output, "{} := {}", name, access_expression);
            } else {
                let fresh = emitter.fresh_var(Some(&binding.lisette_name));
                emitter.scope.bindings.add(&binding.lisette_name, &fresh);
                emitter.try_declare(&fresh);
                write_line!(output, "{} := {}", fresh, access_expression);
            }
        }
    }
}
