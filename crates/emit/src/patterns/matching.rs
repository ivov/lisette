use crate::Planner;
use crate::abi::callable::LoweredReturnAbi;
use crate::abi::fallible_producer::OptionFusePlan;
use crate::abi::wrapping::{nil_condition, non_nil_condition, unexpected_nil_error};
use crate::calls::bound_value::{BoundValue, PairCondition};
use crate::calls::comma_ok::CommaOkValueSlot;

use crate::calls::wrap_err::WrapMessage;
use crate::ir::bodies::GoUses;
use crate::ir::bodies::{
    Definition, ElseArm, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, PlacePlan, Statement,
    SwitchKind, assign, define, define_many, discard, expression_statement,
};
use crate::ir::go_expression::{BinaryOp, GoExpressionNode};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{CaptureBoundary, GoExpression, ValuePlan};
use crate::names::source_name::SourceName;
use crate::patterns::binding_decls::{
    has_binding_or_pattern, pattern_binds_name, pattern_has_bindings,
};
use crate::patterns::decision_tree;
use crate::patterns::sites::ResolvedSubject;
use crate::patterns::tree_planner::{MatchSubject, TreePlanner};
use crate::plan::calls::{CallPlan, CallableOrigin};
use crate::state::expression::ExpressionContext;
use crate::state::scope::PairStatusKind;
use syntax::EcoString;
use syntax::ast::{BindingId, ConstructorPatternResolution, Expression, MatchArm, Pattern};
use syntax::program;
use syntax::types::{Type, unqualified_name};

mod fused;
pub(crate) use fused::*;

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
pub(crate) struct FusedName<'a> {
    pub(crate) name: &'a EcoString,
    pub(crate) binding: Option<BindingId>,
}

impl FusedName<'_> {
    pub(crate) fn go_name(&self) -> SourceName {
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PartialVariant {
    Ok,
    Both,
    Err,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreludeVariant {
    Some,
    None,
    Ok,
    Err,
    Partial(PartialVariant),
}

impl PreludeVariant {
    fn from_resolution(enum_name: &str, variant_name: &str) -> Option<Self> {
        Some(match (enum_name, variant_name) {
            ("prelude.Option", "Some") => Self::Some,
            ("prelude.Option", "None") => Self::None,
            ("prelude.Result", "Ok") => Self::Ok,
            ("prelude.Result", "Err") => Self::Err,
            ("prelude.Partial", "Ok") => Self::Partial(PartialVariant::Ok),
            ("prelude.Partial", "Both") => Self::Partial(PartialVariant::Both),
            ("prelude.Partial", "Err") => Self::Partial(PartialVariant::Err),
            _ => return None,
        })
    }
}

pub(crate) fn prelude_variant(pattern: &Pattern) -> Option<(PreludeVariant, &[Pattern])> {
    let Pattern::EnumVariant {
        fields,
        rest: false,
        resolution:
            ConstructorPatternResolution::EnumVariant {
                enum_name,
                variant_name,
            },
        ..
    } = pattern
    else {
        return None;
    };
    let variant = PreludeVariant::from_resolution(enum_name, unqualified_name(variant_name))?;
    Some((variant, fields))
}

pub(crate) fn prelude_constructor(expression: &Expression) -> Option<PreludeVariant> {
    let (enum_name, variant_name) = program::prelude_constructor(expression)?;
    PreludeVariant::from_resolution(enum_name, variant_name)
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
    pub(crate) fn lower_match_to_block(
        &mut self,
        subject: &Expression,
        arms: &[MatchArm],
        place: &PlacePlan,
    ) -> LoweredBlock {
        let mut statements: Vec<Statement> = Vec::new();

        if subject.get_type().is_never() {
            statements.push(self.lower_statement(subject));
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
        let else_arm = ElseArm::from_body(else_body, then_body.ends_with_diverge());
        bound.statements.push(
            LoweredStatement::If(IfPlan {
                initializer: condition.initializer,
                condition: condition.condition,
                then_body,
                else_arm,
                else_if_line: None,
                origin: IfOrigin::Match { else_label: None },
            })
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
        let Expression::Tuple { elements, .. } = subject.unwrap_parens() else {
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

fn pair_if(test: PairCondition, then_body: LoweredBlock, else_arm: ElseArm) -> IfPlan {
    IfPlan {
        initializer: test.initializer,
        condition: test.condition,
        then_body,
        else_arm,
        else_if_line: None,
        origin: IfOrigin::Match { else_label: None },
    }
}

fn is_inert_value(element: &Expression, value: &GoExpression) -> bool {
    value.is_literal() && (!value.is_composite_literal() || element.get_type().is_unit())
}
