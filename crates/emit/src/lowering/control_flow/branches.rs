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
use crate::target::ir::go_expression::GoBinaryOperator;
use crate::target::ir::values::GoExpression;
use syntax::ast::{
    BinaryOperator, Expression, ExpressionId, IfLetAlternative, MatchArm, Pattern, Span,
};
use syntax::types::Type;

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

fn collect_conjuncts<'e>(condition: &'e Expression, conjuncts: &mut Vec<&'e Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::And,
        left,
        right,
        ..
    } = condition
    {
        collect_conjuncts(left, conjuncts);
        collect_conjuncts(right, conjuncts);
    } else {
        conjuncts.push(condition);
    }
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
        ty: &Type,
    ) -> ValuePlan {
        let ty = self.facts.value_use_slot(expression).unwrap_or(ty);
        let (result_var, declaration) = self.operand_temp_declaration(ty);
        let target = GoExpression::identifier(result_var.clone());
        let block = self.lower_branching_to_block(
            expression,
            &PlacePlan::Assign {
                local: &target,
                target_ty: Some(ty),
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
        let mut conjuncts = Vec::new();
        collect_conjuncts(condition, &mut conjuncts);
        let mut levels: Vec<(Vec<Statement>, PairCondition)> = Vec::new();
        for conjunct in conjuncts {
            let (setup, next) = self.lower_if_condition(conjunct);
            if setup.is_empty()
                && next.initializer.is_none()
                && let Some((outer_setup, current)) = levels.pop()
            {
                let condition =
                    GoExpression::binary(current.condition, GoBinaryOperator::And, next.condition);
                let merged = PairCondition {
                    initializer: current.initializer,
                    condition,
                };
                levels.push((outer_setup, merged));
            } else {
                levels.push((setup, next));
                self.scope.enter_block();
            }
        }
        let mut then_body = body(self);
        let mut levels = levels.into_iter().rev().peekable();
        loop {
            let (setup, condition) = levels.next().expect("a conjunction has a first operand");
            self.scope.exit_block();
            let guard = Guard {
                setup,
                condition,
                then_body,
            };
            if levels.peek().is_none() {
                return guard;
            }
            then_body = LoweredBlock {
                statements: vec![guard.into_statement().into()],
            };
        }
    }

    pub(in crate::lowering) fn lower_branching_to_block(
        &mut self,
        expression: &Expression,
        place: &PlacePlan,
    ) -> LoweredBlock {
        match expression {
            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                let statement =
                    self.lower_if(condition, consequence, alternative.as_deref(), place);
                LoweredBlock {
                    statements: vec![statement.into()],
                }
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                span,
                ..
            } => {
                let implicit_unit = self.facts.checked(expression).implicit_unit();
                let arms =
                    if_let_match_arms(pattern, consequence, alternative, *span, implicit_unit);
                let place = match alternative {
                    IfLetAlternative::Absent => &PlacePlan::Statement,
                    IfLetAlternative::Present { .. } => place,
                };
                self.lower_match_to_block(scrutinee, &arms, place)
            }
            Expression::Match { subject, arms, .. } => {
                self.lower_match_to_block(subject, arms, place)
            }
            Expression::Select { arms, span, .. } => LoweredBlock {
                statements: vec![self.lower_select(arms, span, place).into()],
            },
            _ => unreachable!("lower_branching_to_block: expected if/if-let/match/select"),
        }
    }

    pub(in crate::lowering) fn lower_if(
        &mut self,
        condition: &Expression,
        consequence: &Expression,
        alternative: Option<&Expression>,
        place: &PlacePlan,
    ) -> LoweredStatement {
        if alternative.is_none() && is_conjunction(condition) {
            return self
                .lower_conjunction_guard(condition, |this| {
                    this.with_scope(|this| this.lower_block_to_place(consequence, place))
                })
                .into_statement();
        }

        let (condition_setup, condition) = self.lower_if_condition(condition);

        let then_place = if alternative.is_none() {
            &PlacePlan::Statement
        } else {
            place
        };
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

        if let Expression::If {
            condition,
            consequence,
            alternative: next_alternative,
            ..
        } = alternative
        {
            let line = self.facts.source_position(&alternative.get_span());
            if next_alternative.is_none() && is_conjunction(condition) {
                return self.with_scope(|this| {
                    let guard = this.lower_conjunction_guard(condition, |this| {
                        this.with_scope(|this| this.lower_block_to_place(consequence, place))
                    });
                    if guard.setup.is_empty() {
                        let mut plan = guard.into_if();
                        plan.else_if_line = line;
                        ElseArm::ElseIf(Box::new(plan))
                    } else {
                        ElseArm::Else {
                            body: LoweredBlock {
                                statements: directed_first(
                                    line,
                                    vec![guard.into_statement().into()],
                                ),
                            },
                            layout: ElseLayout::Block,
                        }
                    }
                });
            }
            let (condition_setup, condition) = self.lower_if_condition(condition);

            if !condition_setup.is_empty() {
                let body = self.with_scope(|this| {
                    let then_body =
                        this.with_scope(|this| this.lower_block_to_place(consequence, place));
                    let inner = this.lower_else_chain(
                        next_alternative.as_deref(),
                        Divergence::of(&then_body),
                        place,
                    );
                    let mut statements = condition_setup;
                    statements.push(
                        LoweredStatement::If(IfPlan::pair_if(
                            condition,
                            then_body,
                            inner,
                            IfOrigin::User,
                        ))
                        .into(),
                    );
                    LoweredBlock {
                        statements: directed_first(line, statements),
                    }
                });

                ElseArm::Else {
                    body,
                    layout: ElseLayout::Block,
                }
            } else {
                let then_body =
                    self.with_scope(|this| this.lower_block_to_place(consequence, place));
                let chained = match preceding {
                    Divergence::Diverges => Divergence::of(&then_body),
                    Divergence::FallsThrough => Divergence::FallsThrough,
                };
                let inner = self.lower_else_chain(next_alternative.as_deref(), chained, place);
                let mut plan = IfPlan::pair_if(condition, then_body, inner, IfOrigin::User);
                plan.else_if_line = line;
                ElseArm::ElseIf(Box::new(plan))
            }
        } else {
            match preceding {
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
            }
        }
    }
}
