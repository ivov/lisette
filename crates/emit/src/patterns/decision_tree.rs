use crate::patterns::binding_decls::pattern_has_bindings;
use rustc_hash::FxHashMap as HashMap;
use std::slice;

use syntax::ast::{
    BindingId, ConstructorPatternResolution, EnumFieldDefinition, MatchArm, Pattern,
    RecordPatternResolution, RestPattern, SequencePatternResolution, StructFieldPattern,
};
use syntax::parse::TUPLE_FIELDS;
use syntax::program::DefinitionBody;
use syntax::types::{Type, unqualified_name};

use crate::Planner;
use crate::control_flow::propagation::plain_return;
use crate::names::generics;
use crate::names::packages::PackageRequirements;
use crate::patterns::binding_decls::emit_pattern_literal;
use crate::plan::bodies::{LoweredBlock, define_many};
use crate::plan::go_expression::{BinaryOp, FunctionLiteralLayout, GoExpressionNode, UnaryOp};
use crate::plan::values::GoExpression;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathSegment {
    /// `.FieldName` (Go name, already resolved).
    Field(String),
    ValueField(String),
    Index(usize),
    /// `[offset:]`.
    SliceFrom(usize),
    ArraySliceFrom {
        offset: usize,
        go_type: String,
    },
    /// `(*expression)`: auto-pointer deref for recursive enum fields.
    Deref,
    /// `GoType(expression)`: newtype cast to underlying Go type.
    NewtypeCast(String),
    /// `expression.(GoType)`: Go interface type assertion, inserted when a
    /// concrete pattern targets a Go interface.
    AssertedAs(String),
}

fn element_index(segments: &[PathSegment]) -> usize {
    let [
        PathSegment::Field(field) | PathSegment::ValueField(field),
        ..,
    ] = segments
    else {
        unreachable!("a tuple-element subject only resolves field paths")
    };
    TUPLE_FIELDS
        .iter()
        .position(|tuple_field| tuple_field == field)
        .expect("a tuple-element subject only resolves tuple fields")
}

fn checked_tuple_element(path: &AccessPath, arity: usize) -> Option<usize> {
    let Some(PathSegment::Field(field) | PathSegment::ValueField(field)) = path.segments.first()
    else {
        return None;
    };
    TUPLE_FIELDS
        .iter()
        .take(arity)
        .position(|tuple_field| tuple_field == field)
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
                }
                PathSegment::SliceFrom(offset) => GoExpression::slice(
                    result,
                    Some(&GoExpression::literal(offset.to_string())),
                    None,
                    None,
                ),
                PathSegment::ArraySliceFrom { offset, go_type } => GoExpression::conversion(
                    go_type.clone(),
                    GoExpression::slice(
                        result,
                        Some(&GoExpression::literal(offset.to_string())),
                        None,
                        None,
                    ),
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
}

