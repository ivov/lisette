use crate::patterns::binding_decls::pattern_has_bindings;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{
    ConstructorPatternResolution, EnumFieldDefinition, MatchArm, Pattern, RecordPatternResolution,
    RestPattern, SequencePatternResolution, StructFieldPattern,
};
use syntax::parse::TUPLE_FIELDS;
use syntax::program::DefinitionBody;
use syntax::types::{Type, unqualified_name};

use crate::Planner;
use crate::names::packages::PackageRequirements;
use crate::names::{generics, go_name};
use crate::patterns::binding_decls::emit_pattern_literal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathSegment {
    /// `.FieldName` (Go name, already resolved).
    Field(String),
    Index(usize),
    /// `[offset:]`.
    SliceFrom(usize),
    ArraySliceFrom {
        offset: usize,
        go_type: String,
    },
    /// `(*expression)` — auto-pointer deref for recursive enum fields.
    Deref,
    /// `GoType(expression)` — newtype cast to underlying Go type.
    NewtypeCast(String),
    /// `expression.(GoType)` — Go interface type assertion, inserted when a
    /// concrete pattern targets a Go interface at a non-root path.
    AssertedAs(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessPath {
    pub segments: Vec<PathSegment>,
}

impl AccessPath {
    fn root() -> Self {
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

    pub(crate) fn render(&self, subject: &str) -> String {
        let mut result = subject.to_string();
        let last = self.segments.len().saturating_sub(1);
        for (i, seg) in self.segments.iter().enumerate() {
            match seg {
                PathSegment::Field(name) => result = format!("{}.{}", result, name),
                PathSegment::Index(index) => result = format!("{}[{}]", result, index),
                PathSegment::SliceFrom(offset) => result = format!("{}[{}:]", result, offset),
                PathSegment::ArraySliceFrom { offset, go_type } => {
                    result = format!("{}({}[{}:])", go_type, result, offset)
                }
                PathSegment::Deref => {
                    if i == last {
                        result = format!("*{}", result);
                    } else {
                        result = format!("(*{})", result);
                    }
                }
                PathSegment::NewtypeCast(ty) => {
                    result = if ty.starts_with('*') {
                        format!("({})({})", ty, result)
                    } else {
                        format!("{}({})", ty, result)
                    }
                }
                PathSegment::AssertedAs(ty) => {
                    result = format!("{}.({})", result, ty);
                }
            }
        }
        result
    }

    /// Like `render`, but a tail-`Deref` is parenthesized so the result is
    /// safe as a selector receiver, index target, or call callee.
    pub(crate) fn render_composable(&self, subject: &str) -> String {
        let rendered = self.render(subject);
        if matches!(self.segments.last(), Some(PathSegment::Deref)) {
            format!("({})", rendered)
        } else {
            rendered
        }
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

#[derive(Clone, Debug)]
pub(crate) enum Check {
    EnumTag {
        path: AccessPath,
        tag_constant: String,
    },
    Literal {
        path: AccessPath,
        go_literal: String,
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

impl Check {
    fn render(&self, subject: &str) -> String {
        match self {
            Check::EnumTag {
                path, tag_constant, ..
            } => {
                let rendered_path = path.render(subject);
                format!("{}.Tag == {}", rendered_path, tag_constant)
            }
            Check::Literal {
                path, go_literal, ..
            } => {
                let rendered_path = path.render(subject);
                match go_literal.as_str() {
                    "true" => rendered_path,
                    "false" => format!("!{}", rendered_path),
                    _ => format!("{} == {}", rendered_path, go_literal),
                }
            }
            Check::SliceLenEq { path, length } => {
                let rendered_path = path.render(subject);
                format!("len({}) == {}", rendered_path, length)
            }
            Check::SliceLenGe { path, min_length } => {
                let rendered_path = path.render(subject);
                format!("len({}) >= {}", rendered_path, min_length)
            }
            Check::Or { alternatives } => {
                let alt_strs: Vec<String> = alternatives
                    .iter()
                    .map(|checks| {
                        if checks.len() == 1 {
                            checks[0].render(subject)
                        } else {
                            format!(
                                "({})",
                                checks
                                    .iter()
                                    .map(|c| c.render(subject))
                                    .collect::<Vec<_>>()
                                    .join(" && ")
                            )
                        }
                    })
                    .collect();
                let joined = alt_strs.join(" || ");
                if alt_strs.len() > 1 {
                    format!("({})", joined)
                } else {
                    joined
                }
            }
            Check::TypeAssert { path, go_type } => format!(
                "func() bool {{ _, ok := {}.({}); return ok }}()",
                path.render(subject),
                go_type,
            ),
        }
    }

    /// Comparison-shaped checks flip their operator; `Or`/`TypeAssert` wrap
    /// in `!(...)`.
    pub(crate) fn render_negated(&self, subject: &str) -> String {
        match self {
            Check::EnumTag {
                path, tag_constant, ..
            } => {
                let rendered_path = path.render(subject);
                format!("{}.Tag != {}", rendered_path, tag_constant)
            }
            Check::Literal {
                path, go_literal, ..
            } => {
                let rendered_path = path.render(subject);
                match go_literal.as_str() {
                    "true" => format!("!{}", rendered_path),
                    "false" => rendered_path,
                    _ => format!("{} != {}", rendered_path, go_literal),
                }
            }
            Check::SliceLenEq { path, length } => {
                let rendered_path = path.render(subject);
                format!("len({}) != {}", rendered_path, length)
            }
            Check::SliceLenGe { path, min_length } => {
                let rendered_path = path.render(subject);
                format!("len({}) < {}", rendered_path, min_length)
            }
            Check::Or { .. } | Check::TypeAssert { .. } => {
                format!("!({})", self.render(subject))
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

    fn as_enum_tag(&self) -> Option<&str> {
        match self {
            Check::EnumTag { tag_constant, .. } => Some(tag_constant),
            _ => None,
        }
    }

    fn as_literal(&self) -> Option<&str> {
        match self {
            Check::Literal { go_literal, .. } => Some(go_literal),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PatternBinding {
    pub lisette_name: String,
    /// `None` when the binding is unused.
    pub go_name: Option<String>,
    pub path: AccessPath,
}

/// Root-path Go-interface type assertion lifted out of `checks`.
#[derive(Clone, Debug)]
pub(crate) struct TypeAssertion {
    pub path: AccessPath,
    pub go_types: Vec<String>,
}

pub(crate) struct PatternInfo {
    pub root_assertion: Option<TypeAssertion>,
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
    pub packages: PackageRequirements,
}

impl PatternInfo {
    /// True when a downstream consumer will reference the asserted value.
    pub(super) fn requires_asserted_subject(&self) -> bool {
        !self.checks.is_empty() || self.bindings.iter().any(|b| b.go_name.is_some())
    }
}

/// Result of compiling a list of expanded arms into a `Decision`.
pub(crate) struct CompiledDecision {
    pub decision: Decision,
    pub packages: PackageRequirements,
}

/// Accumulates checks, bindings, and package requirements during compilation.
struct PatternCollector {
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
    packages: PackageRequirements,
}

impl PatternCollector {
    fn new() -> Self {
        Self {
            checks: Vec::new(),
            bindings: Vec::new(),
            packages: PackageRequirements::default(),
        }
    }
}

/// A pre-computed decision tree for pattern matching.
///
#[derive(Debug)]
pub(crate) enum Decision {
    Success {
        arm_index: usize,
        bindings: Vec<PatternBinding>,
    },
    /// Guarded success: emit body iff the guard holds, else continue with
    /// `failure`.
    Guard {
        arm_index: usize,
        bindings: Vec<PatternBinding>,
        success: Box<Decision>,
        failure: Box<Decision>,
    },
    /// Emits as Go `switch` when eligible.
    Switch {
        path: AccessPath,
        kind: SwitchKind,
        shape: SwitchShape,
        branches: Vec<SwitchBranch>,
        fallback: Option<Box<Decision>>,
    },
    /// Emits as `if`/`else if`/`else`.
    Chain {
        tests: Vec<ChainTest>,
        fallback: Box<Decision>,
    },
    /// Emits `panic("unreachable")` in tail position.
    Unreachable,
}

#[derive(Debug, Clone)]
pub(crate) enum SwitchKind {
    EnumTag,
    Value,
    TypeSwitch,
}

/// Structural shape of a switch site, chosen at build time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchShape {
    TypeSwitch,
    /// Two branches with `"true"`/`"false"` labels, no fallback.
    Bool,
    /// Two branches, no fallback, not `Bool`.
    Binary,
    SingleArm,
    Multi,
}

fn classify_switch_shape(
    kind: &SwitchKind,
    branches: &[SwitchBranch],
    fallback: &Option<Box<Decision>>,
) -> SwitchShape {
    if matches!(kind, SwitchKind::TypeSwitch) {
        return SwitchShape::TypeSwitch;
    }
    let is_bool = matches!(kind, SwitchKind::Value)
        && branches.len() == 2
        && fallback.is_none()
        && branches.iter().any(|b| b.case_label == "true")
        && branches.iter().any(|b| b.case_label == "false");
    if is_bool {
        return SwitchShape::Bool;
    }
    if branches.len() == 2 && fallback.is_none() {
        return SwitchShape::Binary;
    }
    if branches.len() == 1 {
        return SwitchShape::SingleArm;
    }
    SwitchShape::Multi
}

/// True when the tree has an unconditional success path.
pub(crate) fn decision_is_exhaustive(tree: &Decision) -> bool {
    match tree {
        Decision::Success { .. } => true,
        Decision::Chain {
            tests, fallback, ..
        } => {
            (matches!(fallback.as_ref(), Decision::Unreachable) && tests.len() > 1)
                || decision_is_exhaustive(fallback)
        }
        Decision::Switch {
            fallback, branches, ..
        } => fallback.is_some() || !branches.is_empty(),
        _ => false,
    }
}

/// True when the tree has a terminal Success reachable without passing a guard.
pub(crate) fn tree_has_unguarded_terminal(tree: &Decision) -> bool {
    match tree {
        Decision::Success { .. } => true,
        Decision::Guard { failure, .. } => tree_has_unguarded_terminal(failure),
        Decision::Chain {
            tests, fallback, ..
        } => {
            tree_has_unguarded_terminal(fallback)
                || (matches!(fallback.as_ref(), Decision::Unreachable)
                    && tests
                        .last()
                        .is_some_and(|t| tree_has_unguarded_terminal(&t.decision)))
        }
        Decision::Switch {
            fallback, branches, ..
        } => fallback
            .as_ref()
            .map_or(!branches.is_empty(), |fb| tree_has_unguarded_terminal(fb)),
        Decision::Unreachable => false,
    }
}

#[derive(Debug)]
pub(crate) struct SwitchBranch {
    pub case_label: String,
    pub decision: Decision,
}

#[derive(Debug)]
pub(crate) struct ChainTest {
    pub checks: Vec<Check>,
    pub decision: Decision,
}

#[derive(Clone)]
struct ArmInfo {
    arm_index: usize,
    root_assertion: Option<TypeAssertion>,
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
    has_guard: bool,
}

impl ArmInfo {
    fn is_catchall(&self) -> bool {
        self.checks.is_empty() && self.root_assertion.is_none()
    }
}

/// Build a Decision tree from a list of arm infos.
fn build_tree(arms: Vec<ArmInfo>) -> Decision {
    if arms.is_empty() {
        return Decision::Unreachable;
    }

    let first_is_catchall = arms[0].is_catchall();

    if first_is_catchall && !arms[0].has_guard {
        return Decision::Success {
            arm_index: arms[0].arm_index,
            bindings: arms[0].bindings.clone(),
        };
    }

    if first_is_catchall && arms[0].has_guard {
        let rest = arms[1..].to_vec();
        return Decision::Guard {
            arm_index: arms[0].arm_index,
            bindings: arms[0].bindings.clone(),
            success: Box::new(Decision::Success {
                arm_index: arms[0].arm_index,
                bindings: vec![],
            }),
            failure: Box::new(build_tree(rest)),
        };
    }

    if let Some(switch) = try_build_switch(&arms) {
        return switch;
    }

    build_chain(arms)
}

/// Try to build a Switch node from the arms.
///
/// Returns Some(Switch) if ALL non-catchall arms agree on a switchable shape:
/// a root TypeAssertion (TypeSwitch), or a single same-path EnumTag/Literal
/// first check (EnumTag/Value), with guards permitted only for TypeSwitch.
fn try_build_switch(arms: &[ArmInfo]) -> Option<Decision> {
    let first_relevant = arms.iter().find(|a| !a.is_catchall())?;
    let (kind, switch_path) = pick_switch_kind(first_relevant)?;

    validate_switch_arms(arms, &kind, &switch_path)?;

    let grouped = group_switch_branches(arms, &kind);
    let branches = build_switch_branches(grouped.order, grouped.by_label, &grouped.fallback);

    let fallback = if grouped.fallback.is_empty() {
        None
    } else {
        Some(Box::new(build_tree(grouped.fallback)))
    };

    let shape = classify_switch_shape(&kind, &branches, &fallback);
    Some(Decision::Switch {
        path: switch_path,
        kind,
        shape,
        branches,
        fallback,
    })
}

fn pick_switch_kind(arm: &ArmInfo) -> Option<(SwitchKind, AccessPath)> {
    if let Some(assertion) = &arm.root_assertion {
        return Some((SwitchKind::TypeSwitch, assertion.path.clone()));
    }
    let first_check = arm.checks.first()?;
    let path = first_check.path()?.clone();
    if first_check.as_enum_tag().is_some() {
        Some((SwitchKind::EnumTag, path))
    } else if first_check.as_literal().is_some() {
        Some((SwitchKind::Value, path))
    } else {
        None
    }
}

fn validate_switch_arms(
    arms: &[ArmInfo],
    kind: &SwitchKind,
    switch_path: &AccessPath,
) -> Option<()> {
    for arm in arms {
        if arm.is_catchall() {
            continue;
        }
        if arm.has_guard && !matches!(kind, SwitchKind::TypeSwitch) {
            return None;
        }

        let arm_path = match kind {
            SwitchKind::EnumTag => {
                if arm.root_assertion.is_some() {
                    return None;
                }
                let first = arm.checks.first()?;
                first.as_enum_tag()?;
                first.path()?
            }
            SwitchKind::Value => {
                if arm.root_assertion.is_some() {
                    return None;
                }
                let first = arm.checks.first()?;
                first.as_literal()?;
                if arm.checks.len() != 1 {
                    return None;
                }
                first.path()?
            }
            SwitchKind::TypeSwitch => &arm.root_assertion.as_ref()?.path,
        };
        if arm_path != switch_path {
            return None;
        }
    }
    Some(())
}

struct BranchGroup {
    arms: Vec<ArmInfo>,
}

struct GroupedBranches {
    order: Vec<String>,
    by_label: HashMap<String, BranchGroup>,
    fallback: Vec<ArmInfo>,
}

fn group_switch_branches(arms: &[ArmInfo], kind: &SwitchKind) -> GroupedBranches {
    let mut by_label: HashMap<String, BranchGroup> = HashMap::default();
    let mut order: Vec<String> = Vec::new();
    let mut fallback = Vec::new();

    for arm in arms {
        if arm.is_catchall() {
            fallback.push(arm.clone());
            continue;
        }

        let (case_label, inner_checks) = match kind {
            SwitchKind::EnumTag => {
                let tag = arm.checks[0].as_enum_tag().unwrap();
                (tag.to_string(), arm.checks[1..].to_vec())
            }
            SwitchKind::Value => {
                let lit = arm.checks[0].as_literal().unwrap();
                (lit.to_string(), arm.checks[1..].to_vec())
            }
            SwitchKind::TypeSwitch => {
                let assertion = arm.root_assertion.as_ref().unwrap();
                (assertion.go_types.join(", "), arm.checks.clone())
            }
        };
        let inner_arm = ArmInfo {
            arm_index: arm.arm_index,
            root_assertion: None,
            checks: inner_checks,
            bindings: arm.bindings.clone(),
            has_guard: arm.has_guard,
        };

        by_label
            .entry(case_label.clone())
            .and_modify(|group| group.arms.push(inner_arm.clone()))
            .or_insert_with(|| {
                order.push(case_label);
                BranchGroup {
                    arms: vec![inner_arm],
                }
            });
    }

    GroupedBranches {
        order,
        by_label,
        fallback,
    }
}

/// Splice the catchall into any fail-prone case body: Go `switch` cases do not
/// fall through to `default`, so a failed inner check has nowhere else to go.
fn build_switch_branches(
    order: Vec<String>,
    mut by_label: HashMap<String, BranchGroup>,
    fallback_arms: &[ArmInfo],
) -> Vec<SwitchBranch> {
    order
        .into_iter()
        .map(|label| {
            let BranchGroup { arms: inner_arms } = by_label.remove(&label).unwrap();
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
                case_label: label,
                decision,
            }
        })
        .collect()
}

/// Build a Chain (if/else if/else) from the arms.
fn build_chain(arms: Vec<ArmInfo>) -> Decision {
    let mut tests = Vec::new();

    for (i, arm) in arms.iter().enumerate() {
        if arm.is_catchall() && !arm.has_guard {
            let fallback = Decision::Success {
                arm_index: arm.arm_index,
                bindings: arm.bindings.clone(),
            };
            return if tests.is_empty() {
                fallback
            } else {
                Decision::Chain {
                    tests,
                    fallback: Box::new(fallback),
                }
            };
        }

        let decision = if arm.has_guard {
            let remaining = arms[i + 1..].to_vec();
            Decision::Guard {
                arm_index: arm.arm_index,
                bindings: arm.bindings.clone(),
                success: Box::new(Decision::Success {
                    arm_index: arm.arm_index,
                    bindings: vec![],
                }),
                failure: Box::new(build_tree(remaining)),
            }
        } else {
            Decision::Success {
                arm_index: arm.arm_index,
                bindings: arm.bindings.clone(),
            }
        };

        let mut checks = arm.checks.clone();
        if let Some(assertion) = arm.root_assertion.clone() {
            checks.insert(0, type_assertion_to_check(assertion));
        }
        tests.push(ChainTest { checks, decision });
    }

    Decision::Chain {
        tests,
        fallback: Box::new(Decision::Unreachable),
    }
}

/// Re-encode a lifted `TypeAssertion` back as a renderable `Check` for chain
/// emission when a type switch cannot consolidate the arms.
fn type_assertion_to_check(assertion: TypeAssertion) -> Check {
    let TypeAssertion { path, mut go_types } = assertion;
    if go_types.len() == 1 {
        return Check::TypeAssert {
            path,
            go_type: go_types.pop().unwrap(),
        };
    }
    Check::Or {
        alternatives: go_types
            .into_iter()
            .map(|go_type| {
                vec![Check::TypeAssert {
                    path: path.clone(),
                    go_type,
                }]
            })
            .collect(),
    }
}

/// Recursively walk a pattern, collecting checks and bindings.
///
/// `path_ty` is the expected type of the value at `path` — used to detect
/// when a struct pattern is matched against a Go interface (type switch).
fn collect_checks_and_bindings(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Unit { .. } => {}

        Pattern::Identifier { identifier, .. } => {
            let go_name = planner.go_name_for_binding(pattern);
            collector.bindings.push(PatternBinding {
                lisette_name: identifier.to_string(),
                go_name,
                path: path.clone(),
            });
        }

        Pattern::Literal { literal, .. } => {
            collector.checks.push(Check::Literal {
                path: path.clone(),
                go_literal: emit_pattern_literal(literal),
            });
        }

        Pattern::EnumVariant { .. } => {
            collect_enum_variant_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Struct { .. } => {
            collect_struct_checks(planner, path, pattern, path_ty, collector);
        }

        Pattern::Tuple { elements, .. } => {
            collect_tuple_checks(planner, path, elements, path_ty, collector);
        }

        Pattern::Slice {
            prefix,
            rest,
            resolution,
            ..
        } => {
            collect_slice_checks(planner, path, prefix, rest, resolution, collector);
        }

        Pattern::Or { patterns, .. } => {
            collect_or_pattern_checks(planner, path, patterns, pattern, path_ty, collector);
        }

        p @ Pattern::AsBinding {
            pattern: inner,
            name,
            ..
        } => {
            collect_checks_and_bindings(planner, path, inner, path_ty, collector);
            let go_name = planner.go_name_for_binding(p);
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                go_name,
                path: path.clone(),
            });
        }
    }
}

fn collect_tuple_checks(
    planner: &Planner,
    path: &AccessPath,
    elements: &[Pattern],
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let stripped_path_ty = path_ty.map(Type::strip_refs);
    let element_tys: Option<&[Type]> = match &stripped_path_ty {
        Some(Type::Tuple(tys)) => Some(tys.as_slice()),
        _ => None,
    };

    for (i, element) in elements.iter().enumerate() {
        let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
        let field_path = path.push(PathSegment::Field(field_name.to_string()));
        collect_checks_and_bindings(
            planner,
            &field_path,
            element,
            element_tys.and_then(|tys| tys.get(i)),
            collector,
        );
    }
}

fn collect_slice_checks(
    planner: &Planner,
    path: &AccessPath,
    prefix: &[Pattern],
    rest: &RestPattern,
    resolution: &SequencePatternResolution,
    collector: &mut PatternCollector,
) {
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
        | SequencePatternResolution::Array { element_type, .. } => Some(element_type),
        SequencePatternResolution::Unresolved => None,
    };

    for (i, element) in prefix.iter().enumerate() {
        let element_path = path.push(PathSegment::Index(i));
        collect_checks_and_bindings(planner, &element_path, element, element_type, collector);
    }

    if let RestPattern::Bind { name, .. } = rest {
        let go_name = planner.go_name_for_rest_binding(rest);
        let segment = match &array_info {
            Some((length, element_type)) => {
                let sub_length = length.saturating_sub(prefix.len() as u64);
                let sub_ty = Type::Array {
                    length: sub_length,
                    element: Box::new((*element_type).clone()),
                };
                PathSegment::ArraySliceFrom {
                    offset: prefix.len(),
                    go_type: planner.go_type_string(&sub_ty),
                }
            }
            None => PathSegment::SliceFrom(prefix.len()),
        };
        collector.bindings.push(PatternBinding {
            lisette_name: name.to_string(),
            go_name,
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
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let has_bindings = pattern_has_bindings(pattern);
    if !has_bindings {
        let alt_collectors: Vec<PatternCollector> = patterns
            .iter()
            .map(|p| {
                let mut alt_collector = PatternCollector::new();
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
    variant_fields: Option<&[EnumFieldDefinition]>,
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
    } else if planner.struct_field_is_exported(ty, &field.name) {
        go_name::make_exported(&field.name)
    } else if planner.field_is_embedded(ty, &field.name) {
        go_name::escape_keyword(&field.name).into_owned()
    } else {
        go_name::unexported_method_go_name(&field.name)
    };

    if let Some((_, variant_name)) = enum_info
        && let Some(field_index) =
            planner.get_enum_struct_field_index(ty, variant_name, &field.name)
    {
        let is_source_ref = variant_fields
            .and_then(|vf| vf.get(field_index).map(|f| f.ty.is_ref()))
            .unwrap_or_else(|| planner.is_enum_field_source_ref(ty, variant_name, field_index));
        let is_auto_pointer =
            planner.is_enum_field_pointer(ty, variant_name, field_index) && !is_source_ref;
        if is_auto_pointer {
            return parent_path
                .push(PathSegment::Field(go_field_name))
                .push(PathSegment::Deref);
        }
    }

    parent_path.push(PathSegment::Field(go_field_name))
}

/// When a concrete pattern targets a Go-interface scrutinee, push a TypeAssert
/// check and return the path child patterns should read from. At root, child
/// paths stay as-is and the type switch shadows the subject; at nested paths,
/// child paths gain an `AssertedAs` segment so they reach the asserted value.
fn interface_assert_child_path(
    planner: &Planner,
    path: &AccessPath,
    pattern_ty: &Type,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) -> Option<AccessPath> {
    path_ty.filter(|st| planner.facts.as_interface(st).is_some())?;
    let go_type_result = planner.go_type(pattern_ty);
    collector.packages.extend(go_type_result.requirements());
    let go_type = go_type_result.code;
    let child_path = if path.is_root() {
        path.clone()
    } else {
        path.push(PathSegment::AssertedAs(go_type.clone()))
    };
    collector.checks.push(Check::TypeAssert {
        path: path.clone(),
        go_type,
    });
    Some(child_path)
}

/// Collect checks and bindings for an enum variant pattern (tuple or tagged).
fn collect_enum_variant_checks(
    planner: &Planner,
    path: &AccessPath,
    pattern: &Pattern,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
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
    let (variant_fields, field_types): (&[EnumFieldDefinition], Vec<Type>) = match &definition.body
    {
        DefinitionBody::Struct {
            fields, generics, ..
        } => (
            &[],
            fields
                .iter()
                .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
                .collect(),
        ),
        DefinitionBody::Enum {
            variants, generics, ..
        } => {
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == unqualified_name(variant_name))
            else {
                return;
            };
            (
                variant.fields.as_slice(),
                variant
                    .fields
                    .iter()
                    .map(|field| generics::resolve_field_type(generics, &params, &field.ty))
                    .collect(),
            )
        }
        _ => return,
    };

    let variant_data = EnumVariantData {
        identifier,
        fields,
        ty,
        variant_fields,
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

/// Emit a const pattern as a Go `case` constant (e.g. `time.Friday`), requiring
/// the constant's package import when it is cross-module.
fn collect_const_pattern_check(
    planner: &Planner,
    path: &AccessPath,
    qualified_name: &str,
    collector: &mut PatternCollector,
) {
    let const_name = go_name::unqualified_name(qualified_name);
    let go_literal = match planner.facts.module_for_qualified_name(qualified_name) {
        Some(module) => {
            let qualifier = planner.go_pkg_qualifier(module);
            if qualifier.is_empty() || qualifier == planner.facts.current_module() {
                const_name.to_string()
            } else {
                let qualifier = planner.record_module_import(module, &mut collector.packages);
                format!("{}.{}", qualifier, const_name)
            }
        }
        None => const_name.to_string(),
    };
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal,
    });
}

/// `true` when the variant is a foreign-module dotted name (e.g.
/// `httpkg.MethodGet`) emitted as a `Check::Literal`.
fn handle_foreign_variant_literal(
    planner: &Planner,
    path: &AccessPath,
    ty: &Type,
    identifier: &str,
    collector: &mut PatternCollector,
) -> bool {
    if planner.as_enum(ty).is_some() || !identifier.contains('.') {
        return false;
    }
    if let Some((module, _)) = identifier.split_once('.')
        && planner.facts.is_foreign_module(module)
    {
        collector
            .packages
            .require(planner.package_use_for_module(&planner.canonical_module(module)));
    }
    collector.checks.push(Check::Literal {
        path: path.clone(),
        go_literal: identifier.to_string(),
    });
    true
}

/// Collect checks and bindings for a newtype struct pattern (single-field wrapper).
fn collect_newtype_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
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
            variant.field_types.first(),
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
    collector: &mut PatternCollector,
) {
    for (i, field) in fields.iter().enumerate() {
        let field_path = path.push(PathSegment::Field(format!("F{}", i)));
        collect_checks_and_bindings(planner, &field_path, field, field_types.get(i), collector);
    }
}

struct EnumVariantData<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    variant_fields: &'a [EnumFieldDefinition],
    field_types: &'a [Type],
}

fn enum_module_of<'a>(planner: &Planner<'a>, ty: &'a Type) -> &'a str {
    match ty {
        Type::Nominal { id, .. } => planner.facts.module_for_qualified_name(id).unwrap_or(id),
        _ => "",
    }
}

/// Collect checks and bindings for a tagged enum variant pattern.
fn collect_tagged_enum_checks(
    planner: &Planner,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
) {
    let enum_module = enum_module_of(planner, variant.ty);
    let alias = if planner.facts.is_foreign_module(enum_module) {
        Some(planner.record_module_import(enum_module, &mut collector.packages))
    } else {
        None
    };
    let resolved = go_name::variant(
        variant.identifier,
        variant.ty,
        enum_module,
        planner.facts.current_module(),
        alias.as_deref(),
    );
    if let Some(package) = resolved.package {
        collector.packages.require_generated(package);
    }
    collector.checks.push(Check::EnumTag {
        path: path.clone(),
        tag_constant: resolved.name.clone(),
    });

    let variant_name = variant
        .identifier
        .split('.')
        .next_back()
        .unwrap_or(variant.identifier);
    for (i, field) in variant.fields.iter().enumerate() {
        let field_name = planner.get_enum_tuple_field_name(variant.ty, variant_name, i);

        let is_source_ref = variant
            .variant_fields
            .get(i)
            .map(|f| f.ty.is_ref())
            .unwrap_or_else(|| planner.is_enum_field_source_ref(variant.ty, variant_name, i));
        let is_auto_pointer =
            planner.is_enum_field_pointer(variant.ty, variant_name, i) && !is_source_ref;

        let is_unit = planner.is_enum_field_unit(variant.ty, variant_name, i);

        let field_path = if is_auto_pointer {
            path.push(PathSegment::Field(field_name))
                .push(PathSegment::Deref)
        } else {
            path.push(PathSegment::Field(field_name))
        };

        if is_unit {
            if let Pattern::Identifier { identifier, .. } = field {
                collector.bindings.push(PatternBinding {
                    lisette_name: identifier.to_string(),
                    go_name: None,
                    path: field_path,
                });
            }
        } else {
            collect_checks_and_bindings(
                planner,
                &field_path,
                field,
                variant.field_types.get(i),
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
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let Pattern::Struct {
        fields,
        ty,
        identifier,
        resolution,
        ..
    } = pattern
    else {
        return;
    };

    let (enum_info, child_path) = resolve_struct_child_path(
        planner, path, ty, identifier, resolution, path_ty, collector,
    );
    let variant_fields = record_variant_fields(planner, resolution);

    for field in fields {
        let field_path = compute_struct_field_path(
            planner,
            &child_path,
            field,
            ty,
            enum_info.as_ref(),
            variant_fields,
        );
        let field_ty = record_field_type(planner, resolution, ty, &field.name);
        collect_checks_and_bindings(
            planner,
            &field_path,
            &field.value,
            field_ty.as_ref(),
            collector,
        );
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
    ty: &Type,
    identifier: &str,
    resolution: &RecordPatternResolution,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) -> (Option<(String, String)>, AccessPath) {
    if let Some(asserted) = interface_assert_child_path(planner, path, ty, path_ty, collector) {
        return (None, asserted);
    }
    let enum_info = detect_enum_info(resolution);
    if enum_info.is_some() {
        let enum_module = enum_module_of(planner, ty);
        let alias = if planner.facts.is_foreign_module(enum_module) {
            Some(planner.record_module_import(enum_module, &mut collector.packages))
        } else {
            None
        };
        let resolved = go_name::variant(
            identifier,
            ty,
            enum_module,
            planner.facts.current_module(),
            alias.as_deref(),
        );
        if let Some(package) = resolved.package {
            collector.packages.require_generated(package);
        }
        collector.checks.push(Check::EnumTag {
            path: path.clone(),
            tag_constant: resolved.name.clone(),
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

/// Expand match arms, splitting or-patterns with bindings into separate arms.
///
/// Or-patterns without bindings are handled inline by the condition collector.
/// Or-patterns with bindings need separate arms so each alternative can bind
/// its own variables.
pub(super) fn expand_or_patterns<'a>(arms: &'a [MatchArm]) -> Vec<ExpandedArm<'a>> {
    let mut result = Vec::new();
    for (i, arm) in arms.iter().enumerate() {
        if let Pattern::Or { patterns, .. } = &arm.pattern
            && pattern_has_bindings(&arm.pattern)
        {
            for alt in patterns {
                result.push(ExpandedArm {
                    arm_index: i,
                    pattern: alt,
                    has_guard: arm.has_guard(),
                });
            }
            continue;
        }
        result.push(ExpandedArm {
            arm_index: i,
            pattern: &arm.pattern,
            has_guard: arm.has_guard(),
        });
    }
    result
}

/// An expanded arm reference, possibly one alternative of an or-pattern.
pub(super) struct ExpandedArm<'a> {
    pub arm_index: usize,
    pub pattern: &'a Pattern,
    pub has_guard: bool,
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
                let mut checks = alt.clone();
                let root_assertion = extract_root_assertion(&mut checks);
                result.push(ArmInfo {
                    arm_index: arm.arm_index,
                    root_assertion,
                    checks,
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

/// Compile expanded arms into a decision tree plus package requirements.
pub(super) fn compile_expanded_arms<'a>(
    planner: &Planner,
    expanded: &'a [ExpandedArm<'a>],
    subject_ty: &Type,
) -> CompiledDecision {
    let mut packages = PackageRequirements::default();
    let arm_infos: Vec<ArmInfo> = expanded
        .iter()
        .map(|ea| {
            let info = collect_pattern_info(planner, ea.pattern, subject_ty);
            packages.extend(&info.packages);
            ArmInfo {
                arm_index: ea.arm_index,
                root_assertion: info.root_assertion,
                checks: info.checks,
                bindings: info.bindings,
                has_guard: ea.has_guard,
            }
        })
        .collect();

    let mut arm_infos = expand_interface_or_checks(arm_infos);

    // Propagate unused status across or-pattern alternatives sharing an arm.
    let has_or_patterns = expanded
        .windows(2)
        .any(|w| w[0].arm_index == w[1].arm_index);
    if has_or_patterns {
        let mut unused_by_arm: HashMap<usize, HashSet<String>> = HashMap::default();
        for info in &arm_infos {
            for binding in &info.bindings {
                if binding.go_name.is_none() {
                    unused_by_arm
                        .entry(info.arm_index)
                        .or_default()
                        .insert(binding.lisette_name.clone());
                }
            }
        }
        for info in &mut arm_infos {
            if let Some(unused_names) = unused_by_arm.get(&info.arm_index) {
                for binding in &mut info.bindings {
                    if unused_names.contains(&binding.lisette_name) {
                        binding.go_name = None;
                    }
                }
            }
        }
    }

    CompiledDecision {
        decision: build_tree(arm_infos),
        packages,
    }
}

/// Collect checks, bindings, and any root type assertion from a single pattern
/// for use outside match emission (let-else, while-let, for-loop, complex let,
/// function param). `subject_ty` is the static type of the scrutinee; sites
/// that pass a wrong type would silently miss root-level Go-interface type
/// assertions, so the public entry takes it by reference rather than `Option`.
pub(crate) fn collect_pattern_info(
    planner: &Planner,
    pattern: &Pattern,
    subject_ty: &Type,
) -> PatternInfo {
    let mut collector = PatternCollector::new();
    collect_checks_and_bindings(
        planner,
        &AccessPath::root(),
        pattern,
        Some(subject_ty),
        &mut collector,
    );
    let root_assertion = extract_root_assertion(&mut collector.checks);
    PatternInfo {
        root_assertion,
        checks: collector.checks,
        bindings: collector.bindings,
        packages: collector.packages,
    }
}

/// Move a root-path type assertion out of `checks` into a `TypeAssertion`.
/// Recognizes both a single `Check::TypeAssert` at root and a `Check::Or` whose
/// alternatives are each a single root `TypeAssert` at the same path.
fn extract_root_assertion(checks: &mut Vec<Check>) -> Option<TypeAssertion> {
    let position = checks.iter().position(|c| match c {
        Check::TypeAssert { path, .. } => path.is_root(),
        Check::Or { alternatives } => alternatives.iter().all(
            |alt| matches!(alt.as_slice(), [Check::TypeAssert { path, .. }] if path.is_root()),
        ),
        _ => false,
    })?;
    match checks.remove(position) {
        Check::TypeAssert { path, go_type } => Some(TypeAssertion {
            path,
            go_types: vec![go_type],
        }),
        Check::Or { alternatives } => {
            let mut go_types = Vec::with_capacity(alternatives.len());
            let mut shared_path: Option<AccessPath> = None;
            for alt in alternatives {
                let [Check::TypeAssert { path, go_type }] = alt.as_slice() else {
                    unreachable!("predicate above confirmed shape")
                };
                if let Some(existing) = &shared_path {
                    debug_assert_eq!(existing, path);
                } else {
                    shared_path = Some(path.clone());
                }
                go_types.push(go_type.clone());
            }
            Some(TypeAssertion {
                path: shared_path.expect("at least one alternative"),
                go_types,
            })
        }
        _ => unreachable!(),
    }
}

pub(super) fn render_condition(checks: &[Check], subject_var: &str) -> String {
    if checks.is_empty() {
        return "true".to_string();
    }

    let conditions: Vec<String> = checks.iter().map(|c| c.render(subject_var)).collect();

    conditions.join(" && ")
}
