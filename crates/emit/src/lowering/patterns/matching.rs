use crate::Planner;
use crate::lowering::abi::fallible_producer::OptionFusePlan;
use crate::lowering::abi::go_result_wrapping::{
    nil_condition, non_nil_condition, unexpected_nil_error,
};
use crate::lowering::calls::bound_value::BoundValue;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::target::ir::prelude::{PartialVariant, PreludeVariant, prelude_variant};
use crate::target::types::return_abi::LoweredReturnAbi;

use crate::lookup::state::scope::PairStatusKind;
use crate::lowering::calls::call_plan::{CallPlan, CallableOrigin};
use crate::lowering::calls::wrap_err::WrapMessage;
use crate::lowering::patterns::binding_decls::{
    has_binding_or_pattern, pattern_binds_name, pattern_has_bindings,
};
use crate::lowering::patterns::decision_tree;
use crate::lowering::patterns::sites::ResolvedSubject;
use crate::lowering::patterns::tree_planner::{MatchSubject, TreePlanner};
use crate::lowering::plan::context::ExpressionContext;
use crate::lowering::plan::values::PlacePlan;
use crate::lowering::plan::values::ValuePlan;
use crate::target::ir::bodies::GoUses;
use crate::target::ir::bodies::{
    Definition, ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, PairCondition,
    Statement, SwitchKind, assign, define, define_many, discard,
};
use crate::target::ir::go_expression::{BinaryOp, GoExpressionNode};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::{CaptureBoundary, GoExpression};
use crate::target::names::source_name::SourceName;
use syntax::EcoString;
use syntax::ast::{BindingId, Expression, MatchArm, Pattern};
use syntax::types::Type;

mod fused;
pub(in crate::lowering) use fused::*;

fn finish_nested_pair_arm(bound: &mut BoundValue, body: LoweredBlock) -> LoweredBlock {
    let body = match body.statements.as_slice() {
        [
            Statement {
                kind: LoweredStatement::Block(inner),
                ..
            },
        ] => inner.clone(),
        _ => body,
    };

    if let Some(name) = bound.payload_identifier()
        && !GoUses::of(&body.statements).contains_identifier(name)
    {
        bound.discard_value();
    }
    body
}

#[derive(Clone, Copy)]
pub(in crate::lowering) struct FusedName<'a> {
    pub(in crate::lowering) name: &'a EcoString,
    pub(in crate::lowering) binding: Option<BindingId>,
}

impl FusedName<'_> {
    pub(in crate::lowering) fn go_name(&self) -> SourceName {
        SourceName::new(self.name)
    }
}

#[derive(Clone)]
pub(super) enum ArmBinding<'a> {
    Alias {
        name: FusedName<'a>,
        go_name: GoIdentifier,
    },
    Copy {
        name: FusedName<'a>,
        value: &'a GoExpression,
    },
    AliasEach {
        name: &'a EcoString,
        ids: Vec<BindingId>,
        go_name: GoIdentifier,
    },
}

impl<'a> ArmBinding<'a> {
    pub(super) fn alias_at(
        name: Option<FusedName<'a>>,
        go_name: Option<&GoIdentifier>,
    ) -> Option<Self> {
        name.zip(go_name).map(|(name, go_name)| Self::Alias {
            name,
            go_name: go_name.clone(),
        })
    }

    pub(super) fn copy_at(
        name: Option<FusedName<'a>>,
        value: Option<&'a GoExpression>,
    ) -> Option<Self> {
        name.zip(value)
            .map(|(name, value)| Self::Copy { name, value })
    }
}

struct ResultArm<'a> {
    arm: &'a MatchArm,
    is_catch_all: bool,
}

struct PartialOutcomeArms<'a> {
    ok: &'a MatchArm,
    failure: &'a MatchArm,
    value_binding: Option<FusedName<'a>>,
    error_binding: Option<(&'a EcoString, Vec<BindingId>)>,
}

impl PartialOutcomeArms<'_> {
    fn error_hint(&self) -> Option<SourceName> {
        self.error_binding
            .as_ref()
            .map(|(name, _)| SourceName::new(name))
    }
}