fn field_segment(planner: &Planner<'_>, receiver_ty: &Type, name: String) -> PathSegment {
    if planner.field_read_cannot_panic(receiver_ty) {
        PathSegment::ValueField(name)
    } else {
        PathSegment::Field(name)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Check {
    EnumTag {
        path: AccessPath,
        tag_constant: GoExpression,
    },
    Literal {
        path: AccessPath,
        go_literal: GoExpression,
    },
    SliceLenEq {
        path: AccessPath,
        length: usize,
    },
    SliceLenGe {
        path: AccessPath,
        min_length: usize,
    },
    /// At least one alternative's inner checks must all pass.
    Or {
        alternatives: Vec<Vec<Check>>,
    },
    /// Go interface type assertion; emitted as a case label in
    /// `switch x := x.(type)`.
    TypeAssert {
        path: AccessPath,
        go_type: String,
    },
}

#[derive(Clone, Copy)]
enum CheckPolarity {
    Positive,
    Negative,
}

impl Check {
    pub(crate) fn render(&self, subject: SubjectRoot<'_>) -> GoExpression {
        self.render_with_polarity(subject, CheckPolarity::Positive)
    }

    pub(crate) fn render_negated(&self, subject: SubjectRoot<'_>) -> GoExpression {
        self.render_with_polarity(subject, CheckPolarity::Negative)
    }

    fn render_with_polarity(
        &self,
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
            Check::EnumTag { path, tag_constant } => {
                let operator = if negative { BinaryOp::Ne } else { BinaryOp::Eq };
                GoExpression::binary(
                    GoExpression::selector(path.render(subject), "Tag".to_string()),
                    operator,
                    tag_constant.clone(),
                )
            }
            Check::Literal { path, go_literal } => {
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
                .map(|checks| join_conditions(checks, subject))
                .reduce(|left, right| GoExpression::binary(left, BinaryOp::Or, right))
                .expect("an or-check has at least one alternative"),
            Check::TypeAssert { path, go_type } if !negative => GoExpression::immediate_call(
                "bool".to_string(),
                LoweredBlock {
                    statements: vec![
                        define_many(
                            vec!["_".to_string(), "ok".to_string()],
                            GoExpression::type_assertion(path.render(subject), go_type.clone()),
                        ),
                        plain_return(GoExpression::name("ok".to_string())),
                    ],
                },
                FunctionLiteralLayout::Inline,
            ),
            Check::Or { .. } | Check::TypeAssert { .. } => GoExpression::unary(
                UnaryOp::Not,
                self.render_with_polarity(subject, CheckPolarity::Positive),
            ),
        }
    }

    fn rebase_past(&mut self, segment: &PathSegment) {
        match self {
            Check::EnumTag { path, .. }
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
            Check::EnumTag { path, .. }
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
}

pub(crate) fn tested_tuple_elements(checks: &[Check], arity: usize) -> Option<Vec<bool>> {
    let mut tested = vec![false; arity];
    for check in checks {
        if let Some(path) = check.path() {
            tested[checked_tuple_element(path, arity)?] = true;
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
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BindingTarget {
    Unused,
    Unit,
    Named { go_name: String, ty: Type },
}

impl BindingTarget {
    fn from_go_name(go_name: Option<String>, ty: &Type) -> Self {
        go_name.map_or(Self::Unused, |go_name| Self::Named {
            go_name,
            ty: ty.clone(),
        })
    }

    pub(crate) fn go_name(&self) -> Option<&str> {
        match self {
            Self::Named { go_name, .. } => Some(go_name),
            Self::Unused | Self::Unit => None,
        }
    }

    pub(crate) fn is_named(&self) -> bool {
        matches!(self, Self::Named { .. })
    }

    /// `Unit` wins over `Unused` since its inline `struct{}{}` serves read and unread names.
    pub(super) fn shared_with(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unit, _) | (_, Self::Unit) => Self::Unit,
            (Self::Unused, _) | (_, Self::Unused) => Self::Unused,
            (Self::Named { .. }, Self::Named { .. }) => self.clone(),
        }
    }
}

/// A root type assertion stays among the checks until a consumer lifts it.
pub(crate) struct PatternInfo {
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
    pub packages: PackageRequirements,
    pub requires_materialized_subject: bool,
}

impl PatternInfo {
    fn new() -> Self {
        Self {
            checks: Vec::new(),
            bindings: Vec::new(),
            packages: PackageRequirements::default(),
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

/// A pre-computed decision tree for pattern matching.
///
#[derive(Debug)]
pub(crate) enum Decision {
    Success(ArmLeaf),
    Guard(GuardedLeaf),
    Switch(ValueSwitch),
    TypeSwitch {
        branches: Vec<SwitchBranch<Vec<String>>>,
        fallback: Option<Box<Decision>>,
    },
    /// `catchall` is the unguarded arm that ends the chain.
    Chain {
        tests: Vec<ChainTest>,
        catchall: Option<ArmLeaf>,
    },
    /// Emits `panic("unreachable")` in tail position.
    Unreachable,
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
    /// Two branches, no fallback, not `Bool`.
    Binary,
    SingleArm,
    Multi,
}

/// True when the tree has a terminal Success reachable without passing a guard.
pub(crate) fn tree_has_unguarded_terminal(tree: &Decision) -> bool {
    match tree {
        Decision::Success(_) => true,
        Decision::Guard(guard) => tree_has_unguarded_terminal(&guard.failure),
        Decision::Chain { tests, catchall } => {
            catchall.is_some()
                || tests.last().is_some_and(|t| match &t.arm {
                    ChainArm::Leaf(_) => true,
                    ChainArm::Guard(guard) => tree_has_unguarded_terminal(&guard.failure),
                })
        }
        Decision::Switch(ValueSwitch { fallback, .. }) | Decision::TypeSwitch { fallback, .. } => {
            fallback.as_deref().is_none_or(tree_has_unguarded_terminal)
        }
        Decision::Unreachable => false,
    }
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
    /// Root type assertion still in place, only the type switch lifts it.
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
}

/// Build a Decision tree from a list of arm infos.
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

/// The arms from the first catchall on form the fallback.
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
            lift_root_assertion(&mut arm.checks, &mut arm.bindings).unwrap()
        });
        return Some(Decision::TypeSwitch { branches, fallback });
    }

    let first_check = first_relevant.checks.first()?;
    let path = first_check.path()?.clone();
    let kind = if first_check.as_enum_tag().is_some() {
        SwitchKind::EnumTag
    } else if first_check.as_literal().is_some() {
        SwitchKind::Value
    } else {
        return None;
    };
    validate_value_switch_arms(arms, &kind, &path)?;

    let (branches, fallback) = build_switch_branches(cases, fallback_arms, |arm| {
        let first = arm.checks.remove(0);
        match kind {
            SwitchKind::EnumTag => first.as_enum_tag(),
            SwitchKind::Value => first.as_literal(),
        }
        .cloned()
        .unwrap()
    });
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
    for arm in arms {
        if arm.is_catchall() {
            continue;
        }
        if arm.has_guard {
            return None;
        }

        let first = arm.checks.first()?;
        match kind {
            SwitchKind::EnumTag => {
                first.as_enum_tag()?;
            }
            SwitchKind::Value => {
                first.as_literal()?;
                if arm.checks.len() != 1 {
                    return None;
                }
            }
        }
        if first.path()? != switch_path {
            return None;
        }
    }
    Some(())
}

/// Go `switch` cases do not fall through to `default`, so fallback arms are spliced into fail-prone case bodies.
fn build_switch_branches<L: PartialEq>(
    cases: &[ArmInfo],
    fallback_arms: &[ArmInfo],
    take_label: impl Fn(&mut ArmInfo) -> L,
) -> (Vec<SwitchBranch<L>>, Option<Box<Decision>>) {
    let mut grouped: Vec<(L, Vec<ArmInfo>)> = Vec::new();

    for arm in cases {
        let mut inner_arm = arm.clone();
        let case_label = take_label(&mut inner_arm);

        if let Some((_, arms)) = grouped.iter_mut().find(|(label, _)| label == &case_label) {
            arms.push(inner_arm);
        } else {
            grouped.push((case_label, vec![inner_arm]));
        }
    }

    let branches = grouped
        .into_iter()
        .map(|(label, inner_arms)| {
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
            SwitchBranch { label, decision }
        })
        .collect();

    let fallback = if fallback_arms.is_empty() {
        None
    } else {
        Some(Box::new(build_tree(fallback_arms.to_vec())))
    };
    (branches, fallback)
}

/// Build a Chain (if/else if/else) from the arms.
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
                    catchall: Some(catchall),
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

    Decision::Chain {
        tests,
        catchall: None,
    }
}

/// Recursively walk a pattern, collecting checks and bindings.
///
/// `path_ty` is the expected type of the value at `path`, used to detect
/// when a struct pattern is matched against a Go interface (type switch).
fn collect_checks_and_bindings(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Unit { .. } => {}

        Pattern::Identifier {
            identifier,
            binding,
            ..
        } => {
            let go_name = planner.go_name_for_binding(pattern);
            collector.bindings.push(PatternBinding {
                lisette_name: identifier.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, path_ty),
                path: path.clone(),
            });
        }

        Pattern::Literal { literal, .. } => {
            collector.checks.push(Check::Literal {
                path: path.clone(),
                go_literal: GoExpression::literal(emit_pattern_literal(literal)),
            });
        }

        Pattern::EnumVariant { resolution, ty, .. } => {
            let can_split_tuple =
                matches!(
                    resolution,
                    ConstructorPatternResolution::Const { .. }
                        | ConstructorPatternResolution::ConstValue { .. }
                ) || matches!(resolution, ConstructorPatternResolution::EnumVariant { .. })
                    && !planner.is_tuple_struct_type(ty);
            collector.requires_materialized_subject |= !can_split_tuple;
            collect_enum_variant_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Struct { .. } => {
            collector.requires_materialized_subject = true;
            collect_struct_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Tuple { elements, .. } => {
            collect_tuple_checks(planner, path, elements, path_ty, collector);
        }

        Pattern::Slice { .. } => {
            collector.requires_materialized_subject = true;
            collect_slice_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Or { patterns, .. } => {
            collect_or_pattern_checks(planner, path, patterns, pattern, path_ty, collector);
        }

        p @ Pattern::AsBinding {
            pattern: inner,
            name,
            binding,
            ..
        } => {
            collect_checks_and_bindings(planner, path, inner, path_ty, collector);
            let go_name = planner.go_name_for_binding(p);
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                binding_ids: binding.as_slice().to_vec(),
                target: BindingTarget::from_go_name(go_name, path_ty),
                path: path.clone(),
            });
        }
    }
}

fn collect_tuple_checks(
    planner: &Planner,
    path: &AccessPath,
    elements: &[Pattern],
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let Type::Tuple(element_tys) = planner.facts.strip_and_peel(path_ty) else {
        unreachable!("tuple pattern on a non-tuple type: {path_ty:?}");
    };

    for (i, element) in elements.iter().enumerate() {
        let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
        let field_path = path.push(field_segment(planner, path_ty, field_name.to_string()));
        collect_checks_and_bindings(planner, &field_path, element, &element_tys[i], collector);
    }
}

fn collect_slice_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let Pattern::Slice {
        prefix,
        rest,
        resolution,
        ..
    } = pattern
    else {
        return;
    };
    let array_info = match resolution {
        SequencePatternResolution::Array {
            length,
            element_type,
        } => Some((*length, element_type)),
        _ => None,
    };

    if array_info.is_none() {
        if !rest.is_present() {
            collector.checks.push(Check::SliceLenEq {
                path: path.clone(),
                length: prefix.len(),
            });
        } else if !prefix.is_empty() {
            collector.checks.push(Check::SliceLenGe {
                path: path.clone(),
                min_length: prefix.len(),
            });
        }
    }

    let element_type = match resolution {
        SequencePatternResolution::Slice { element_type }
        | SequencePatternResolution::Array { element_type, .. } => element_type,
        SequencePatternResolution::Unresolved => {
            unreachable!("slice pattern without a resolved element type")
        }
    };

    for (i, element) in prefix.iter().enumerate() {
        let element_path = path.push(PathSegment::Index(i));
        collect_checks_and_bindings(planner, &element_path, element, element_type, collector);
    }

    if let RestPattern::Bind { name, binding, .. } = rest {
        let go_name = planner.go_name_for_rest_binding(rest);
        let (segment, rest_ty) = match &array_info {
            Some((length, element_type)) => {
                let sub_length = length.saturating_sub(prefix.len() as u64);
                let sub_ty = Type::Array {
                    length: sub_length,
                    element: Box::new((*element_type).clone()),
                };
                let go_type = planner.go_type(&sub_ty);
                collector.packages.extend(go_type.requirements());
                (
                    PathSegment::ArraySliceFrom {
                        offset: prefix.len(),
                        go_type: go_type.code,
                    },
                    sub_ty,
                )
            }
            None => (PathSegment::SliceFrom(prefix.len()), path_ty.clone()),
        };
        collector.bindings.push(PatternBinding {
            lisette_name: name.to_string(),
            binding_ids: binding.as_slice().to_vec(),
            target: BindingTarget::from_go_name(go_name, &rest_ty),
            path: path.push(segment),
        });
    }
}

/// Handle or-patterns without bindings by collecting conditions from each
/// alternative and combining with `||`.
fn collect_or_pattern_checks(
    planner: &Planner,
    path: &AccessPath,
    patterns: &[Pattern],
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let has_bindings = pattern_has_bindings(pattern);
    if !has_bindings {
        let alt_collectors: Vec<PatternInfo> = patterns
            .iter()
            .map(|p| {
                let mut alt_collector = PatternInfo::new();
                collect_checks_and_bindings(planner, path, p, path_ty, &mut alt_collector);
                alt_collector
            })
            .collect();

        if alt_collectors.iter().any(|c| c.checks.is_empty()) {
            return;
        }

        for alt in &alt_collectors {
            collector.packages.extend(&alt.packages);
        }
        collector.checks.push(Check::Or {
            alternatives: alt_collectors.into_iter().map(|c| c.checks).collect(),
        });
    }
}

/// Compute the access path for a struct field, handling enum struct variants
/// and auto-pointer dereference.
fn compute_struct_field_path(
    planner: &Planner,
    parent_path: &AccessPath,
    field: &StructFieldPattern,
    ty: &Type,
    enum_info: Option<&(String, String)>,
) -> AccessPath {
    let go_field_name = if let Some((enum_id, variant_name)) = enum_info {
        planner
            .enum_struct_field_name(enum_id, variant_name, &field.name)
            .unwrap_or_else(|| {
                panic!(
                    "enum layout not found: {}.{}.{}",
                    enum_id, variant_name, field.name
                )
            })
    } else {
        planner.struct_field_go_name(ty, &field.name, false)
    };

    if let Some((_, variant_name)) = enum_info
        && let Some(field_index) =
            planner.get_enum_struct_field_index(ty, variant_name, &field.name)
        && planner.is_enum_field_recursive(ty, variant_name, field_index)
    {
        return parent_path
            .push(field_segment(planner, ty, go_field_name))
            .push(PathSegment::Deref);
    }

    parent_path.push(field_segment(planner, ty, go_field_name))
}

/// When a concrete pattern targets a Go-interface scrutinee, push a TypeAssert
/// check and return the child path through `AssertedAs`.
fn interface_assert_child_path(
    planner: &Planner,
    path: &AccessPath,
    pattern_ty: &Type,
    path_ty: &Type,
    collector: &mut PatternInfo,
) -> Option<AccessPath> {
    planner.facts.as_interface(path_ty)?;
    let go_type_result = planner.go_type(pattern_ty);
    collector.packages.extend(go_type_result.requirements());
    let go_type = go_type_result.code;
    collector.checks.push(Check::TypeAssert {
        path: path.clone(),
        go_type: go_type.clone(),
    });
    Some(path.push(PathSegment::AssertedAs(go_type)))
}

/// Collect checks and bindings for an enum variant pattern (tuple or tagged).
fn collect_enum_variant_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let Pattern::EnumVariant {
        identifier,
        fields,
        resolution,
        ty,
        ..
    } = pattern
    else {
        return;
    };

    // A const pattern is a value comparison against a named constant, emitted
    // as a Go `case` expression rather than an enum tag or newtype destructure.
    if let ConstructorPatternResolution::Const { qualified_name }
    | ConstructorPatternResolution::ConstValue { qualified_name, .. } = resolution
    {
        collect_const_pattern_check(planner, path, qualified_name, collector);
        return;
    }

    let ConstructorPatternResolution::EnumVariant {
        enum_name,
        variant_name,
    } = resolution
    else {
        return;
    };
    let params = pattern_type_args(planner, ty);
    let Some(definition) = planner.facts.definition(enum_name) else {
        return;
    };
    let field_types: Vec<Type> = match &definition.body {
        DefinitionBody::Struct {
            fields, generics, ..
        } => fields
            .iter()
            .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
            .collect(),
        DefinitionBody::Enum {
            variants, generics, ..
        } => {
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
            else {
                return;
            };
            variant
                .fields
                .iter()
                .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
                .collect()
        }
        _ => return,
    };

    let variant_data = EnumVariantData {
        identifier,
        fields,
        ty,
        field_types: &field_types,
    };

    if planner.is_tuple_struct_type(ty) {
        let child_path = interface_assert_child_path(planner, path, ty, path_ty, collector)
            .unwrap_or_else(|| path.clone());
        if planner.is_newtype_struct(ty) {
            collect_newtype_checks(planner, &child_path, &variant_data, collector);
        } else {
            collect_tuple_struct_checks(planner, &child_path, fields, &field_types, collector);
        }
        return;
    }

    if handle_foreign_variant_literal(planner, path, ty, identifier, collector) {
        return;
    }

    collect_tagged_enum_checks(planner, path, &variant_data, collector);
}

