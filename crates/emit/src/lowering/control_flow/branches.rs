use crate::Planner;
use crate::lowering::statements::placement::collapse_declared_temp;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::PairCondition;
use crate::target::ir::bodies::{
    ElseArm, ElseLayout, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, Statement,
    directed_first, with_setup,
};
use crate::target::ir::go_expression::{GoBinaryOperator, Position};
use crate::target::ir::values::GoExpression;
use syntax::ast::{
    BinaryOperator, Expression, ExpressionId, IfLetAlternative, MatchArm, Pattern, SelectArm, Span,
};
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(in crate::lowering) enum Branching<'e> {
    If {
        condition: &'e Expression,
        consequence: &'e Expression,
        alternative: Option<&'e Expression>,
    },
    IfLet {
        expression: &'e Expression,
        pattern: &'e Pattern,
        scrutinee: &'e Expression,
        consequence: &'e Expression,
        alternative: &'e IfLetAlternative,
        span: Span,
    },
    Match {
        subject: &'e Expression,
        arms: &'e [MatchArm],
    },
    Select {
        arms: &'e [SelectArm],
        span: &'e Span,
    },
}

impl<'e> Branching<'e> {
    #[allow(clippy::wildcard_enum_match_arm)]
    pub(in crate::lowering) fn of(expression: &'e Expression) -> Option<Self> {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => Some(Self::If {
                condition,
                consequence,
                alternative: alternative.as_deref(),
            }),
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => Some(Self::IfLet {
                expression,
                pattern,
                scrutinee,
                consequence,
                alternative,
                span: *span,
            }),
            Expression::Match { subject, arms, .. } => Some(Self::Match { subject, arms }),
            Expression::Select { arms, span, .. } => Some(Self::Select { arms, span }),
            _ => None,
        }
    }
}

pub(in crate::lowering) fn if_let_match_arms(
    pattern: &Pattern,
    consequence: &Expression,
    alternative: &IfLetAlternative,
    span: Span,
    implicit_unit: Option<ExpressionId>,
) -> Vec<MatchArm> {
    let alternative = alternative
        .expression()
        .cloned()
        .unwrap_or_else(|| Expression::Unit {
            id: implicit_unit.expect("internal error: an `if let` with no `else` has its unit"),
            ty: Type::unit(),
            span,
        });

    vec![
        MatchArm {
            pattern: pattern.clone(),
            guard: None,
            expression: Box::new(consequence.clone()),
        },
        MatchArm {
            pattern: Pattern::WildCard {
                span: alternative.get_span(),
            },
            guard: None,
            expression: Box::new(alternative),
        },
    ]
}

pub(in crate::lowering) struct Guard {
    setup: Vec<Statement>,
    condition: PairCondition,
    then_body: LoweredBlock,
}

impl Guard {
    fn into_if(self) -> IfPlan {
        IfPlan::pair_if(
            self.condition,
            self.then_body,
            ElseArm::None,
            IfOrigin::User,
        )
    }

    pub(in crate::lowering) fn into_statement(self) -> LoweredStatement {
        let Guard {
            setup,
            condition,
            then_body,
        } = self;
        let guard = Guard {
            setup: Vec::new(),
            condition,
            then_body,
        };
        with_setup(setup, LoweredStatement::If(guard.into_if()))
    }
}

fn split_conjuncts(condition: &Expression) -> (&Expression, Vec<&Expression>) {
    let Expression::Binary {
        operator: BinaryOperator::And,
        left,
        right,
        ..
    } = condition
    else {
        return (condition, Vec::new());
    };
    let (first, mut rest) = split_conjuncts(left);
    let (right_first, right_rest) = split_conjuncts(right);
    rest.push(right_first);
    rest.extend(right_rest);
    (first, rest)
}

fn merge_conjunct(level: ConjunctLevel, next: PairCondition) -> ConjunctLevel {
    let (setup, current) = level;
    let condition = GoExpression::binary(current.condition, GoBinaryOperator::And, next.condition);
    (
        setup,
        PairCondition {
            initializer: current.initializer,
            condition,
        },
    )
}

type ConjunctLevel = (Vec<Statement>, PairCondition);

struct ElseIfBranch<'e> {
    condition: &'e Expression,
    consequence: &'e Expression,
    alternative: Option<&'e Expression>,
    line: Option<Position>,
}

