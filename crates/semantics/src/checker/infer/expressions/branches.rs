use crate::checker::EnvResolve;
use crate::checker::infer::context::{BranchArm, BranchSubsumption};
use syntax::ast::BindingKind;
use syntax::ast::{Expression, IfLetAlternative, MatchArm, Pattern, Span};
use syntax::types::Type;

use crate::checker::infer::InferCtx;

/// Outcome of unifying branch types: kept first, widened to a supertype, or failed.
enum BranchReconciliation {
    FirstBranch,
    Widened(Type),
    Failed,
}

#[derive(Clone, Copy)]
enum MatchArmsKind {
    Match,
    IfLet,
    IfLetWithoutElse,
}

impl MatchArmsKind {
    fn binding_kind(&self) -> BindingKind {
        match self {
            MatchArmsKind::Match => BindingKind::MatchArm,
            MatchArmsKind::IfLet | MatchArmsKind::IfLetWithoutElse => BindingKind::IfLet,
        }
    }

    fn is_if_let_without_else(&self) -> bool {
        matches!(self, MatchArmsKind::IfLetWithoutElse)
    }
}

impl InferCtx<'_> {
    pub(super) fn reconcile_and_unify(
        &mut self,
        result_ty: &Type,
        branches: &[BranchArm],
        span: &Span,
    ) {
        if branches.is_empty() {
            return;
        }
        if branches
            .iter()
            .any(|branch| self.contains_pending_branch_var(&branch.ty))
        {
            self.record_branch_subsumption(result_ty, branches);
            return;
        }
        match self.reconcile_branch_types(branches, span) {
            BranchReconciliation::FirstBranch => {
                self.unify(result_ty, &branches[0].ty, span);
            }
            BranchReconciliation::Widened(ty) => {
                self.unify(result_ty, &ty, span);
            }
            BranchReconciliation::Failed => {
                self.record_branch_subsumption(result_ty, branches);
            }
        }
    }

    fn record_branch_subsumption(&mut self, result_ty: &Type, branches: &[BranchArm]) {
        self.file_checks
            .branch_subsumptions
            .push(BranchSubsumption {
                result_ty: result_ty.clone(),
                arms: branches.to_vec(),
            });
    }

    fn contains_pending_branch_var(&self, ty: &Type) -> bool {
        self.file_checks.branch_subsumptions.iter().any(|o| {
            let Type::Var { id, .. } = self.env.shallow_resolve(&o.result_ty) else {
                return false;
            };
            self.env.occurs(id, ty)
        })
    }

    pub fn resolve_branch_subsumptions(&mut self) {
        let obligations = std::mem::take(&mut self.file_checks.branch_subsumptions);
        for obligation in obligations.into_iter().rev() {
            for branch in &obligation.arms {
                let arm = branch.ty.resolve_in(&self.env);
                if arm.is_never() || arm.is_error() {
                    continue;
                }
                let (unification, reported) = self.tracking_diagnostics(|this| {
                    this.try_unify(&obligation.result_ty, &branch.ty, &branch.span)
                });
                if unification.is_err() && !reported {
                    let result = obligation.result_ty.resolve_in(&self.env);
                    self.sink.push(diagnostics::infer::branch_type_mismatch(
                        &arm,
                        branch.span,
                        &result,
                    ));
                }
            }
        }
    }

    fn reconcile_branch_types(
        &mut self,
        branches: &[BranchArm],
        span: &Span,
    ) -> BranchReconciliation {
        if branches.len() < 2 {
            return BranchReconciliation::FirstBranch;
        }

        let mut common = branches[0].ty.clone();
        let mut widened_to: Option<Type> = None;

        for branch in &branches[1..] {
            let next = &branch.ty;
            if self
                .speculatively(|this| this.try_unify(&common, next, span))
                .is_ok()
            {
                continue;
            }

            if self
                .speculatively(|this| this.try_unify(next, &common, span))
                .is_ok()
            {
                common = next.clone();
                widened_to = Some(common.clone());
                continue;
            }

            return BranchReconciliation::Failed;
        }

        match widened_to {
            Some(ty) => BranchReconciliation::Widened(ty),
            None => BranchReconciliation::FirstBranch,
        }
    }

    pub(super) fn ensure_subject_matchable(&mut self, ty: &Type, span: &Span) {
        match ty {
            _ if ty.is_unknown() => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_unknown(*span));
            }
            Type::Nominal { .. } => {}
            Type::Function(_) => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_functions(*span));
            }
            Type::Var { .. } | Type::Uninferred | Type::Ignored => {
                self.sink
                    .push(diagnostics::infer::cannot_match_on_unconstrained_type(
                        *span,
                    ));
            }
            Type::Forall { body, .. } => {
                self.ensure_subject_matchable(body, span);
            }
            Type::Parameter(_) => {}
            Type::Tuple(_) => {}
            Type::Array { .. } => {}
            Type::Never | Type::Error => {}
            Type::ImportNamespace(_) => {}
            Type::ReceiverPlaceholder => {}
            Type::Simple(_) | Type::Compound { .. } => {}
        }
    }

    pub(super) fn infer_if(
        &mut self,
        condition: Box<Expression>,
        consequence: Box<Expression>,
        alternative: Option<Box<Expression>>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let consequence_ty = self.new_type_var();
        let alternative_ty = self.new_type_var();

        let is_expression = !expected_ty.is_ignored();
        let has_no_else = alternative.is_none();

        // When expected_ty is already resolved to a concrete type (e.g. an
        // interface from a return type annotation), use a shared type variable
        // (like match does) so both branches can satisfy interface constraints.
        let expected_is_concrete =
            is_expression && !has_no_else && !expected_ty.resolve_in(&self.env).is_variable();

        if expected_is_concrete {
            self.unify(&consequence_ty, expected_ty, &span);
            self.unify(&alternative_ty, expected_ty, &span);
        }

        // Branch bodies are tail-like contexts where Never calls are valid.
        let new_consequence = self.infer_root_expression(*consequence, &consequence_ty);
        let new_alternative = alternative
            .map(|alternative| self.infer_root_expression(*alternative, &alternative_ty));

        if has_no_else {
            // An `if` without `else` always has type () (unit), like Rust.
            // The consequence body can produce any type, it's discarded.
            if is_expression {
                let unit_ty = self.type_unit();
                self.unify(expected_ty, &unit_ty, &span);
            }
        } else if is_expression
            && !expected_is_concrete
            && let Some(new_alternative) = new_alternative.as_ref()
        {
            let consequence_span = new_consequence.get_span();
            let alternative_span = new_alternative.get_span();
            self.reconcile_and_unify(
                expected_ty,
                &[
                    BranchArm {
                        ty: consequence_ty.clone(),
                        span: consequence_span,
                    },
                    BranchArm {
                        ty: alternative_ty.clone(),
                        span: alternative_span,
                    },
                ],
                &span,
            );
        }

        let result_ty = if has_no_else {
            self.type_unit()
        } else if is_expression && !expected_is_concrete {
            expected_ty.resolve_in(&self.env)
        } else {
            consequence_ty
        };

        let new_condition = self.infer_condition(*condition, &span);
        if let Some(span) = Self::find_propagate(&new_condition) {
            self.sink
                .push(diagnostics::infer::propagate_in_condition(span));
        }
        Expression::If {
            condition: new_condition.into(),
            consequence: new_consequence.into(),
            alternative: new_alternative.map(Box::new),
            ty: result_ty,
            span,
        }
    }

    pub(super) fn infer_match(
        &mut self,
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let (new_subject, new_arms, result_ty) =
            self.infer_match_arms(subject, arms, MatchArmsKind::Match, span, expected_ty);

        Expression::Match {
            subject: new_subject.into(),
            arms: new_arms,
            ty: result_ty,
            span,
        }
    }

    pub(super) fn infer_if_let(
        &mut self,
        expression: Expression,
        expected_ty: &Type,
    ) -> Expression {
        let Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            span,
            ..
        } = expression
        else {
            unreachable!("infer_if_let called with non-IfLet expression");
        };
        let (alternative, else_span) = match alternative {
            IfLetAlternative::Absent => (
                Expression::Unit {
                    ty: Type::uninferred(),
                    span,
                },
                None,
            ),
            IfLetAlternative::Present {
                expression,
                else_span,
            } => (*expression, Some(else_span)),
        };
        let kind = if else_span.is_none() {
            MatchArmsKind::IfLetWithoutElse
        } else {
            MatchArmsKind::IfLet
        };
        let arms = vec![
            MatchArm {
                pattern,
                guard: None,
                expression: consequence,
            },
            MatchArm {
                pattern: Pattern::WildCard {
                    span: alternative.get_span(),
                },
                guard: None,
                expression: Box::new(alternative),
            },
        ];

        let (new_scrutinee, mut new_arms, result_ty) =
            self.infer_match_arms(scrutinee, arms, kind, span, expected_ty);

        let wildcard_arm = new_arms.pop().expect("if-let has an else arm");
        let pattern_arm = new_arms.pop().expect("if-let has a pattern arm");

        Expression::IfLet {
            pattern: pattern_arm.pattern,
            scrutinee: new_scrutinee.into(),
            consequence: pattern_arm.expression,
            alternative: match else_span {
                None => IfLetAlternative::Absent,
                Some(else_span) => IfLetAlternative::Present {
                    expression: wildcard_arm.expression,
                    else_span,
                },
            },
            ty: result_ty,
            span,
        }
    }

    fn infer_match_arms(
        &mut self,
        subject: Box<Expression>,
        arms: Vec<MatchArm>,
        kind: MatchArmsKind,
        span: Span,
        expected_ty: &Type,
    ) -> (Expression, Vec<MatchArm>, Type) {
        let arm_kind = kind.binding_kind();
        let is_if_let_without_else = kind.is_if_let_without_else();
        let result_ty = self.new_type_var();
        let subject_ty = self.new_type_var();
        let new_subject = self.infer_expression(*subject, &subject_ty);

        let resolved_subject_ty = new_subject.get_type().resolve_in(&self.env);
        self.ensure_subject_matchable(&resolved_subject_ty, &new_subject.get_span());

        let is_statement = expected_ty.is_ignored();

        // if-let without else always has type (), like if without else.
        // Arms don't need to agree since the result is always ().
        let arms_independent = is_statement || is_if_let_without_else;

        if !is_statement {
            if is_if_let_without_else {
                let unit = self.type_unit();
                self.unify(expected_ty, &unit, &span);
                let _ = self.try_unify(&result_ty, &unit, &span);
            } else {
                self.unify(expected_ty, &result_ty, &span);
            }
        }

        let needs_reconciliation =
            !arms_independent && result_ty.resolve_in(&self.env).is_variable();

        let new_arms = arms
            .into_iter()
            .map(|a| {
                self.with_scope(|this| {
                    let pattern_ty = subject_ty.resolve_in(&this.env);
                    let new_pattern = this.infer_pattern(a.pattern, pattern_ty, arm_kind);

                    let new_guard = a
                        .guard
                        .map(|guard| Box::new(this.infer_condition(*guard, &span)));

                    let independent_ty;
                    let arm_expected = if arms_independent || needs_reconciliation {
                        independent_ty = this.new_type_var();
                        &independent_ty
                    } else {
                        &result_ty
                    };
                    // Arm body is a tail-like context where Never calls are valid.
                    let new_expression = this.infer_root_expression(*a.expression, arm_expected);

                    MatchArm {
                        pattern: new_pattern,
                        guard: new_guard,
                        expression: Box::new(new_expression),
                    }
                })
            })
            .collect::<Vec<_>>();

        if needs_reconciliation {
            let branches: Vec<BranchArm> = new_arms
                .iter()
                .map(|arm| BranchArm {
                    ty: arm.expression.get_type(),
                    span: arm.expression.get_span(),
                })
                .collect();
            self.reconcile_and_unify(&result_ty, &branches, &span);
        } else if is_statement && let Some(first_arm) = new_arms.first() {
            // In statement position, set the match's type from the first arm so the
            // expression still has a well-defined type for inspection, even though
            // arms are not required to agree.
            let first_ty = first_arm.expression.get_type();
            let _ = self.try_unify(&result_ty, &first_ty, &span);
        }

        (new_subject, new_arms, result_ty)
    }
}