/// Emit a const pattern as a Go `case` constant (e.g. `time.Friday`).
fn collect_const_pattern_check(
    planner: &Planner,
    path: &AccessPath,
    qualified_name: &str,
    collector: &mut PatternInfo,
) {
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal: planner.definition_reference(qualified_name),
    });
}

/// `true` when the variant is a foreign-package dotted name (e.g.
/// `httpkg.MethodGet`) emitted as a `Check::Literal`.
fn handle_foreign_variant_literal(
    planner: &Planner,
    path: &AccessPath,
    ty: &Type,
    identifier: &str,
    collector: &mut PatternInfo,
) -> bool {
    if planner.as_enum(ty).is_some() || !identifier.contains('.') {
        return false;
    }
    let go_literal = match identifier.split_once('.') {
        Some((package, member)) if planner.facts.is_foreign_package(package) => {
            GoExpression::qualified(
                planner.package_use_for_package(&planner.canonical_package(package)),
                member,
            )
        }
        _ => GoExpression::name(identifier.to_string()),
    };
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal,
    });
    true
}

/// Collect checks and bindings for a newtype struct pattern (single-field wrapper).
fn collect_newtype_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternInfo,
) {
    let Some(underlying_ty) = planner.get_newtype_underlying(variant.ty) else {
        return;
    };
    let go_underlying = planner.go_type(&underlying_ty);
    collector.packages.extend(go_underlying.requirements());
    let field_path = path.push(PathSegment::NewtypeCast(go_underlying.code));
    if let Some(field) = variant.fields.first() {
        collect_checks_and_bindings(
            planner,
            &field_path,
            field,
            &variant.field_types[0],
            collector,
        );
    }
}