pub(in crate::lowering) fn is_conjunction(condition: &Expression) -> bool {
    matches!(
        condition,
        Expression::Binary {
            operator: BinaryOperator::And,
            ..
        }
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum Divergence {
    Diverges,
    FallsThrough,
}

impl Divergence {
    pub(in crate::lowering) fn of(block: &LoweredBlock) -> Self {
        if block.ends_with_diverge() {
            Self::Diverges
        } else {
            Self::FallsThrough
        }
    }
}

impl Planner<'_> {
    pub(in crate::lowering) fn plan_branching_as_operand_temp(
        &mut self,
        expression: &Expression,
        branching: Branching<'_>,
    ) -> ValuePlan {
        let ty = self
            .facts
            .value_use_slot(expression)
            .expect("a branching value has a slot type");
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let target = GoExpression::identifier(result_var.clone());
        let block = self.lower_branching_to_block(
            branching,
            &PlacePlan::Assign {
                local: &target,
                target_ty: ty,
            },
        );

        let mut setup = vec![declaration];
        setup.extend(block.statements);
        collapse_declared_temp(
            &mut setup,
            &result_var,
            self.facts.short_declaration_keeps_type(ty),
        );
        ValuePlan::captured(setup, result_var)
    }

    pub(in crate::lowering) fn plan_condition(
        &mut self,
        condition: &Expression,
    ) -> (Vec<Statement>, GoExpression) {
        let plan = self.plan_operand(condition, ExpressionContext::value());
        plan.into_parts()
    }

    fn lower_if_condition(&mut self, condition: &Expression) -> (Vec<Statement>, PairCondition) {
        if let Some(fused) = self.try_plan_fused_predicate_condition(condition) {
            return fused;
        }
        let (setup, rendered) = self.plan_condition(condition);
        (
            setup,
            PairCondition {
                initializer: None,
                condition: rendered,
            },
        )
    }

    pub(in crate::lowering) fn lower_conjunction_guard(
        &mut self,
        condition: &Expression,
        body: impl FnOnce(&mut Self) -> LoweredBlock,
    ) -> Guard {
        let (first, rest) = split_conjuncts(condition);
        let mut outermost = self.lower_if_condition(first);
        self.scope.enter_block();
        let mut levels: Vec<ConjunctLevel> = Vec::new();
        for conjunct in rest {
            let (setup, next) = self.lower_if_condition(conjunct);
            if !setup.is_empty() || next.initializer.is_some() {
                levels.push((setup, next));
                self.scope.enter_block();
            } else if let Some(level) = levels.pop() {
                levels.push(merge_conjunct(level, next));
            } else {
                outermost = merge_conjunct(outermost, next);
            }
        }
        let mut then_body = body(self);
        for (setup, condition) in levels.into_iter().rev() {
            self.scope.exit_block();
            let guard = Guard {
                setup,
                condition,
                then_body,
            };
            then_body = LoweredBlock {
                statements: vec![guard.into_statement().into()],
            };
        }
        self.scope.exit_block();
        let (setup, condition) = outermost;
        Guard {
            setup,
            condition,
            then_body,
        }
    }

    pub(in crate::lowering) fn lower_branching_to_block(
        &mut self,
        branching: Branching<'_>,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match branching {
            Branching::If {
                condition,
                consequence,
                alternative,
            } => {
                let statement = self.lower_if(condition, consequence, alternative, place);
                LoweredBlock {
                    statements: vec![statement.into()],
                }
            }
            Branching::IfLet {
                expression,
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
            } => {
                let implicit_unit = self.facts.checked(expression).implicit_unit();
                let arms =
                    if_let_match_arms(pattern, consequence, alternative, span, implicit_unit);
                let place = match alternative {
                    IfLetAlternative::Absent => &PlacePlan::Statement,
                    IfLetAlternative::Present { .. } => place,
                };
                self.lower_match_to_block(scrutinee, &arms, place)
            }
            Branching::Match { subject, arms } => self.lower_match_to_block(subject, arms, place),
            Branching::Select { arms, span } => LoweredBlock {
                statements: vec![self.lower_select(arms, span, place).into()],
            },
        }
    }

    pub(in crate::lowering) fn lower_if(
        &mut self,
        condition: &Expression,
        consequence: &Expression,
        alternative: Option<&Expression>,
        place: &PlacePlan,
    ) -> LoweredStatement {
        let then_place = if alternative.is_none() {
            &PlacePlan::Statement
        } else {
            place
        };
        if alternative.is_none() && is_conjunction(condition) {
            return self
                .lower_conjunction_guard(condition, |this| {
                    this.with_scope(|this| this.lower_block_to_place(consequence, then_place))
                })
                .into_statement();
        }

        let (condition_setup, condition) = self.lower_if_condition(condition);
        let then_body = self.with_scope(|this| this.lower_block_to_place(consequence, then_place));

        let preceding = Divergence::of(&then_body);
        let else_arm = self.lower_else_chain(alternative, preceding, place);

        with_setup(
            condition_setup,
            LoweredStatement::If(IfPlan::pair_if(
                condition,
                then_body,
                else_arm,
                IfOrigin::User,
            )),
        )
    }

    fn lower_else_chain(
        &mut self,
        alternative: Option<&Expression>,
        preceding: Divergence,
        place: &PlacePlan,
    ) -> ElseArm {
        let Some(alternative) = alternative else {
            return ElseArm::None;
        };
        let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        else {
            return match preceding {
                Divergence::Diverges => {
                    let body = self
                        .with_binding_frame(|this| this.lower_block_to_place(alternative, place));
                    ElseArm::inline(body)
                }
                Divergence::FallsThrough => {
                    let body =
                        self.with_scope(|this| this.lower_block_to_place(alternative, place));
                    ElseArm::block(body)
                }
            };
        };

        let branch = ElseIfBranch {
            condition,
            consequence,
            alternative: next_alternative.as_deref(),
            line: self.facts.source_position(&alternative.get_span()),
        };
        if branch.alternative.is_none() && is_conjunction(condition) {
            return self.lower_else_if_conjunction(branch, place);
        }
        let (condition_setup, condition) = self.lower_if_condition(condition);
        if condition_setup.is_empty() {
            self.lower_chained_else_if(branch, condition, preceding, place)
        } else {
            self.lower_else_if_with_setup(branch, condition_setup, condition, place)
        }
    }

    fn lower_else_if_conjunction(
        &mut self,
        branch: ElseIfBranch<'_>,
        place: &PlacePlan,
    ) -> ElseArm {
        self.with_scope(|this| {
            let guard = this.lower_conjunction_guard(branch.condition, |this| {
                this.with_scope(|this| this.lower_block_to_place(branch.consequence, place))
            });
            if guard.setup.is_empty() {
                let mut plan = guard.into_if();
                plan.else_if_line = branch.line;
                ElseArm::ElseIf(Box::new(plan))
            } else {
                ElseArm::Else {
                    body: LoweredBlock {
                        statements: directed_first(
                            branch.line,
                            vec![guard.into_statement().into()],
                        ),
                    },
                    layout: ElseLayout::Block,
                }
            }
        })
    }

    fn lower_else_if_with_setup(
        &mut self,
        branch: ElseIfBranch<'_>,
        condition_setup: Vec<Statement>,
        condition: PairCondition,
        place: &PlacePlan,
    ) -> ElseArm {
        let body = self.with_scope(|this| {
            let then_body =
                this.with_scope(|this| this.lower_block_to_place(branch.consequence, place));
            let inner =
                this.lower_else_chain(branch.alternative, Divergence::of(&then_body), place);
            let mut statements = condition_setup;
            statements.push(
                LoweredStatement::If(IfPlan::pair_if(condition, then_body, inner, IfOrigin::User))
                    .into(),
            );
            LoweredBlock {
                statements: directed_first(branch.line, statements),
            }
        });

        ElseArm::Else {
            body,
            layout: ElseLayout::Block,
        }
    }

    fn lower_chained_else_if(
        &mut self,
        branch: ElseIfBranch<'_>,
        condition: PairCondition,
        preceding: Divergence,
        place: &PlacePlan,
    ) -> ElseArm {
        let then_body =
            self.with_scope(|this| this.lower_block_to_place(branch.consequence, place));
        let chained = match preceding {
            Divergence::Diverges => Divergence::of(&then_body),
            Divergence::FallsThrough => Divergence::FallsThrough,
        };
        let inner = self.lower_else_chain(branch.alternative, chained, place);
        let mut plan = IfPlan::pair_if(condition, then_body, inner, IfOrigin::User);
        plan.else_if_line = branch.line;
        ElseArm::ElseIf(Box::new(plan))
    }
}
