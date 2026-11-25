use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::state::scope::ScopeState;
use crate::lowering::patterns::binding_declarations::pattern_has_bindings;
use crate::output::finish::function_literals::immediate_call;
use crate::target::ir::go_expression::GoLiteral;
use crate::target::ir::local::GoIdentifier;
use rustc_hash::FxHashMap;
use std::borrow::Cow;
use syntax::go_names::ENUM_TAG_FIELD;

use syntax::ast::{
    BindingId, ConstructorPatternResolution, Literal, MatchArm, Pattern, RecordPatternResolution,
    RestPattern, SequencePatternResolution, StructFieldPattern, expand_or_patterns,
};
use syntax::lex::{rune_codepoint, string_bytes};
use syntax::types::{SimpleKind, Type, unqualified_name};

use crate::Planner;
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::patterns::binding_declarations::pattern_literal;
use crate::target::ir::bodies::{LoweredBlock, define_many};
use crate::target::ir::go_expression::{
    FunctionLiteralLayout, GoBinaryOperator, GoExpressionNode, GoUnaryOperator,
};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use syntax::program::SlotOrigin;

mod checks;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum PathSegment {
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
pub(in crate::lowering) enum SubjectRoot<'a> {
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
pub(in crate::lowering) struct AccessPath {
    pub segments: Vec<PathSegment>,
}

impl AccessPath {
    pub(in crate::lowering) fn root() -> Self {
        Self { segments: vec![] }
    }

    fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    fn push(&self, segment: PathSegment) -> Self {
        let mut new = self.clone();
        new.segments.push(segment);
        new
    }

    fn rebase_past(&mut self, segment: &PathSegment) {
        if self.segments.first() == Some(segment) {
            self.segments.remove(0);
        }
    }