/// Collect checks and bindings for a tuple struct pattern (positional fields).
fn collect_tuple_struct_checks(
    planner: &Planner,
    path: &AccessPath,
    fields: &[Pattern],
    field_types: &[Type],
    collector: &mut PatternInfo,
) {
    for (i, field) in fields.iter().enumerate() {
        let field_path = path.push(PathSegment::Field(format!("F{}", i)));
        collect_checks_and_bindings(planner, &field_path, field, &field_types[i], collector);
    }
}

struct EnumVariantData<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    field_types: &'a [Type],
}

fn enum_tag_constant(planner: &Planner, identifier: &str, ty: &Type) -> GoExpression {
    match ty {
        Type::Nominal { id, .. } => planner.resolve_variant(identifier, id),
        _ => GoExpression::name(identifier.replace('.', "_")),
    }
}

/// Collect checks and bindings for a tagged enum variant pattern.
fn collect_tagged_enum_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternInfo,
) {
    collector.checks.push(Check::EnumTag {
        path: path.clone(),
        tag_constant: enum_tag_constant(planner, variant.identifier, variant.ty),
    });

    let variant_name = variant
        .identifier
        .split('.')
        .next_back()
        .unwrap_or(variant.identifier);
    for (i, field) in variant.fields.iter().enumerate() {
        let field_name = planner.get_enum_tuple_field_name(variant.ty, variant_name, i);

        let is_unit = planner.is_enum_field_unit(variant.ty, variant_name, i);

        let field_path = if planner.is_enum_field_recursive(variant.ty, variant_name, i) {
            path.push(field_segment(planner, variant.ty, field_name))
                .push(PathSegment::Deref)
        } else {
            path.push(field_segment(planner, variant.ty, field_name))
        };

        if is_unit {
            if let Pattern::Identifier {
                identifier,
                binding,
                ..
            } = field
            {
                collector.bindings.push(PatternBinding {
                    lisette_name: identifier.to_string(),
                    binding_ids: binding.as_slice().to_vec(),
                    target: BindingTarget::Unit,
                    path: field_path,
                });
            }
        } else {
            collect_checks_and_bindings(
                planner,
                &field_path,
                field,
                &variant.field_types[i],
                collector,
            );
        }
    }
}