struct SelectivePartialArms<'a> {
    selects_both: bool,
    selected: &'a MatchArm,
    fallback: &'a MatchArm,
    value_binding: Option<FusedName<'a>>,
    error_binding: Option<FusedName<'a>>,
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_match_to_block(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> LoweredBlock {
        let mut statements: Vec<Statement> = Vec::new();

        if self.facts.is_never(&subject.get_type()) {
            statements.push(self.lower_never_value(subject));
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_fused_lowered_match(subject, arms, place, None) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_fused_partial_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_fused_partial_outcome_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_fused_selective_partial_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_fused_option_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_nested_option_pair_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(fused) = self.try_lower_nested_result_pair_match(subject, arms, place) {
            statements.extend(fused);
            return LoweredBlock { statements };
        }

        if let Some(elementwise) = self.try_lower_tuple_subject_match(subject, arms, place) {
            statements.extend(elementwise);
            return LoweredBlock { statements };
        }

        let subject_ty = subject.get_type();
        let resolved = self.lower_match_subject_var(&mut statements, subject, arms);

        let mut block =
            self.lower_match_tree(arms, MatchSubject::Var(resolved.var()), subject_ty, place);
        match resolved {
            ResolvedSubject::Deferred {
                ref name,
                ref expression,
            } if inline_subject_into_header(&mut block.statements, name, expression) => {}
            resolved => resolved.push_declaration(&mut statements, &block.statements),
        }
        statements.extend(block.statements);

        LoweredBlock { statements }
    }

    fn try_lower_nested_option_pair_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        if arms.iter().any(MatchArm::has_guard) {
            return None;
        }
        let fuse = self.option_fuse_plan(subject)?;
        if !matches!(fuse, OptionFusePlan::CommaOk { .. }) {
            return None;
        }
        let mut some_arms = Vec::new();
        let mut none_body = None;
        for arm in arms {
            match prelude_variant(&arm.pattern)? {
                (PreludeVariant::Some, [payload]) => {
                    let mut inner_arm = arm.clone();
                    inner_arm.pattern = payload.clone();
                    some_arms.push(inner_arm);
                }
                (PreludeVariant::None, []) if none_body.is_none() => {
                    none_body = Some(arm.expression.as_ref());
                }
                _ => return None,
            }
        }
        if some_arms.is_empty() {
            return None;
        }
        let none_body = none_body?;
        let payload_ty = self.facts.ok_payload(&subject.get_type());
        let mut bound = fuse.bind(self, CommaOkValueSlot::Temp);
        let value = bound.payload()?;
        let condition = bound.success_condition();
        let then_body = self.lower_match_tree(
            &some_arms,
            MatchSubject::Var(value.clone()),
            payload_ty,
            place,
        );

        let then_body = finish_nested_pair_arm(&mut bound, then_body);
        let else_body = self.lower_block_to_place(none_body, place);
        let else_arm = if then_body.ends_with_diverge() {
            ElseArm::inline(else_body)
        } else {
            ElseArm::block(else_body)
        };
        bound.statements.push(
            LoweredStatement::If(IfPlan::pair_if(
                condition,
                then_body,
                else_arm,
                IfOrigin::Match { else_label: None },
            ))
            .into(),
        );
        Some(bound.statements)
    }

    fn try_lower_tuple_subject_match(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> Option<Vec<Statement>> {
        let Expression::Tuple { elements, .. } = subject else {
            return None;
        };
        if arms.iter().any(MatchArm::has_guard) {
            return None;
        }
        let subject_ty = subject.get_type();
        let mut tested = vec![false; elements.len()];
        for arm in arms {
            if pattern_has_bindings(&arm.pattern) {
                return None;
            }
            let info = self.collect_pattern_info(&arm.pattern, &subject_ty);
            if info.has_root_assertion() || info.requires_materialized_subject {
                return None;
            }
            let arm_tested = decision_tree::tested_tuple_elements(&info.checks, elements.len())?;
            for (element, arm_element) in tested.iter_mut().zip(arm_tested) {
                *element |= arm_element;
            }
        }

        let mut statements: Vec<Statement> = Vec::new();
        let mut stages: Vec<ValuePlan> = elements
            .iter()
            .map(|element| self.plan_composite_value(element, ExpressionContext::value()))
            .collect();
        for ((stage, tested), element) in stages.iter_mut().zip(&tested).zip(elements) {
            let rereadable = is_inert_value(element, stage.expression())
                || stage.rests_in_fixed_name()
                || self.plan_rests_in_stable_name(stage);
            if *tested && !rereadable {
                self.pin_staged(stage, "arg");
            }
        }
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "arg");
        statements.extend(sequenced.setup);

        let mut roots = Vec::with_capacity(elements.len());
        for ((value, tested), element) in sequenced.values.iter().zip(&tested).zip(elements) {
            if !tested && !is_inert_value(element, value) {
                statements.push(discard(value.clone()));
            }
            roots.push(value.clone().without_positions());
        }

        let block = self.lower_match_tree(arms, MatchSubject::Elements(roots), subject_ty, place);
        statements.extend(block.statements);
        Some(statements)
    }

    fn lower_match_tree(
        &mut self,
        arms: &[MatchArm],
        subject_var: MatchSubject,
        subject_ty: Type,
        place: &PlacePlan,
    ) -> LoweredBlock {
        let tree_planner = TreePlanner::new(self, arms, subject_var, subject_ty);
        tree_planner.lower(place)
    }

    fn lower_match_subject_var(
        &mut self,
        setup: &mut Vec<Statement>,
        subject: &Expression,
        arms: &[MatchArm],
    ) -> ResolvedSubject {
        let any_guard = arms.iter().any(|arm| arm.has_guard());
        if let Expression::Identifier {
            value, resolution, ..
        } = subject
            && !any_guard
        {
            let name = value.to_string();
            let has_collision = arms
                .iter()
                .any(|arm| pattern_binds_name(&arm.pattern, &name));
            if self.can_reuse_subject_identifier(&name, resolution, has_collision) {
                let identifier = self.reference_go_identifier(&name, resolution);
                return if identifier.id().is_some() {
                    ResolvedSubject::Materialized {
                        subject: GoExpression::identifier(identifier.clone()),
                        local: identifier,
                    }
                } else {
                    ResolvedSubject::Existing(GoExpression::identifier(identifier))
                };
            }
        }
        if matches!(subject, Expression::Literal { .. }) {
            let staged = self.plan_operand(subject, ExpressionContext::value());
            let (subject_setup, value) = staged.into_parts();
            setup.extend(subject_setup);
            return ResolvedSubject::Existing(value);
        }
        let staged = self.plan_composite_value(subject, ExpressionContext::value());
        let rests_in_stable_name = self.plan_rests_in_stable_name(&staged);
        let (subject_setup, value) = staged.into_parts();
        setup.extend(subject_setup);
        let reads_in_place = matches!(value.node(), GoExpressionNode::Identifier(_))
            || self.field_path_reads_in_place(subject, &value, |root| {
                arms.iter()
                    .any(|arm| pattern_binds_name(&arm.pattern, root))
            });
        if !any_guard && reads_in_place {
            return ResolvedSubject::Existing(value);
        }
        if any_guard
            && rests_in_stable_name
            && let GoExpressionNode::Identifier(var) = value.node()
            && var.id().is_some()
        {
            return ResolvedSubject::Materialized {
                local: var.clone(),
                subject: value,
            };
        }
        let var = self.fresh_declared("subject");
        ResolvedSubject::Deferred {
            name: var,
            expression: value,
        }
    }
}

