use crate::EmitFacts;
use crate::ir::local::GoIdentifier;
use crate::patterns::binding_decls::pattern_has_bindings;
use crate::state::scope::ScopeState;
use rustc_hash::FxHashMap as HashMap;
use std::borrow::Cow;
use syntax::go_names::ENUM_TAG_FIELD;

use syntax::ast::{
    BindingId, ConstructorPatternResolution, Literal, MatchArm, Pattern, RecordPatternResolution,
    RestPattern, SequencePatternResolution, StructFieldPattern, expand_or_patterns,
};
use syntax::lex::{rune_codepoint, string_bytes};
use syntax::types::{SimpleKind, Type, unqualified_name};

use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::ValueLayout;
use crate::abi::wrapping::NilGuard;
use crate::control_flow::propagation::plain_return;
use crate::ir::bodies::{LoweredBlock, define_many};
use crate::ir::go_expression::{BinaryOp, FunctionLiteralLayout, GoExpressionNode, UnaryOp};
use crate::ir::values::GoExpression;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use crate::patterns::binding_decls::pattern_literal_text;
use crate::types::go_type::GoType;
use syntax::program::SlotOrigin;

mod checks;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathSegment {
    Field(String),
    ValueField(String),
    Index(usize),
    SliceFrom(usize),
    ArraySliceFrom { offset: usize, go_type: GoType },
    Deref,
    NewtypeCast(GoType),
    AssertedAs(GoType),
}

fn element_index(segments: &[PathSegment]) -> usize {
    let [
        PathSegment::Field(field) | PathSegment::ValueField(field),
        ..,
    ] = segments
    else {
        unreachable!("a tuple-element subject only resolves field paths")
    };
    go_name::tuple_field_index(field).expect("a tuple-element subject only resolves tuple fields")
}