/// Collect checks and bindings for a struct pattern (plain struct or enum struct variant).
/// Detect whether a struct pattern is actually an enum struct variant,
/// returning `(enum_id, variant_name)` if so.
fn detect_enum_info(resolution: &RecordPatternResolution) -> Option<(String, String)> {
    match resolution {
        RecordPatternResolution::EnumVariant {
            enum_name,
            variant_name,
            ..
        } => Some((
            enum_name.to_string(),
            unqualified_name(variant_name).to_string(),
        )),
        RecordPatternResolution::Struct { .. } | RecordPatternResolution::Unresolved => None,
    }
}

fn collect_struct_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) {
    let Pattern::Struct {
        fields,
        ty,
        resolution,
        ..
    } = pattern
    else {
        return;
    };

    let (enum_info, child_path) =
        resolve_struct_child_path(planner, path, pattern, path_ty, collector);

    for field in fields {
        let field_path =
            compute_struct_field_path(planner, &child_path, field, ty, enum_info.as_ref());
        let field_ty = record_field_type(planner, resolution, ty, &field.name)
            .unwrap_or_else(|| panic!("record pattern field not resolved: {}", field.name));
        collect_checks_and_bindings(planner, &field_path, &field.value, &field_ty, collector);
    }
}

/// Resolve the access path for struct-pattern field lookups. Returns the
/// enum-variant identity (when the pattern is an enum-struct variant) and the
/// child path used for field projection: either the interface-assertion alias
/// or the input path. Pushes the variant's tag check into `collector` when
/// applicable.
fn resolve_struct_child_path(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: &Type,
    collector: &mut PatternInfo,
) -> (Option<(String, String)>, AccessPath) {
    let Pattern::Struct {
        ty,
        identifier,
        resolution,
        ..
    } = pattern
    else {
        unreachable!("resolve_struct_child_path requires a Struct pattern");
    };
    if let Some(asserted) = interface_assert_child_path(planner, path, ty, path_ty, collector) {
        return (None, asserted);
    }
    let enum_info = detect_enum_info(resolution);
    if enum_info.is_some() {
        collector.checks.push(Check::EnumTag {
            path: path.clone(),
            tag_constant: enum_tag_constant(planner, identifier, ty),
        });
    }
    (enum_info, path.clone())
}