    pub(in crate::lowering) fn render(&self, subject: SubjectRoot<'_>) -> GoExpression {
        let (root, segments) = subject.resolve(&self.segments);
        let mut result = root;
        for segment in segments {
            result = match segment {
                PathSegment::Field(name) => GoExpression::selector(result, name.clone()),
                PathSegment::ValueField(name) => GoExpression::value_field(result, name.clone()),
                PathSegment::Index(index) => {
                    GoExpression::index(result, GoExpression::integer(*index as i128))
                        .proven_in_bounds()
                }
                PathSegment::SliceFrom(offset) => GoExpression::slice(
                    result,
                    Some(&GoExpression::integer(*offset as i128)),
                    None,
                    None,
                )
                .proven_in_bounds(),
                PathSegment::ArraySliceFrom { offset, go_type } => GoExpression::conversion(
                    go_type.clone(),
                    GoExpression::slice(
                        result,
                        Some(&GoExpression::integer(*offset as i128)),
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

    pub(in crate::lowering) fn payload_is_index(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, PathSegment::Index(_)))
    }

    pub(in crate::lowering) fn contains_deferred_evaluation(&self) -> bool {
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
pub(in crate::lowering) enum Check {
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
    SliceLengthEqual {
        path: AccessPath,
        length: usize,
    },
    SliceLengthAtLeast {
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
    pub(in crate::lowering) fn render(
        &self,
        scope: &mut ScopeState,
        subject: SubjectRoot<'_>,
    ) -> GoExpression {
        self.render_with_polarity(scope, subject, CheckPolarity::Positive)
    }

    pub(in crate::lowering) fn render_negated(
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
                let operator = if negative {
                    GoBinaryOperator::NotEqual
                } else {
                    GoBinaryOperator::Equal
                };
                GoExpression::binary(
                    GoExpression::selector(path.render(subject), ENUM_TAG_FIELD.to_string()),
                    operator,
                    tag_constant.clone(),
                )
            }
            Check::Literal {
                path, go_literal, ..
            } => render_literal_check(path.render(subject), go_literal, polarity),
            Check::SliceLengthEqual { path, length } => {
                let operator = if negative {
                    GoBinaryOperator::NotEqual
                } else {
                    GoBinaryOperator::Equal
                };
                render_length_check(path.render(subject), operator, *length)
            }
            Check::SliceLengthAtLeast { path, min_length } => {
                let operator = if negative {
                    GoBinaryOperator::LessThan
                } else {
                    GoBinaryOperator::GreaterThanOrEqual
                };
                render_length_check(path.render(subject), operator, *min_length)
            }
            Check::Or { alternatives } if !negative => alternatives
                .iter()
                .map(|checks| render_condition(scope, checks, subject))
                .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::Or, right))
                .expect("an or-check has at least one alternative"),
            Check::TypeAssert { path, go_type } if !negative => {
                render_type_assert_check(scope, path.render(subject), go_type)
            }
            Check::Or { .. } | Check::TypeAssert { .. } => GoExpression::unary(
                GoUnaryOperator::Not,
                self.render_with_polarity(scope, subject, CheckPolarity::Positive),
            ),
        }
    }

    fn target(&self) -> CheckTarget<'_> {
        match self {
            Check::Nilable { path, .. }
            | Check::EnumTag { path, .. }
            | Check::Literal { path, .. }
            | Check::SliceLengthEqual { path, .. }
            | Check::SliceLengthAtLeast { path, .. }
            | Check::TypeAssert { path, .. } => CheckTarget::Path(path),
            Check::Or { alternatives } => CheckTarget::Alternatives(alternatives),
        }
    }

    fn rebase_past(&mut self, segment: &PathSegment) {
        match self {
            Check::Nilable { path, .. }
            | Check::EnumTag { path, .. }
            | Check::Literal { path, .. }
            | Check::SliceLengthEqual { path, .. }
            | Check::SliceLengthAtLeast { path, .. }
            | Check::TypeAssert { path, .. } => path.rebase_past(segment),
            Check::Or { alternatives } => {
                for check in alternatives.iter_mut().flatten() {
                    check.rebase_past(segment);
                }
            }
        }
    }

    fn path(&self) -> Option<&AccessPath> {
        match self.target() {
            CheckTarget::Path(path) => Some(path),
            CheckTarget::Alternatives(_) => None,
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

enum CheckTarget<'a> {
    Path(&'a AccessPath),
    Alternatives(&'a [Vec<Check>]),
}

fn render_literal_check(
    rendered_path: GoExpression,
    go_literal: &GoExpression,
    polarity: CheckPolarity,
) -> GoExpression {
    let negative = matches!(polarity, CheckPolarity::Negative);
    match (boolean_literal(go_literal), negative) {
        (Some(true), false) | (Some(false), true) => rendered_path,
        (Some(true), true) | (Some(false), false) => {
            GoExpression::unary(GoUnaryOperator::Not, rendered_path)
        }
        (None, false) => {
            GoExpression::binary(rendered_path, GoBinaryOperator::Equal, go_literal.clone())
        }
        (None, true) => GoExpression::binary(
            rendered_path,
            GoBinaryOperator::NotEqual,
            go_literal.clone(),
        ),
    }
}

fn render_length_check(
    rendered_path: GoExpression,
    operator: GoBinaryOperator,
    length: usize,
) -> GoExpression {
    GoExpression::binary(
        GoExpression::call(
            GoExpression::external_name("len".to_string()),
            vec![rendered_path],
        ),
        operator,
        GoExpression::integer(length as i128),
    )
}

fn render_type_assert_check(
    scope: &mut ScopeState,
    rendered_path: GoExpression,
    go_type: &GoType,
) -> GoExpression {
    let ok = scope.new_local_identifier("ok");
    immediate_call(
        GoType::new("bool"),
        LoweredBlock {
            statements: vec![
                define_many(
                    vec![GoIdentifier::blank(), ok.clone()],
                    GoExpression::type_assertion(rendered_path, go_type.clone()),
                ),
                plain_return(GoExpression::identifier(ok)),
            ],
        },
        FunctionLiteralLayout::Inline,
    )
}

#[derive(PartialEq)]
enum LabelValue<'a> {
    Integer(u64),
    Boolean(bool),
    Bytes(Cow<'a, [u8]>),
}

pub(in crate::lowering) fn tested_tuple_elements(
    checks: &[Check],
    arity: usize,
) -> Option<Vec<bool>> {
    let mut tested = vec![false; arity];
    for check in checks {
        let alternatives = match check.target() {
            CheckTarget::Path(path) => {
                tested[path.checked_tuple_element(arity)?] = true;
                continue;
            }
            CheckTarget::Alternatives(alternatives) => alternatives,
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
pub(in crate::lowering) struct PatternBinding {
    pub lisette_name: String,
    pub binding_ids: Vec<BindingId>,
    pub target: BindingTarget,
    pub path: AccessPath,
    pub coercion: CoercionPlan,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::lowering) enum BindingTarget {
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

    pub(in crate::lowering) fn go_name(&self) -> Option<&SourceName> {
        match self {
            Self::Named { go_name, .. } => Some(go_name),
            Self::Unused | Self::Unit => None,
        }
    }

    pub(in crate::lowering) fn is_named(&self) -> bool {
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

pub(in crate::lowering) struct PatternInfo {
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

    pub(in crate::lowering) fn is_irrefutable(&self) -> bool {
        self.checks.is_empty()
    }

    pub(in crate::lowering) fn has_root_assertion(&self) -> bool {
        root_assertion_position(&self.checks).is_some()
    }
}

#[derive(Debug)]
pub(in crate::lowering) struct ArmLeaf {
    pub arm_index: usize,
    pub bindings: Vec<PatternBinding>,
}

#[derive(Debug)]
pub(in crate::lowering) struct GuardedLeaf {
    pub leaf: ArmLeaf,
    pub failure: Box<Decision>,
}

#[derive(Debug)]
pub(in crate::lowering) enum Decision {
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
    pub(in crate::lowering) fn first_arm(&self) -> Option<usize> {
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

    pub(in crate::lowering) fn has_unguarded_terminal(&self) -> bool {
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
pub(in crate::lowering) enum ChainTail {
    Catchall(ArmLeaf),
    LastTest,
    Open,
}

#[derive(Debug)]
pub(in crate::lowering) struct ValueSwitch {
    pub path: AccessPath,
    pub kind: SwitchKind,
    pub branches: Vec<SwitchBranch<GoExpression>>,
    pub fallback: Option<Box<Decision>>,
}

impl ValueSwitch {
    pub(in crate::lowering) fn shape(&self) -> SwitchShape<'_> {
        match (&self.kind, self.branches.as_slice(), &self.fallback) {
            (SwitchKind::Value, [left, right], None) => {
                match (boolean_literal(&left.label), boolean_literal(&right.label)) {
                    (Some(true), Some(false)) => SwitchShape::Bool {
                        true_branch: left,
                        false_branch: right,
                    },
                    (Some(false), Some(true)) => SwitchShape::Bool {
                        true_branch: right,
                        false_branch: left,
                    },
                    _ => SwitchShape::Binary {
                        first: left,
                        second: right,
                    },
                }
            }
            (_, [first, second], None) => SwitchShape::Binary { first, second },
            (_, [branch], _) => SwitchShape::SingleArm(branch),
            _ => SwitchShape::Multi,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::lowering) enum SwitchKind {
    EnumTag,
    Value,
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum SwitchShape<'s> {
    Bool {
        true_branch: &'s SwitchBranch<GoExpression>,
        false_branch: &'s SwitchBranch<GoExpression>,
    },
    Binary {
        first: &'s SwitchBranch<GoExpression>,
        second: &'s SwitchBranch<GoExpression>,
    },
    SingleArm(&'s SwitchBranch<GoExpression>),
    Multi,
}

pub(in crate::lowering) fn boolean_literal(expression: &GoExpression) -> Option<bool> {
    match expression.node() {
        GoExpressionNode::Literal(GoLiteral::Bool(value)) => Some(*value),
        _ => None,
    }
}

#[derive(Debug)]
pub(in crate::lowering) struct SwitchBranch<L> {
    pub label: L,
    pub extra_labels: Vec<L>,
    pub decision: Decision,
}

#[derive(Debug)]
pub(in crate::lowering) struct ChainTest {
    pub checks: Vec<Check>,
    pub arm: ChainArm,
}

#[derive(Debug)]
pub(in crate::lowering) enum ChainArm {
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

    fn interface_or_with_extras(&self) -> Option<&[Vec<Check>]> {
        let [Check::Or { alternatives }] = self.checks.as_slice() else {
            return None;
        };
        let with_extras = alternatives
            .iter()
            .all(|alternative| matches!(alternative.first(), Some(Check::TypeAssert { .. })))
            && alternatives.iter().any(|alternative| alternative.len() > 1);
        with_extras.then_some(alternatives.as_slice())
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
        if !fallback_arms
            .iter()
            .all(|a| a.is_catchall() || a.has_root_assertion())
        {
            return None;
        }
        let cases = cases
            .iter()
            .map(|arm| {
                let mut arm = arm.clone();
                let label = lift_root_assertion(&mut arm.checks, &mut arm.bindings)?;
                Some(LabeledCase {
                    label,
                    extra_labels: Vec::new(),
                    arm,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let (branches, fallback) = build_switch_branches(cases, fallback_arms);

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

    let cases = cases
        .iter()
        .map(|arm| value_switch_case(arm, &kind))
        .collect::<Option<Vec<_>>>()?;
    let (branches, fallback) = build_switch_branches(cases, fallback_arms);
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

struct LabeledCase<L> {
    label: L,
    extra_labels: Vec<L>,
    arm: ArmInfo,
}

fn value_switch_case(arm: &ArmInfo, kind: &SwitchKind) -> Option<LabeledCase<GoExpression>> {
    let (first, rest) = arm.checks.split_first()?;
    let (_, labels) = first.switch_labels(kind)?;
    let (label, extra_labels) = labels
        .split_first()
        .expect("an or-check has at least one alternative");
    Some(LabeledCase {
        label: (*label).clone(),
        extra_labels: extra_labels.iter().map(|label| (*label).clone()).collect(),
        arm: ArmInfo {
            checks: rest.to_vec(),
            ..arm.clone()
        },
    })
}

fn build_switch_branches<L: PartialEq>(
    cases: Vec<LabeledCase<L>>,
    fallback_arms: &[ArmInfo],
) -> (Vec<SwitchBranch<L>>, Option<Box<Decision>>) {
    let mut grouped: Vec<(L, Vec<L>, Vec<ArmInfo>)> = Vec::new();

    for case in cases {
        let LabeledCase {
            label: case_label,
            extra_labels,
            arm: inner_arm,
        } = case;

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
    if !arm_infos
        .iter()
        .any(|arm| arm.interface_or_with_extras().is_some())
    {
        return arm_infos;
    }
    let mut result = Vec::with_capacity(arm_infos.len());
    for arm in arm_infos {
        match arm.interface_or_with_extras() {
            Some(alternatives) => {
                for alternative in alternatives {
                    result.push(ArmInfo {
                        arm_index: arm.arm_index,
                        checks: alternative.clone(),
                        bindings: arm.bindings.clone(),
                        has_guard: arm.has_guard,
                    });
                }
            }
            None => result.push(arm),
        }
    }
    result
}

pub(super) fn share_alternative_bindings(alternatives: &mut [&mut Vec<PatternBinding>]) {
    let mut shared_by_name: FxHashMap<String, (Vec<BindingId>, BindingTarget)> =
        FxHashMap::default();
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

enum RootAssertion<'a> {
    Single(&'a GoType),
    Alternatives(Vec<&'a GoType>),
}

fn root_asserted_type(check: &Check) -> Option<&GoType> {
    match check {
        Check::TypeAssert { path, go_type } if path.is_root() => Some(go_type),
        _ => None,
    }
}

fn root_assertion(check: &Check) -> Option<RootAssertion<'_>> {
    match check {
        Check::Or { alternatives } => alternatives
            .iter()
            .map(|alternative| match alternative.as_slice() {
                [check] => root_asserted_type(check),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(RootAssertion::Alternatives),
        _ => root_asserted_type(check).map(RootAssertion::Single),
    }
}

fn root_assertion_position(checks: &[Check]) -> Option<usize> {
    checks
        .iter()
        .position(|check| root_assertion(check).is_some())
}

pub(super) fn lift_root_assertion(
    checks: &mut Vec<Check>,
    bindings: &mut [PatternBinding],
) -> Option<Vec<GoType>> {
    let (position, assertion) = checks
        .iter()
        .enumerate()
        .find_map(|(position, check)| Some((position, root_assertion(check)?)))?;
    let (go_types, asserted) = match assertion {
        RootAssertion::Single(go_type) => (
            vec![go_type.clone()],
            Some(PathSegment::AssertedAs(go_type.clone())),
        ),
        RootAssertion::Alternatives(go_types) => {
            debug_assert!(!go_types.is_empty(), "at least one alternative");
            (go_types.into_iter().cloned().collect(), None)
        }
    };
    checks.remove(position);
    if let Some(asserted) = asserted {
        for check in checks.iter_mut() {
            check.rebase_past(&asserted);
        }
        for binding in bindings {
            binding.path.rebase_past(&asserted);
        }
    }
    Some(go_types)
}

pub(super) fn render_condition(
    scope: &mut ScopeState,
    checks: &[Check],
    subject_var: SubjectRoot<'_>,
) -> GoExpression {
    checks
        .iter()
        .map(|check| check.render(scope, subject_var))
        .reduce(|left, right| GoExpression::binary(left, GoBinaryOperator::And, right))
        .unwrap_or_else(|| GoExpression::boolean(true))
}

impl Planner<'_> {
    pub(super) fn compile_match_arms(&self, arms: &[MatchArm], subject_ty: &Type) -> Decision {
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

    pub(in crate::lowering) fn collect_pattern_info(
        &self,
        pattern: &Pattern,
        subject_ty: &Type,
    ) -> PatternInfo {
        let mut info = PatternInfo::new();
        self.collect_checks_and_bindings(
            &AccessPath::root(),
            pattern,
            subject_ty,
            Some(&self.facts.value_layout(subject_ty, SlotOrigin::Lisette)),
            &mut info,
        );
        info
    }
}

fn field_segment(facts: &EmitFacts<'_>, receiver_ty: &Type, name: String) -> PathSegment {
    if facts.field_read_cannot_panic(receiver_ty) {
        PathSegment::ValueField(name)
    } else {
        PathSegment::Field(name)
    }
}