fn inline_subject_into_header(
    statements: &mut [Statement],
    var: &GoIdentifier,
    expression: &GoExpression,
) -> bool {
    let Some(header) = statements.first_mut().and_then(header_mut) else {
        return false;
    };
    let original = header.clone();
    let Some(read) = leading_subject_read(header.node_mut(), var) else {
        return false;
    };
    *read = expression.node().clone();
    if let Some(position) = expression.position() {
        *header = header.clone().positioned(position.clone());
    }
    let used = GoUses::of(statements);
    if !used.contains_identifier(var) {
        return true;
    }
    *statements
        .first_mut()
        .and_then(header_mut)
        .expect("header was found above") = original;
    false
}

fn header_mut(statement: &mut Statement) -> Option<&mut GoExpression> {
    match &mut statement.kind {
        LoweredStatement::Switch(plan) => match &mut plan.kind {
            SwitchKind::Value { subject } | SwitchKind::Type { subject, .. } => Some(subject),
            SwitchKind::Conditional => None,
        },
        LoweredStatement::If(plan) if plan.initializer.is_none() => Some(&mut plan.condition),
        _ => None,
    }
}

fn leading_subject_read<'n>(
    node: &'n mut GoExpressionNode,
    var: &GoIdentifier,
) -> Option<&'n mut GoExpressionNode> {
    let compares_to_name = matches!(
        node,
        GoExpressionNode::Binary { right, .. } if matches!(
            right.as_ref(),
            GoExpressionNode::Literal(_)
                | GoExpressionNode::Identifier(_)
                | GoExpressionNode::Qualified { .. }
        )
    );

    let mut node = node;
    if compares_to_name && let GoExpressionNode::Binary { left, .. } = node {
        node = left.as_mut();
    }
    if let GoExpressionNode::Selector { base, .. } = node {
        node = base.as_mut();
    }
    matches!(node, GoExpressionNode::Identifier(name) if name.refers_to_same(var)).then_some(node)
}

fn is_inert_value(element: &Expression, value: &GoExpression) -> bool {
    value.is_literal() && (!value.is_composite_literal() || element.get_type().is_unit())
}
