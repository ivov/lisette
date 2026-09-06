use crate::Planner;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::patterns::matching::{OptionFusePlan, ResultFusePlan};
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{EvaluationEffect, GoExpression, ValuePlan};
use syntax::ast::{Expression, UnaryOperator};
use syntax::program::CallKind;

enum FusedPredicate<'a> {
    Result {
        fuse: ResultFusePlan<'a>,
        succeeds: bool,
    },
    Option {
        fuse: OptionFusePlan<'a>,
        succeeds: bool,
    },
}

pub(crate) fn strip_negations(expression: &Expression) -> (&Expression, bool) {
    let mut expression = expression.unwrap_parens();
    let mut negated = false;
    while let Expression::Unary {
        operator: UnaryOperator::Not,
        expression: inner,
        ..
    } = expression
    {
        negated = !negated;
        expression = inner.unwrap_parens();
    }
    (expression, negated)
}

impl Planner<'_> {
    /// `call().is_ok()` and its siblings on a call whose Go result can be tested directly.
    fn fused_predicate<'a>(&self, expression: &'a Expression) -> Option<FusedPredicate<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            spread,
            call_kind,
            ..
        } = expression
        else {
            return None;
        };
        if !args.is_empty() || spread.is_some() || *call_kind != CallKind::Regular {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = callee.unwrap_parens()
        else {
            return None;
        };
        let receiver_ty = self.facts.peel_alias(&receiver.get_type());
        match member.as_str() {
            "is_ok" | "is_err" if receiver_ty.is_result() => Some(FusedPredicate::Result {
                fuse: self.result_fuse_plan(receiver)?,
                succeeds: member == "is_ok",
            }),
            "is_some" | "is_none" if receiver_ty.is_option() => Some(FusedPredicate::Option {
                fuse: self.option_fuse_plan(receiver)?,
                succeeds: member == "is_some",
            }),
            _ => None,
        }
    }

    fn bind_fused_predicate(
        &mut self,
        predicate: FusedPredicate<'_>,
        negated: bool,
        slot: CommaOkValueSlot,
    ) -> (Vec<LoweredStatement>, String) {
        match predicate {
            FusedPredicate::Result { fuse, succeeds } => {
                let pair = fuse.bind(self, slot, None);
                let condition = if succeeds != negated {
                    self.pair_success_condition(&pair)
                } else {
                    self.pair_failure_condition(&pair)
                };
                (pair.statements, condition)
            }
            FusedPredicate::Option { fuse, succeeds } => {
                let bound = fuse.bind(self, slot);
                let condition = if succeeds != negated {
                    bound.some_condition(self)
                } else {
                    bound.none_condition(self)
                };
                (bound.statements, condition)
            }
        }
    }

    pub(crate) fn lower_fused_predicate_condition(
        &mut self,
        condition: &Expression,
    ) -> Option<(Vec<LoweredStatement>, String)> {
        let (target, negated) = strip_negations(condition);
        let predicate = self.fused_predicate(target)?;
        Some(self.bind_fused_predicate(predicate, negated, CommaOkValueSlot::Unused))
    }

    pub(crate) fn lower_fused_predicate_value(
        &mut self,
        expression: &Expression,
        negated: bool,
    ) -> Option<ValuePlan> {
        let predicate = self.fused_predicate(expression)?;
        let (setup, condition) =
            self.bind_fused_predicate(predicate, negated, CommaOkValueSlot::Discarded);
        Some(ValuePlan::plain_call(
            setup,
            GoExpression::opaque_with_deferred_evaluation(condition, true),
            EvaluationEffect::EffectfulCall,
        ))
    }
}