#[derive(Clone, Copy)]
pub(crate) enum SubjectRoot<'a> {
    Var(&'a GoExpression),
    Elements(&'a [GoExpression]),
}

impl<'a> SubjectRoot<'a> {
    fn resolve<'p>(&self, segments: &'p [PathSegment]) -> (GoExpression, &'p [PathSegment]) {
        match self {
            Self::Var(var) => ((*var).clone(), segments),
            Self::Elements(elements) => (elements[element_index(segments)].clone(), &segments[1..]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessPath {
    pub segments: Vec<PathSegment>,
}

impl AccessPath {
    pub(crate) fn root() -> Self {
        Self { segments: vec![] }
    }

    fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    fn push(&self, seg: PathSegment) -> Self {
        let mut new = self.clone();
        new.segments.push(seg);
        new
    }

    fn rebase_past(&mut self, segment: &PathSegment) {
        if self.segments.first() == Some(segment) {
            self.segments.remove(0);
        }
    }

    pub(crate) fn render(&self, subject: SubjectRoot<'_>) -> GoExpression {
        let (root, segments) = subject.resolve(&self.segments);
        let mut result = root;
        for seg in segments {
            result = match seg {
                PathSegment::Field(name) => GoExpression::selector(result, name.clone()),
                PathSegment::ValueField(name) => GoExpression::value_field(result, name.clone()),
                PathSegment::Index(index) => {
                    GoExpression::index(result, GoExpression::literal(index.to_string()))
                        .proven_in_bounds()
                }
                PathSegment::SliceFrom(offset) => GoExpression::slice(
                    result,
                    Some(&GoExpression::literal(offset.to_string())),
                    None,
                    None,
                )
                .proven_in_bounds(),
                PathSegment::ArraySliceFrom { offset, go_type } => GoExpression::conversion(
                    go_type.clone(),
                    GoExpression::slice(
                        result,
                        Some(&GoExpression::literal(offset.to_string())),
                        None,
                        None,
                    )
                    .proven_in_bounds(),
                ),
                PathSegment::Deref => GoExpression::dereference(result),
                PathSegment::NewtypeCast(go_type) => {
                    GoExpression::conversion(go_type.clone(), result)
                }
                PathSegment::AssertedAs(ty) => GoExpression::type_assertion(result, ty.clone()),
            };
        }
        result
    }

    pub(crate) fn payload_is_index(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, PathSegment::Index(_)))
    }

    pub(crate) fn contains_deferred_evaluation(&self) -> bool {
        self.segments.iter().any(|segment| {
            matches!(
                segment,
                PathSegment::ArraySliceFrom { .. }
                    | PathSegment::Deref
                    | PathSegment::NewtypeCast(_)
                    | PathSegment::AssertedAs(_)
            )
        })
    }

    fn checked_tuple_element(&self, arity: usize) -> Option<usize> {
        let Some(PathSegment::Field(field) | PathSegment::ValueField(field)) =
            self.segments.first()
        else {
            return None;
        };
        go_name::tuple_field_index(field).filter(|index| *index < arity)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Check {
    Nilable {
        path: AccessPath,
        guard: NilGuard,
        some: bool,
    },
    EnumTag {
        path: AccessPath,
        tag_constant: GoExpression,
    },
    Literal {
        path: AccessPath,
        go_literal: GoExpression,
        value: Option<Literal>,
    },
    SliceLenEq {
        path: AccessPath,
        length: usize,
    },
    SliceLenGe {
        path: AccessPath,
        min_length: usize,
    },
    Or {
        alternatives: Vec<Vec<Check>>,
    },
    TypeAssert {
        path: AccessPath,
        go_type: GoType,
    },
}

#[derive(Clone, Copy)]
enum CheckPolarity {
    Positive,
    Negative,
}

impl Check {
    pub(crate) fn render(&self, scope: &mut ScopeState, subject: SubjectRoot<'_>) -> GoExpression {
        self.render_with_polarity(scope, subject, CheckPolarity::Positive)
    }

    pub(crate) fn render_negated(
        &self,
        scope: &mut ScopeState,
        subject: SubjectRoot<'_>,
    ) -> GoExpression {
        self.render_with_polarity(scope, subject, CheckPolarity::Negative)
    }

    fn render_with_polarity(
        &self,
        scope: &mut ScopeState,
        subject: SubjectRoot<'_>,
        polarity: CheckPolarity,
    ) -> GoExpression {
        let negative = matches!(polarity, CheckPolarity::Negative);
        let length_of = |path: &AccessPath| {
            GoExpression::call(
                GoExpression::external_name("len".to_string()),
                vec![path.render(subject)],
            )
        };
        match self {
            Check::Nilable { path, guard, some } => {
                let value = path.render(subject);
                if *some != negative {
                    guard.non_nil_condition(value)
                } else {
                    guard.nil_condition(value)
                }
            }
            Check::EnumTag { path, tag_constant } => {
                let operator = if negative { BinaryOp::Ne } else { BinaryOp::Eq };
                GoExpression::binary(
                    GoExpression::selector(path.render(subject), ENUM_TAG_FIELD.to_string()),
                    operator,
                    tag_constant.clone(),
                )
            }
            Check::Literal {
                path, go_literal, ..
            } => {
                let rendered_path = path.render(subject);
                match (boolean_literal(go_literal), negative) {
                    (Some(true), false) | (Some(false), true) => rendered_path,
                    (Some(true), true) | (Some(false), false) => {
                        GoExpression::unary(UnaryOp::Not, rendered_path)
                    }
                    (None, false) => {
                        GoExpression::binary(rendered_path, BinaryOp::Eq, go_literal.clone())
                    }
                    (None, true) => {
                        GoExpression::binary(rendered_path, BinaryOp::Ne, go_literal.clone())
                    }
                }
            }
            Check::SliceLenEq { path, length } => {
                let operator = if negative { BinaryOp::Ne } else { BinaryOp::Eq };
                GoExpression::binary(
                    length_of(path),
                    operator,
                    GoExpression::literal(length.to_string()),
                )
            }
            Check::SliceLenGe { path, min_length } => {
                let operator = if negative { BinaryOp::Lt } else { BinaryOp::Ge };
                GoExpression::binary(
                    length_of(path),
                    operator,
                    GoExpression::literal(min_length.to_string()),
                )
            }
            Check::Or { alternatives } if !negative => alternatives
                .iter()
                .map(|checks| join_conditions(scope, checks, subject))
                .reduce(|left, right| GoExpression::binary(left, BinaryOp::Or, right))
                .expect("an or-check has at least one alternative"),
            Check::TypeAssert { path, go_type } if !negative => {
                let ok = scope.new_local_identifier("ok");
                GoExpression::immediate_call(
                    GoType::new("bool"),
                    LoweredBlock {
                        statements: vec![
                            define_many(
                                vec![GoIdentifier::external("_".to_string()), ok.clone()],
                                GoExpression::type_assertion(path.render(subject), go_type.clone()),
                            ),
                            plain_return(GoExpression::identifier(ok)),
                        ],
                    },
                    FunctionLiteralLayout::Inline,
                )
            }
            Check::Or { .. } | Check::TypeAssert { .. } => GoExpression::unary(
                UnaryOp::Not,
                self.render_with_polarity(scope, subject, CheckPolarity::Positive),
            ),
        }
    }

    fn rebase_past(&mut self, segment: &PathSegment) {
        match self {
            Check::Nilable { path, .. }
            | Check::EnumTag { path, .. }
            | Check::Literal { path, .. }
            | Check::SliceLenEq { path, .. }
            | Check::SliceLenGe { path, .. }
            | Check::TypeAssert { path, .. } => path.rebase_past(segment),
            Check::Or { alternatives } => {
                for check in alternatives.iter_mut().flatten() {
                    check.rebase_past(segment);
                }
            }
        }
    }

    fn path(&self) -> Option<&AccessPath> {
        match self {
            Check::Nilable { path, .. }
            | Check::EnumTag { path, .. }
            | Check::Literal { path, .. }
            | Check::SliceLenEq { path, .. }
            | Check::SliceLenGe { path, .. }
            | Check::TypeAssert { path, .. } => Some(path),
            Check::Or { .. } => None,
        }
    }

    fn as_enum_tag(&self) -> Option<&GoExpression> {
        match self {
            Check::EnumTag { tag_constant, .. } => Some(tag_constant),
            _ => None,
        }
    }

    fn as_literal(&self) -> Option<&GoExpression> {
        match self {
            Check::Literal { go_literal, .. } => Some(go_literal),
            _ => None,
        }
    }

    fn switch_label_checks(&self) -> Option<Vec<&Check>> {
        let Check::Or { alternatives } = self else {
            return Some(vec![self]);
        };
        alternatives
            .iter()
            .map(|alternative| match alternative.as_slice() {
                [check] if !matches!(check, Check::Or { .. }) => Some(check),
                _ => None,
            })
            .collect()
    }

    fn switch_labels(&self, kind: &SwitchKind) -> Option<(&AccessPath, Vec<&GoExpression>)> {
        let checks = self.switch_label_checks()?;
        let path = checks.first()?.path()?;
        let labels = checks
            .iter()
            .map(|check| {
                if check.path()? != path {
                    return None;
                }
                match kind {
                    SwitchKind::EnumTag => check.as_enum_tag(),
                    SwitchKind::Value => check.as_literal(),
                }
            })
            .collect::<Option<_>>()?;
        Some((path, labels))
    }

    fn label_value(&self) -> Option<LabelValue<'_>> {
        let Check::Literal {
            value: Some(value), ..
        } = self
        else {
            return None;
        };
        match value {
            Literal::Integer { value, .. } => Some(LabelValue::Integer(*value)),
            Literal::Char(text) => {
                rune_codepoint(text).map(|code| LabelValue::Integer(code.into()))
            }
            Literal::Boolean(value) => Some(LabelValue::Boolean(*value)),
            Literal::String { value, raw } => string_bytes(value, *raw).map(LabelValue::Bytes),
            _ => None,
        }
    }
}

#[derive(PartialEq)]
enum LabelValue<'a> {
    Integer(u64),
    Boolean(bool),
    Bytes(Cow<'a, [u8]>),
}

pub(crate) fn tested_tuple_elements(checks: &[Check], arity: usize) -> Option<Vec<bool>> {
    let mut tested = vec![false; arity];
    for check in checks {
        if let Some(path) = check.path() {
            tested[path.checked_tuple_element(arity)?] = true;
            continue;
        }
        let Check::Or { alternatives } = check else {
            unreachable!("only Or checks lack a path")
        };
        let mut alternatives = alternatives
            .iter()
            .map(|checks| tested_tuple_elements(checks, arity));
        let first = alternatives.next()??;
        if !alternatives.all(|alternative| alternative.as_ref() == Some(&first)) {
            return None;
        }
        for (tested, alternative) in tested.iter_mut().zip(first) {
            *tested |= alternative;
        }
    }
    Some(tested)
}

#[derive(Clone, Debug)]
pub(crate) struct PatternBinding {
    pub lisette_name: String,
    pub binding_ids: Vec<BindingId>,
    pub target: BindingTarget,
    pub path: AccessPath,
    pub coercion: CoercionPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BindingTarget {
    Unused,
    Unit,
    Named { go_name: SourceName, ty: Type },
}

impl BindingTarget {
    fn from_go_name(go_name: Option<SourceName>, ty: &Type) -> Self {
        go_name.map_or(Self::Unused, |go_name| Self::Named {
            go_name,
            ty: ty.clone(),
        })
    }

    pub(crate) fn go_name(&self) -> Option<&SourceName> {
        match self {
            Self::Named { go_name, .. } => Some(go_name),
            Self::Unused | Self::Unit => None,
        }
    }

    pub(crate) fn is_named(&self) -> bool {
        matches!(self, Self::Named { .. })
    }

    pub(super) fn shared_with(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unit, _) | (_, Self::Unit) => Self::Unit,
            (Self::Unused, _) | (_, Self::Unused) => Self::Unused,
            (Self::Named { .. }, Self::Named { .. }) => self.clone(),
        }
    }
}

pub(crate) struct PatternInfo {
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
    pub requires_materialized_subject: bool,
}

impl PatternInfo {
    fn new() -> Self {
        Self {
            checks: Vec::new(),
            bindings: Vec::new(),
            requires_materialized_subject: false,
        }
    }

    pub(crate) fn is_irrefutable(&self) -> bool {
        self.checks.is_empty()
    }

    pub(crate) fn has_root_assertion(&self) -> bool {
        root_assertion_position(&self.checks).is_some()
    }
}

#[derive(Debug)]
pub(crate) struct ArmLeaf {
    pub arm_index: usize,
    pub bindings: Vec<PatternBinding>,
}

#[derive(Debug)]
pub(crate) struct GuardedLeaf {
    pub leaf: ArmLeaf,
    pub failure: Box<Decision>,
}

#[derive(Debug)]
pub(crate) enum Decision {
    Success(ArmLeaf),
    Guard(GuardedLeaf),
    Switch(ValueSwitch),
    TypeSwitch {
        branches: Vec<SwitchBranch<Vec<GoType>>>,
        fallback: Option<Box<Decision>>,
    },
    Chain {
        tests: Vec<ChainTest>,
        tail: ChainTail,
    },
    Unreachable,
}

impl Decision {
    pub(crate) fn first_arm(&self) -> Option<usize> {
        match self {
            Decision::Success(leaf) => Some(leaf.arm_index),
            Decision::Guard(guard) => Some(guard.leaf.arm_index),
            Decision::Chain { tests, tail } => match (tests.first(), tail) {
                (Some(test), _) => Some(test.arm.leaf().arm_index),
                (None, ChainTail::Catchall(catchall)) => Some(catchall.arm_index),
                (None, ChainTail::LastTest | ChainTail::Open) => None,
            },
            Decision::Switch(switch) => switch
                .branches
                .first()
                .and_then(|branch| branch.decision.first_arm())
                .or_else(|| switch.fallback.as_ref()?.first_arm()),
            Decision::TypeSwitch { branches, fallback } => branches
                .first()
                .and_then(|branch| branch.decision.first_arm())
                .or_else(|| fallback.as_ref()?.first_arm()),
            Decision::Unreachable => None,
        }
    }

    pub(crate) fn has_unguarded_terminal(&self) -> bool {
        match self {
            Decision::Success(_) => true,
            Decision::Guard(guard) => guard.failure.has_unguarded_terminal(),
            Decision::Chain { tests, tail } => {
                matches!(tail, ChainTail::Catchall(_))
                    || tests.last().is_some_and(|t| match &t.arm {
                        ChainArm::Leaf(_) => true,
                        ChainArm::Guard(guard) => guard.failure.has_unguarded_terminal(),
                    })
            }
            Decision::Switch(ValueSwitch { fallback, .. })
            | Decision::TypeSwitch { fallback, .. } => fallback
                .as_deref()
                .is_none_or(Decision::has_unguarded_terminal),
            Decision::Unreachable => false,
        }
    }
}

#[derive(Debug)]
pub(crate) enum ChainTail {
    Catchall(ArmLeaf),
    LastTest,
    Open,
}

#[derive(Debug)]
pub(crate) struct ValueSwitch {
    pub path: AccessPath,
    pub kind: SwitchKind,
    pub branches: Vec<SwitchBranch<GoExpression>>,
    pub fallback: Option<Box<Decision>>,
}

impl ValueSwitch {
    pub(crate) fn shape(&self) -> SwitchShape {
        match (&self.kind, self.branches.as_slice(), &self.fallback) {
            (SwitchKind::Value, [left, right], None)
                if matches!(
                    (boolean_literal(&left.label), boolean_literal(&right.label)),
                    (Some(true), Some(false)) | (Some(false), Some(true))
                ) =>
            {
                SwitchShape::Bool
            }
            (_, [_, _], None) => SwitchShape::Binary,
            (_, [_], _) => SwitchShape::SingleArm,
            _ => SwitchShape::Multi,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SwitchKind {
    EnumTag,
    Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchShape {
    Bool,
    Binary,
    SingleArm,
    Multi,
}

pub(crate) fn boolean_literal(expression: &GoExpression) -> Option<bool> {
    match expression.node() {
        GoExpressionNode::Literal(text) if text == "true" => Some(true),
        GoExpressionNode::Literal(text) if text == "false" => Some(false),
        _ => None,
    }
}

#[derive(Debug)]
pub(crate) struct SwitchBranch<L> {
    pub label: L,
    pub extra_labels: Vec<L>,
    pub decision: Decision,
}

#[derive(Debug)]
pub(crate) struct ChainTest {
    pub checks: Vec<Check>,
    pub arm: ChainArm,
}

#[derive(Debug)]
pub(crate) enum ChainArm {
    Leaf(ArmLeaf),
    Guard(GuardedLeaf),
}

impl ChainArm {
    pub fn leaf(&self) -> &ArmLeaf {
        match self {
            ChainArm::Leaf(leaf) | ChainArm::Guard(GuardedLeaf { leaf, .. }) => leaf,
        }
    }
}

#[derive(Clone)]
struct ArmInfo {
    arm_index: usize,
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
    has_guard: bool,
}

impl ArmInfo {
    fn is_catchall(&self) -> bool {
        self.checks.is_empty()
    }

    fn has_root_assertion(&self) -> bool {
        root_assertion_position(&self.checks).is_some()
    }

    fn is_interface_or_with_extras(&self) -> bool {
        if self.checks.len() != 1 {
            return false;
        }
        let Check::Or { alternatives } = &self.checks[0] else {
            return false;
        };
        alternatives
            .iter()
            .all(|alt| matches!(alt.first(), Some(Check::TypeAssert { .. })))
            && alternatives.iter().any(|alt| alt.len() > 1)
    }
}

fn build_tree(arms: Vec<ArmInfo>) -> Decision {
    if arms.is_empty() {
        return Decision::Unreachable;
    }

    let first_is_catchall = arms[0].is_catchall();

    if first_is_catchall && !arms[0].has_guard {
        return Decision::Success(ArmLeaf {
            arm_index: arms[0].arm_index,
            bindings: arms[0].bindings.clone(),
        });
    }

    if first_is_catchall && arms[0].has_guard {
        let rest = arms[1..].to_vec();
        return Decision::Guard(GuardedLeaf {
            leaf: ArmLeaf {
                arm_index: arms[0].arm_index,
                bindings: arms[0].bindings.clone(),
            },
            failure: Box::new(build_tree(rest)),
        });
    }

    if let Some(switch) = try_build_switch(&arms) {
        return switch;
    }

    build_chain(arms)
}

fn try_build_switch(arms: &[ArmInfo]) -> Option<Decision> {
    let first_relevant = arms.first().filter(|a| !a.is_catchall())?;
    let split = arms
        .iter()
        .position(ArmInfo::is_catchall)
        .unwrap_or(arms.len());
    let (cases, fallback_arms) = arms.split_at(split);
    if first_relevant.has_root_assertion() {
        if !arms
            .iter()
            .all(|a| a.is_catchall() || a.has_root_assertion())
        {
            return None;
        }
        let (branches, fallback) = build_switch_branches(cases, fallback_arms, |arm| {
            (
                lift_root_assertion(&mut arm.checks, &mut arm.bindings).unwrap(),
                Vec::new(),
            )
        });
        return Some(Decision::TypeSwitch { branches, fallback });
    }

    let first_check = first_relevant.checks.first()?;
    let (kind, path) = [SwitchKind::EnumTag, SwitchKind::Value]
        .into_iter()
        .find_map(|kind| {
            let path = first_check.switch_labels(&kind)?.0.clone();
            Some((kind, path))
        })?;
    validate_value_switch_arms(arms, &kind, &path)?;

    let (branches, fallback) = build_switch_branches(cases, fallback_arms, |arm| {
        let first = arm.checks.remove(0);
        let mut labels = first.switch_labels(&kind).unwrap().1.into_iter().cloned();
        (labels.next().unwrap(), labels.collect())
    });
    if branches
        .iter()
        .any(|branch| !branch.extra_labels.is_empty())
        && branches.len() + usize::from(fallback.is_some()) < 3
    {
        return None;
    }
    Some(Decision::Switch(ValueSwitch {
        path,
        kind,
        branches,
        fallback,
    }))
}

fn validate_value_switch_arms(
    arms: &[ArmInfo],
    kind: &SwitchKind,
    switch_path: &AccessPath,
) -> Option<()> {
    let mut alternatives = Vec::new();
    let mut singles = Vec::new();
    let mut values = Vec::new();
    for arm in arms {
        if arm.is_catchall() {
            continue;
        }
        if arm.has_guard {
            return None;
        }

        let first = arm.checks.first()?;
        let (path, labels) = first.switch_labels(kind)?;
        if matches!(kind, SwitchKind::Value) && arm.checks.len() != 1 {
            return None;
        }
        if path != switch_path {
            return None;
        }
        if labels.len() > 1 {
            alternatives.extend(labels);
        } else {
            singles.extend(labels);
        }
        if matches!(kind, SwitchKind::Value) {
            values.extend(
                first
                    .switch_label_checks()?
                    .iter()
                    .map(|check| check.label_value()),
            );
        }
    }
    if values.len() > 1
        && values
            .iter()
            .enumerate()
            .any(|(index, value)| value.is_none() || values[index + 1..].contains(value))
    {
        return None;
    }
    let shared = alternatives
        .iter()
        .enumerate()
        .any(|(index, label)| alternatives[index + 1..].contains(label) || singles.contains(label));
    (!shared).then_some(())
}

fn build_switch_branches<L: PartialEq>(
    cases: &[ArmInfo],
    fallback_arms: &[ArmInfo],
    take_label: impl Fn(&mut ArmInfo) -> (L, Vec<L>),
) -> (Vec<SwitchBranch<L>>, Option<Box<Decision>>) {
    let mut grouped: Vec<(L, Vec<L>, Vec<ArmInfo>)> = Vec::new();

    for arm in cases {
        let mut inner_arm = arm.clone();
        let (case_label, extra_labels) = take_label(&mut inner_arm);

        if let Some((_, _, arms)) = grouped
            .iter_mut()
            .find(|(label, _, _)| label == &case_label)
        {
            arms.push(inner_arm);
        } else {
            grouped.push((case_label, extra_labels, vec![inner_arm]));
        }
    }

    let branches = grouped
        .into_iter()
        .map(|(label, extra_labels, inner_arms)| {
            let any_inner_can_fail = inner_arms
                .iter()
                .any(|a| a.has_guard || !a.checks.is_empty());
            let decision = if any_inner_can_fail {
                let mut arms_with_fallback = inner_arms;
                arms_with_fallback.extend(fallback_arms.iter().cloned());
                build_tree(arms_with_fallback)
            } else {
                build_tree(inner_arms)
            };
            SwitchBranch {
                label,
                extra_labels,
                decision,
            }
        })
        .collect();

    let fallback = if fallback_arms.is_empty() {
        None
    } else {
        Some(Box::new(build_tree(fallback_arms.to_vec())))
    };
    (branches, fallback)
}

fn build_chain(arms: Vec<ArmInfo>) -> Decision {
    let mut tests = Vec::new();

    for (i, arm) in arms.iter().enumerate() {
        if arm.is_catchall() && !arm.has_guard {
            let catchall = ArmLeaf {
                arm_index: arm.arm_index,
                bindings: arm.bindings.clone(),
            };
            return if tests.is_empty() {
                Decision::Success(catchall)
            } else {
                Decision::Chain {
                    tests,
                    tail: ChainTail::Catchall(catchall),
                }
            };
        }

        let leaf = ArmLeaf {
            arm_index: arm.arm_index,
            bindings: arm.bindings.clone(),
        };
        let chain_arm = if arm.has_guard {
            let remaining = arms[i + 1..].to_vec();
            ChainArm::Guard(GuardedLeaf {
                leaf,
                failure: Box::new(build_tree(remaining)),
            })
        } else {
            ChainArm::Leaf(leaf)
        };

        tests.push(ChainTest {
            checks: arm.checks.clone(),
            arm: chain_arm,
        });
    }

    let tail = if tests.len() > 1 {
        ChainTail::LastTest
    } else {
        ChainTail::Open
    };
    Decision::Chain { tests, tail }
}

fn expand_interface_or_checks(arm_infos: Vec<ArmInfo>) -> Vec<ArmInfo> {
    if !arm_infos.iter().any(ArmInfo::is_interface_or_with_extras) {
        return arm_infos;
    }
    let mut result = Vec::with_capacity(arm_infos.len());
    for arm in arm_infos {
        if arm.is_interface_or_with_extras() {
            let Check::Or { alternatives } = &arm.checks[0] else {
                unreachable!()
            };
            for alt in alternatives {
                result.push(ArmInfo {
                    arm_index: arm.arm_index,
                    checks: alt.clone(),
                    bindings: arm.bindings.clone(),
                    has_guard: arm.has_guard,
                });
            }
        } else {
            result.push(arm);
        }
    }
    result
}

pub(super) fn share_alternative_bindings(alternatives: &mut [&mut Vec<PatternBinding>]) {
    let mut shared_by_name: HashMap<String, (Vec<BindingId>, BindingTarget)> = HashMap::default();
    for bindings in alternatives.iter() {
        for binding in bindings.iter() {
            let (ids, target) = shared_by_name
                .entry(binding.lisette_name.clone())
                .or_insert_with(|| (Vec::new(), binding.target.clone()));
            for id in &binding.binding_ids {
                if !ids.contains(id) {
                    ids.push(*id);
                }
            }
            *target = target.shared_with(&binding.target);
        }
    }
    for bindings in alternatives.iter_mut() {
        for binding in bindings.iter_mut() {
            let (ids, target) = &shared_by_name[&binding.lisette_name];
            if !target.is_named() {
                binding.target = target.clone();
            }
            binding.binding_ids = ids.clone();
        }
    }
}

fn root_assertion_position(checks: &[Check]) -> Option<usize> {
    checks.iter().position(|c| match c {
        Check::TypeAssert { path, .. } => path.is_root(),
        Check::Or { alternatives } => alternatives.iter().all(
            |alt| matches!(alt.as_slice(), [Check::TypeAssert { path, .. }] if path.is_root()),
        ),
        _ => false,
    })
}

pub(super) fn lift_root_assertion(
    checks: &mut Vec<Check>,
    bindings: &mut [PatternBinding],
) -> Option<Vec<GoType>> {
    let position = root_assertion_position(checks)?;
    match checks.remove(position) {
        Check::TypeAssert { go_type, .. } => {
            let asserted = PathSegment::AssertedAs(go_type.clone());
            for check in checks.iter_mut() {
                check.rebase_past(&asserted);
            }
            for binding in bindings {
                binding.path.rebase_past(&asserted);
            }
            Some(vec![go_type])
        }
        Check::Or { alternatives } => {
            let go_types: Vec<GoType> = alternatives
                .into_iter()
                .map(|alt| match <[Check; 1]>::try_from(alt) {
                    Ok([Check::TypeAssert { go_type, .. }]) => go_type,
                    _ => unreachable!("predicate above confirmed shape"),
                })
                .collect();
            debug_assert!(!go_types.is_empty(), "at least one alternative");
            Some(go_types)
        }
        _ => unreachable!(),
    }
}

pub(super) fn render_condition(
    scope: &mut ScopeState,
    checks: &[Check],
    subject_var: SubjectRoot<'_>,
) -> GoExpression {
    if checks.is_empty() {
        return GoExpression::literal("true".to_string());
    }
    join_conditions(scope, checks, subject_var)
}

fn join_conditions(
    scope: &mut ScopeState,
    checks: &[Check],
    subject: SubjectRoot<'_>,
) -> GoExpression {
    checks
        .iter()
        .map(|check| check.render(scope, subject))
        .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
        .expect("join_conditions requires at least one check")
}

impl Planner<'_> {
    pub(super) fn compile_match_arms(&mut self, arms: &[MatchArm], subject_ty: &Type) -> Decision {
        let mut arm_infos = Vec::new();
        for (arm_index, arm) in arms.iter().enumerate() {
            let split = |pattern: &Pattern| arm.has_guard() || pattern_has_bindings(pattern);
            for expansion in expand_or_patterns(&arm.pattern, &split) {
                let info = self.collect_pattern_info(&expansion.pattern, subject_ty);
                arm_infos.push(ArmInfo {
                    arm_index,
                    checks: info.checks,
                    bindings: info.bindings,
                    has_guard: arm.has_guard(),
                });
            }
        }

        let mut arm_infos = expand_interface_or_checks(arm_infos);

        for alternatives in arm_infos.chunk_by_mut(|a, b| a.arm_index == b.arm_index) {
            let mut bindings: Vec<_> = alternatives.iter_mut().map(|a| &mut a.bindings).collect();
            share_alternative_bindings(&mut bindings);
        }

        build_tree(arm_infos)
    }

    pub(crate) fn collect_pattern_info(&self, pattern: &Pattern, subject_ty: &Type) -> PatternInfo {
        let mut info = PatternInfo::new();
        self.collect_checks_and_bindings(
            &AccessPath::root(),
            pattern,
            subject_ty,
            Some(&self.value_layout(subject_ty, SlotOrigin::Lisette)),
            &mut info,
        );
        info
    }
}

impl EmitFacts<'_> {
    fn field_segment(&self, receiver_ty: &Type, name: String) -> PathSegment {
        if self.field_read_cannot_panic(receiver_ty) {
            PathSegment::ValueField(name)
        } else {
            PathSegment::Field(name)
        }
    }
}