fn pattern_type_args(planner: &Planner, ty: &Type) -> Vec<Type> {
    match planner.facts.peel_alias(ty) {
        Type::Nominal { params, .. } => params,
        _ => vec![],
    }
}

fn record_variant_fields<'a>(
    planner: &'a Planner,
    resolution: &RecordPatternResolution,
) -> Option<&'a [EnumFieldDefinition]> {
    let RecordPatternResolution::EnumVariant {
        enum_name,
        variant_name,
    } = resolution
    else {
        return None;
    };
    let DefinitionBody::Enum { variants, .. } = &planner.facts.definition(enum_name)?.body else {
        return None;
    };
    variants
        .iter()
        .find(|variant| variant.name == unqualified_name(variant_name))
        .map(|variant| variant.fields.as_slice())
}

fn record_field_type(
    planner: &Planner,
    resolution: &RecordPatternResolution,
    pattern_ty: &Type,
    field_name: &str,
) -> Option<Type> {
    let params = pattern_type_args(planner, pattern_ty);
    match resolution {
        RecordPatternResolution::Struct { struct_name } => {
            let DefinitionBody::Struct {
                fields, generics, ..
            } = &planner.facts.definition(struct_name)?.body
            else {
                return None;
            };
            fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
        }
        RecordPatternResolution::EnumVariant { .. } => {
            let fields = record_variant_fields(planner, resolution)?;
            let RecordPatternResolution::EnumVariant { enum_name, .. } = resolution else {
                unreachable!()
            };
            let DefinitionBody::Enum { generics, .. } = &planner.facts.definition(enum_name)?.body
            else {
                return None;
            };
            fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
        }
        RecordPatternResolution::Unresolved => None,
    }
}

fn arm_is_interface_or_with_extras(arm: &ArmInfo) -> bool {
    if arm.checks.len() != 1 {
        return false;
    }
    let Check::Or { alternatives } = &arm.checks[0] else {
        return false;
    };
    alternatives
        .iter()
        .all(|alt| matches!(alt.first(), Some(Check::TypeAssert { .. })))
        && alternatives.iter().any(|alt| alt.len() > 1)
}

fn expand_interface_or_checks(arm_infos: Vec<ArmInfo>) -> Vec<ArmInfo> {
    if !arm_infos.iter().any(arm_is_interface_or_with_extras) {
        return arm_infos;
    }
    let mut result = Vec::with_capacity(arm_infos.len());
    for arm in arm_infos {
        if arm_is_interface_or_with_extras(&arm) {
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

/// Or-patterns without bindings are handled inline by the condition collector.
pub(super) fn compile_match_arms(
    planner: &mut Planner,
    arms: &[MatchArm],
    subject_ty: &Type,
) -> Decision {
    let mut arm_infos = Vec::new();
    for (arm_index, arm) in arms.iter().enumerate() {
        let alternatives = match &arm.pattern {
            Pattern::Or { patterns, .. } if pattern_has_bindings(&arm.pattern) => {
                patterns.as_slice()
            }
            pattern => slice::from_ref(pattern),
        };
        for alternative in alternatives {
            let info = collect_pattern_info(planner, alternative, subject_ty);
            planner.require_packages(&info.packages);
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

/// Make or-pattern alternatives agree on each bound name's ids and target.
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

/// Takes `subject_ty` by reference so no site can miss root-level Go-interface assertions.
pub(crate) fn collect_pattern_info(
    planner: &Planner,
    pattern: &Pattern,
    subject_ty: &Type,
) -> PatternInfo {
    let mut info = PatternInfo::new();
    collect_checks_and_bindings(planner, &AccessPath::root(), pattern, subject_ty, &mut info);
    info
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

/// Move a root type assertion out of `checks` and rebase the remaining paths onto the asserted value.
pub(super) fn lift_root_assertion(
    checks: &mut Vec<Check>,
    bindings: &mut [PatternBinding],
) -> Option<Vec<String>> {
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
            let go_types: Vec<String> = alternatives
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

pub(super) fn render_condition(checks: &[Check], subject_var: SubjectRoot<'_>) -> GoExpression {
    if checks.is_empty() {
        return GoExpression::literal("true".to_string());
    }
    join_conditions(checks, subject_var)
}

fn join_conditions(checks: &[Check], subject: SubjectRoot<'_>) -> GoExpression {
    checks
        .iter()
        .map(|check| check.render(subject))
        .reduce(|left, right| GoExpression::binary(left, BinaryOp::And, right))
        .expect("join_conditions requires at least one check")
}

#[cfg(test)]
mod tests {
    use super::{
        AccessPath, ArmInfo, ArmLeaf, BindingTarget, ChainArm, Check, Decision, GoExpression,
        GuardedLeaf, PatternBinding, build_chain, share_alternative_bindings, try_build_switch,
    };
    use syntax::ast::BindingId;
    use syntax::types::Type;

    fn binding(name: &str, id: u32, target: BindingTarget) -> PatternBinding {
        PatternBinding {
            lisette_name: name.into(),
            binding_ids: vec![BindingId::new(id)],
            target,
            path: AccessPath::root(),
        }
    }

    #[test]
    fn alternatives_share_unused_targets_and_union_ids() {
        let named = |go_name: &str| BindingTarget::Named {
            go_name: go_name.into(),
            ty: Type::int(),
        };
        let mut first = vec![binding("x", 1, named("x")), binding("y", 2, named("y"))];
        let mut second = vec![
            binding("x", 3, BindingTarget::Unused),
            binding("y", 4, named("y2")),
        ];
        let mut third = vec![binding("x", 1, named("x")), binding("y", 2, named("y"))];
        share_alternative_bindings(&mut [&mut first, &mut second, &mut third]);

        let ids = |bindings: &[PatternBinding], index: usize| bindings[index].binding_ids.clone();
        for alternative in [&first, &second, &third] {
            assert_eq!(alternative[0].target, BindingTarget::Unused);
            assert_eq!(
                ids(alternative, 0),
                vec![BindingId::new(1), BindingId::new(3)]
            );
            assert_eq!(
                ids(alternative, 1),
                vec![BindingId::new(2), BindingId::new(4)]
            );
        }
        assert_eq!(first[1].target, named("y"));
        assert_eq!(second[1].target, named("y2"));
    }

    fn arm(arm_index: usize, checked: bool, has_guard: bool) -> ArmInfo {
        let checks = if checked {
            vec![Check::SliceLenEq {
                path: AccessPath::root(),
                length: arm_index,
            }]
        } else {
            vec![]
        };
        ArmInfo {
            arm_index,
            checks,
            bindings: vec![],
            has_guard,
        }
    }

    #[test]
    fn chain_ends_at_the_first_unguarded_catchall() {
        let Decision::Chain { tests, catchall } = build_chain(vec![
            arm(0, true, false),
            arm(1, false, true),
            arm(2, false, false),
            arm(3, true, false),
        ]) else {
            panic!("expected a chain");
        };
        assert!(matches!(catchall, Some(ArmLeaf { arm_index: 2, .. })));
        assert!(matches!(
            tests[0].arm,
            ChainArm::Leaf(ArmLeaf { arm_index: 0, .. })
        ));
        let ChainArm::Guard(GuardedLeaf {
            leaf: ArmLeaf { arm_index: 1, .. },
            failure,
        }) = &tests[1].arm
        else {
            panic!("expected the guarded catchall as a guard test");
        };
        assert!(matches!(
            **failure,
            Decision::Success(ArmLeaf { arm_index: 2, .. })
        ));
        assert_eq!(tests.len(), 2);
    }

    #[test]
    fn chain_without_unguarded_catchall_has_no_catchall() {
        let Decision::Chain { tests, catchall } =
            build_chain(vec![arm(0, true, false), arm(1, true, true)])
        else {
            panic!("expected a chain");
        };
        assert!(catchall.is_none());
        assert_eq!(tests.len(), 2);
    }

    fn switch_arm(arm_index: usize, check: Option<Check>, has_guard: bool) -> ArmInfo {
        ArmInfo {
            arm_index,
            checks: check.into_iter().collect(),
            bindings: vec![],
            has_guard,
        }
    }

    fn type_assert(go_type: &str) -> Option<Check> {
        Some(Check::TypeAssert {
            path: AccessPath::root(),
            go_type: go_type.into(),
        })
    }

    fn literal(text: &str) -> Option<Check> {
        Some(Check::Literal {
            path: AccessPath::root(),
            go_literal: GoExpression::literal(text.to_string()),
        })
    }

    #[test]
    fn root_assertions_build_a_type_switch_with_guards() {
        let Some(Decision::TypeSwitch { branches, fallback }) = try_build_switch(&[
            switch_arm(0, type_assert("A"), true),
            switch_arm(1, type_assert("B"), false),
            switch_arm(2, None, false),
        ]) else {
            panic!("expected a type switch");
        };
        let labels: Vec<_> = branches.iter().map(|branch| branch.label.clone()).collect();
        assert_eq!(labels, vec![vec!["A".to_string()], vec!["B".to_string()]]);
        assert!(fallback.is_some());
    }

    #[test]
    fn value_switch_rejects_guards_and_type_switch_rejects_mixed_arms() {
        assert!(matches!(
            try_build_switch(&[
                switch_arm(0, literal("1"), false),
                switch_arm(1, literal("2"), false),
            ]),
            Some(Decision::Switch(_))
        ));
        assert!(
            try_build_switch(&[
                switch_arm(0, literal("1"), true),
                switch_arm(1, literal("2"), false),
            ])
            .is_none()
        );
        assert!(
            try_build_switch(&[
                switch_arm(0, type_assert("A"), false),
                switch_arm(1, literal("2"), false),
            ])
            .is_none()
        );
    }
}
