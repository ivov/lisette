use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{MatchArm, Pattern, RestPattern, StructFieldPattern, TypedPattern};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::{Type, unqualified_name};

use crate::Emitter;
use crate::names::go_name;
use crate::patterns::bindings::emit_pattern_literal;
use crate::write_line;

/// A single step in navigating from the match subject to a nested value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PathSegment {
    /// `.FieldName` — access a named field (Go name, already resolved)
    Field(String),
    /// `[i]` — index into a slice
    Index(usize),
    /// `[offset:]` — slice from offset to end
    SliceFrom(usize),
    /// `(*expression)` — dereference an auto-pointer (recursive enum fields)
    Deref,
    /// `GoType(expression)` — newtype cast to underlying Go type
    NewtypeCast(String),
    /// `expression.(GoType)` — Go interface type assertion. Inserted when a
    /// concrete pattern targets a Go interface at a non-root path, so child
    /// paths reach the asserted concrete value rather than the interface.
    AssertedAs(String),
}

/// A path from the match subject to a nested value, built up during compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessPath {
    pub segments: Vec<PathSegment>,
}

impl AccessPath {
    /// The root path (the match subject itself).
    pub(crate) fn root() -> Self {
        Self { segments: vec![] }
    }

    /// True if this is the root path (no segments).
    pub(crate) fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    /// Append a segment, returning a new path (non-mutating).
    pub(crate) fn push(&self, seg: PathSegment) -> Self {
        let mut new = self.clone();
        new.segments.push(seg);
        new
    }

    /// Render this path as a Go expression, given the subject variable name.
    pub(crate) fn render(&self, subject: &str) -> String {
        let mut result = subject.to_string();
        let last = self.segments.len().saturating_sub(1);
        for (i, seg) in self.segments.iter().enumerate() {
            match seg {
                PathSegment::Field(name) => result = format!("{}.{}", result, name),
                PathSegment::Index(idx) => result = format!("{}[{}]", result, idx),
                PathSegment::SliceFrom(offset) => result = format!("{}[{}:]", result, offset),
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
    /// safe to embed as a selector receiver, index target, or call callee.
    pub(crate) fn render_composable(&self, subject: &str) -> String {
        let rendered = self.render(subject);
        if matches!(self.segments.last(), Some(PathSegment::Deref)) {
            format!("({})", rendered)
        } else {
            rendered
        }
    }
}

/// A single runtime check that must be true for a pattern to match.
#[derive(Clone, Debug)]
pub(crate) enum Check {
    /// Enum tag equality: `path.Tag == TAG_CONSTANT`
    EnumTag {
        path: AccessPath,
        tag_constant: String,
        needs_stdlib: bool,
    },
    /// Literal equality: `path == literal`
    Literal {
        path: AccessPath,
        go_literal: String,
    },
    /// Exact slice length: `len(path) == length`
    SliceLenEq { path: AccessPath, length: usize },
    /// Minimum slice length: `len(path) >= min_length`
    SliceLenGe { path: AccessPath, min_length: usize },
    /// Or-pattern: at least one alternative's checks must all pass.
    /// Each inner `Vec<Check>` is one alternative (checks ANDed together);
    /// the alternatives are ORed.
    Or { alternatives: Vec<Vec<Check>> },
    /// Go interface type assertion: the value at `path` implements `go_type`.
    /// Emitted as a case label in a `switch x := x.(type)` statement.
    TypeAssert { path: AccessPath, go_type: String },
}

impl Check {
    /// Render this check as a Go boolean expression.
    pub(crate) fn render(&self, subject: &str) -> String {
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

    /// Render the negation of this check as a Go boolean expression.
    /// Comparison-shaped checks flip their operator; Or and TypeAssert
    /// have no clean local negation, so they wrap in `!(...)`.
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

    /// Returns the access path being checked (for switch grouping).
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

    /// Returns the tag constant if this is an EnumTag check.
    pub(crate) fn as_enum_tag(&self) -> Option<(&str, bool)> {
        match self {
            Check::EnumTag {
                tag_constant,
                needs_stdlib,
                ..
            } => Some((tag_constant, *needs_stdlib)),
            _ => None,
        }
    }

    /// Returns the literal value if this is a Literal check.
    pub(crate) fn as_literal(&self) -> Option<&str> {
        match self {
            Check::Literal { go_literal, .. } => Some(go_literal),
            _ => None,
        }
    }
}

/// A variable binding produced by a pattern match.
#[derive(Clone, Debug)]
pub(crate) struct PatternBinding {
    /// The Lisette identifier name (for bindings registration).
    pub lisette_name: String,
    /// The Go variable name, or None if unused (emit `_` or skip).
    pub go_name: Option<String>,
    /// How to access the value from the match subject.
    pub path: AccessPath,
}

/// Root-path Go-interface type assertion lifted out of `checks`.
#[derive(Clone, Debug)]
pub(crate) struct TypeAssertion {
    pub path: AccessPath,
    pub go_types: Vec<String>,
}

/// Result of collecting checks and bindings from a single pattern.
pub(crate) struct PatternInfo {
    pub root_assertion: Option<TypeAssertion>,
    pub checks: Vec<Check>,
    pub bindings: Vec<PatternBinding>,
    pub effects: PatternEffects,
}

impl PatternInfo {
    /// True when a downstream consumer will reference the asserted value.
    fn requires_asserted_subject(&self) -> bool {
        !self.checks.is_empty() || self.bindings.iter().any(|b| b.go_name.is_some())
    }
}

/// Side effects (stdlib gate + Go imports) accumulated while walking a
/// pattern. Alias for the crate-wide `EmitEffects` so analysis helpers
/// outside `patterns/` can produce the same shape.
pub(crate) type PatternEffects = crate::EmitEffects;

/// Result of compiling a list of expanded arms into a `Decision`.
pub(crate) struct CompiledDecision {
    pub decision: Decision,
    pub effects: PatternEffects,
}

/// Accumulates checks, bindings, and effects during pattern compilation.
struct PatternCollector {
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
    effects: PatternEffects,
}

impl PatternCollector {
    fn new() -> Self {
        Self {
            checks: Vec::new(),
            bindings: Vec::new(),
            effects: PatternEffects::default(),
        }
    }
}

/// A pre-computed decision tree for pattern matching.
///
/// Each node represents either a successful match, a runtime test, or
/// unreachable code. The tree is walked by `TreeEmitter` to produce Go code.
#[derive(Debug)]
pub(crate) enum Decision {
    /// Pattern matched — emit bindings and arm body.
    Success {
        arm_index: usize,
        bindings: Vec<PatternBinding>,
    },
    /// Test a guard expression. If true, emit the guarded arm body.
    /// If false, continue with the failure subtree.
    Guard {
        arm_index: usize,
        bindings: Vec<PatternBinding>,
        success: Box<Decision>,
        failure: Box<Decision>,
    },
    /// Branch on a value — emits as Go `switch` when eligible.
    Switch {
        path: AccessPath,
        kind: SwitchKind,
        shape: SwitchShape,
        branches: Vec<SwitchBranch>,
        fallback: Option<Box<Decision>>,
    },
    /// Sequential tests — emits as if/else if/else chain.
    Chain {
        tests: Vec<ChainTest>,
        fallback: Box<Decision>,
    },
    /// Unreachable code — emits `panic("unreachable")` in tail position.
    Unreachable,
}

/// What kind of switch to emit.
#[derive(Debug, Clone)]
pub(crate) enum SwitchKind {
    /// Switch on `.Tag` — enum discriminant
    EnumTag,
    /// Switch on value directly — literals, booleans, units
    Value,
    /// Switch on dynamic Go type — `switch x := x.(type)`
    TypeSwitch,
}

/// Structural shape of a switch site, chosen at build time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SwitchShape {
    /// Type switch (case labels are Go type names).
    TypeSwitch,
    /// Two branches, `"true"`/`"false"` labels, no fallback, `kind == Value`.
    Bool,
    /// Two branches, no fallback, not Bool.
    Binary,
    /// One branch (any fallback presence).
    SingleArm,
    /// Everything else.
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

/// True when a decision tree has an unconditional success path.
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

/// A single branch in a Switch node.
#[derive(Debug)]
pub(crate) struct SwitchBranch {
    /// The case label (tag constant for enums, literal value for values)
    pub case_label: String,
    pub needs_stdlib: bool,
    pub decision: Decision,
}

/// A single test in a Chain node.
#[derive(Debug)]
pub(crate) struct ChainTest {
    pub checks: Vec<Check>,
    pub decision: Decision,
}

/// Intermediate representation of a single arm during compilation.
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

    // If the first arm has no checks (catchall), it matches unconditionally
    if first_is_catchall && !arms[0].has_guard {
        return Decision::Success {
            arm_index: arms[0].arm_index,
            bindings: arms[0].bindings.clone(),
        };
    }

    // If the first arm has no checks but has a guard, wrap in Guard node
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

    let (kind, switch_path) = if let Some(assertion) = &first_relevant.root_assertion {
        (SwitchKind::TypeSwitch, assertion.path.clone())
    } else {
        let first_check = first_relevant.checks.first()?;
        if first_check.as_enum_tag().is_some() {
            (SwitchKind::EnumTag, first_check.path()?.clone())
        } else if first_check.as_literal().is_some() {
            (SwitchKind::Value, first_check.path()?.clone())
        } else {
            return None;
        }
    };

    for arm in arms {
        if arm.is_catchall() {
            continue;
        }
        if arm.has_guard && !matches!(kind, SwitchKind::TypeSwitch) {
            return None;
        }

        let arm_path = match &kind {
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
        if arm_path != &switch_path {
            return None;
        }
    }

    let mut branch_map: HashMap<String, (bool, Vec<ArmInfo>)> = HashMap::default();
    let mut branch_order: Vec<String> = Vec::new();
    let mut fallback_arms = Vec::new();

    for arm in arms {
        if arm.is_catchall() {
            fallback_arms.push(arm.clone());
            continue;
        }

        let (case_label, needs_stdlib, inner_checks) = match &kind {
            SwitchKind::EnumTag => {
                let (tag, needs) = arm.checks[0].as_enum_tag().unwrap();
                (tag.to_string(), needs, arm.checks[1..].to_vec())
            }
            SwitchKind::Value => {
                let lit = arm.checks[0].as_literal().unwrap();
                (lit.to_string(), false, arm.checks[1..].to_vec())
            }
            SwitchKind::TypeSwitch => {
                let assertion = arm.root_assertion.as_ref().unwrap();
                (assertion.go_types.join(", "), false, arm.checks.clone())
            }
        };
        let inner_arm = ArmInfo {
            arm_index: arm.arm_index,
            root_assertion: None,
            checks: inner_checks,
            bindings: arm.bindings.clone(),
            has_guard: arm.has_guard,
        };

        branch_map
            .entry(case_label.clone())
            .and_modify(|(_, arms)| arms.push(inner_arm.clone()))
            .or_insert_with(|| {
                branch_order.push(case_label);
                (needs_stdlib, vec![inner_arm])
            });
    }

    let branches: Vec<SwitchBranch> = branch_order
        .into_iter()
        .map(|label| {
            let (needs_stdlib, inner_arms) = branch_map.remove(&label).unwrap();
            // For type switches: if any inner arm can fail (via guard or remaining
            // checks), the catchall arms must be appended so the case body stays
            // exhaustive — Go type-switch cases don't fall through automatically.
            let any_inner_can_fail = inner_arms
                .iter()
                .any(|a| a.has_guard || !a.checks.is_empty());
            let decision = if matches!(kind, SwitchKind::TypeSwitch) && any_inner_can_fail {
                let mut arms_with_fallback = inner_arms;
                arms_with_fallback.extend(fallback_arms.iter().cloned());
                build_tree(arms_with_fallback)
            } else {
                build_tree(inner_arms)
            };
            SwitchBranch {
                case_label: label,
                needs_stdlib,
                decision,
            }
        })
        .collect();

    let fallback = if fallback_arms.is_empty() {
        None
    } else {
        Some(Box::new(build_tree(fallback_arms)))
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

/// Build a Chain (if/else if/else) from the arms.
fn build_chain(arms: Vec<ArmInfo>) -> Decision {
    let mut tests = Vec::new();

    for (i, arm) in arms.iter().enumerate() {
        if arm.is_catchall() && !arm.has_guard {
            // This is a catchall — everything after it is unreachable
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

    // No catchall found — remaining arms are all checked
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
    emitter: &Emitter,
    path: &AccessPath,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    match pattern {
        Pattern::WildCard { .. } | Pattern::Unit { .. } => {}

        Pattern::Identifier { identifier, .. } => {
            let go_name = emitter.go_name_for_binding(pattern);
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
            collect_enum_variant_checks(emitter, path, pattern, typed, path_ty, collector);
        }

        Pattern::Struct { .. } => {
            collect_struct_checks(emitter, path, pattern, typed, path_ty, collector);
        }

        Pattern::Tuple { elements, .. } => {
            let typed_elements: Vec<Option<&TypedPattern>> = match typed {
                Some(TypedPattern::Tuple { elements: te, .. }) => te.iter().map(Some).collect(),
                _ => vec![None; elements.len()],
            };

            let stripped_path_ty = path_ty.map(Type::strip_refs);
            let element_tys: Option<&[Type]> = match &stripped_path_ty {
                Some(Type::Tuple(tys)) => Some(tys.as_slice()),
                _ => None,
            };

            for (i, element) in elements.iter().enumerate() {
                let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
                let field_path = path.push(PathSegment::Field(field_name.to_string()));
                collect_checks_and_bindings(
                    emitter,
                    &field_path,
                    element,
                    typed_elements.get(i).copied().flatten(),
                    element_tys.and_then(|tys| tys.get(i)),
                    collector,
                );
            }
        }

        Pattern::Slice { prefix, rest, .. } => {
            let has_rest = rest.is_present();
            if has_rest {
                if !prefix.is_empty() {
                    collector.checks.push(Check::SliceLenGe {
                        path: path.clone(),
                        min_length: prefix.len(),
                    });
                }
            } else {
                collector.checks.push(Check::SliceLenEq {
                    path: path.clone(),
                    length: prefix.len(),
                });
            }

            let typed_prefix: Vec<Option<&TypedPattern>> = match typed {
                Some(TypedPattern::Slice {
                    prefix: tp_prefix, ..
                }) => tp_prefix.iter().map(Some).collect(),
                _ => vec![None; prefix.len()],
            };

            for (i, elem) in prefix.iter().enumerate() {
                let elem_path = path.push(PathSegment::Index(i));
                collect_checks_and_bindings(
                    emitter,
                    &elem_path,
                    elem,
                    typed_prefix.get(i).copied().flatten(),
                    None,
                    collector,
                );
            }

            // Rest binding
            if let RestPattern::Bind { name, .. } = rest {
                let go_name = emitter.go_name_for_rest_binding(rest);
                collector.bindings.push(PatternBinding {
                    lisette_name: name.to_string(),
                    go_name,
                    path: path.push(PathSegment::SliceFrom(prefix.len())),
                });
            }
        }

        Pattern::Or { patterns, .. } => {
            collect_or_pattern_checks(emitter, path, patterns, typed, pattern, path_ty, collector);
        }

        p @ Pattern::AsBinding {
            pattern: inner,
            name,
            ..
        } => {
            collect_checks_and_bindings(emitter, path, inner, typed, path_ty, collector);
            let go_name = emitter.go_name_for_binding(p);
            collector.bindings.push(PatternBinding {
                lisette_name: name.to_string(),
                go_name,
                path: path.clone(),
            });
        }
    }
}

/// Handle or-patterns without bindings by collecting conditions from each
/// alternative and combining with `||`.
fn collect_or_pattern_checks(
    emitter: &Emitter,
    path: &AccessPath,
    patterns: &[Pattern],
    typed: Option<&TypedPattern>,
    pattern: &Pattern,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let has_bindings = Emitter::pattern_has_bindings(pattern);
    if !has_bindings {
        let typed_alternatives: Vec<Option<&TypedPattern>> = match typed {
            Some(TypedPattern::Or { alternatives }) => alternatives.iter().map(Some).collect(),
            _ => vec![None; patterns.len()],
        };

        let alt_collectors: Vec<PatternCollector> = patterns
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut alt_collector = PatternCollector::new();
                let tc = typed_alternatives.get(i).copied().flatten();
                collect_checks_and_bindings(emitter, path, p, tc, path_ty, &mut alt_collector);
                alt_collector
            })
            .collect();

        if alt_collectors.iter().any(|c| c.checks.is_empty()) {
            return;
        }

        for alt in &alt_collectors {
            collector.effects.extend(&alt.effects);
        }
        collector.checks.push(Check::Or {
            alternatives: alt_collectors.into_iter().map(|c| c.checks).collect(),
        });
    }
}

/// Compute the access path for a struct field, handling enum struct variants
/// and auto-pointer dereference.
fn compute_struct_field_path(
    emitter: &Emitter,
    parent_path: &AccessPath,
    field: &StructFieldPattern,
    ty: &Type,
    enum_info: Option<&(String, String)>,
    typed_variant_fields: Option<&[syntax::ast::EnumFieldDefinition]>,
) -> AccessPath {
    let go_field_name = if let Some((enum_id, variant_name)) = enum_info {
        emitter
            .enum_struct_field_name(enum_id, variant_name, &field.name)
            .unwrap_or_else(|| {
                panic!(
                    "enum layout not found: {}.{}.{}",
                    enum_id, variant_name, field.name
                )
            })
    } else if emitter.field_is_public(ty, &field.name) {
        go_name::make_exported(&field.name)
    } else {
        go_name::escape_keyword(&field.name).into_owned()
    };

    if let Some((_, variant_name)) = enum_info
        && let Some(field_index) =
            emitter.get_enum_struct_field_index(ty, variant_name, &field.name)
    {
        let is_source_ref = typed_variant_fields
            .and_then(|vf| vf.get(field_index).map(|f| f.ty.is_ref()))
            .unwrap_or_else(|| emitter.is_enum_field_source_ref(ty, variant_name, field_index));
        let is_auto_pointer =
            emitter.is_enum_field_pointer(ty, variant_name, field_index) && !is_source_ref;
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
    emitter: &Emitter,
    path: &AccessPath,
    pattern_ty: &Type,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) -> Option<AccessPath> {
    path_ty.filter(|st| emitter.facts.as_interface(st).is_some())?;
    let go_type_result = emitter.go_type(pattern_ty);
    collector.effects.merge_from_go_type(&go_type_result);
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
    emitter: &Emitter,
    path: &AccessPath,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let Pattern::EnumVariant {
        identifier,
        fields,
        ty,
        ..
    } = pattern
    else {
        return;
    };

    let (typed_children, typed_variant_fields) = match typed {
        Some(TypedPattern::EnumVariant {
            fields: tf,
            variant_fields: vf,
            ..
        }) => (tf.iter().map(Some).collect::<Vec<_>>(), Some(vf.as_slice())),
        _ => (vec![None; fields.len()], None),
    };

    let variant_data = EnumVariantData {
        identifier,
        fields,
        ty,
        typed_children: &typed_children,
        typed_variant_fields,
    };

    if emitter.is_tuple_struct_type(ty) {
        let child_path = interface_assert_child_path(emitter, path, ty, path_ty, collector)
            .unwrap_or_else(|| path.clone());
        if emitter.is_newtype_struct(ty) {
            collect_newtype_checks(emitter, &child_path, &variant_data, collector);
        } else {
            collect_tuple_struct_checks(emitter, &child_path, fields, &typed_children, collector);
        }
        return;
    }

    if emitter.is_go_value_enum(ty) {
        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return;
        };
        let variant_name = go_name::unqualified_name(identifier);
        let module = go_name::module_of_type_id(id.as_str());
        let qualifier = emitter.go_pkg_qualifier(module);
        let go_literal = if qualifier.is_empty() || qualifier == emitter.facts.current_module() {
            variant_name.to_string()
        } else {
            collector
                .effects
                .require_go_import(emitter.go_import_path_for_module(module));
            format!("{}.{}", qualifier, variant_name)
        };
        collector.checks.push(Check::Literal {
            path: path.clone(),
            go_literal,
        });
        return;
    }

    if emitter.as_enum(ty).is_none() && identifier.contains('.') {
        if let Some((module, _)) = identifier.split_once('.')
            && emitter.facts.is_foreign_module(module)
        {
            collector
                .effects
                .require_go_import(emitter.go_import_path_for_module(module));
        }
        collector.checks.push(Check::Literal {
            path: path.clone(),
            go_literal: identifier.to_string(),
        });
        return;
    }

    collect_tagged_enum_checks(emitter, path, &variant_data, collector);
}

/// Collect checks and bindings for a newtype struct pattern (single-field wrapper).
fn collect_newtype_checks(
    emitter: &Emitter,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
) {
    let Some(underlying_ty) = emitter.get_newtype_underlying(variant.ty) else {
        return;
    };
    let go_underlying = emitter.go_type(&underlying_ty);
    collector.effects.merge_from_go_type(&go_underlying);
    let field_path = path.push(PathSegment::NewtypeCast(go_underlying.code));
    if let Some(field) = variant.fields.first() {
        collect_checks_and_bindings(
            emitter,
            &field_path,
            field,
            variant.typed_children.first().copied().flatten(),
            None,
            collector,
        );
    }
}

/// Collect checks and bindings for a tuple struct pattern (positional fields).
fn collect_tuple_struct_checks(
    emitter: &Emitter,
    path: &AccessPath,
    fields: &[Pattern],
    typed_children: &[Option<&TypedPattern>],
    collector: &mut PatternCollector,
) {
    for (i, field) in fields.iter().enumerate() {
        let field_path = path.push(PathSegment::Field(format!("F{}", i)));
        collect_checks_and_bindings(
            emitter,
            &field_path,
            field,
            typed_children.get(i).copied().flatten(),
            None,
            collector,
        );
    }
}

struct EnumVariantData<'a> {
    identifier: &'a str,
    fields: &'a [Pattern],
    ty: &'a Type,
    typed_children: &'a [Option<&'a TypedPattern>],
    typed_variant_fields: Option<&'a [syntax::ast::EnumFieldDefinition]>,
}

/// Collect checks and bindings for a tagged enum variant pattern.
fn collect_tagged_enum_checks(
    emitter: &Emitter,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
) {
    let alias = emitter.module_alias_for_type(variant.ty);
    let resolved = go_name::variant(
        variant.identifier,
        variant.ty,
        emitter.facts.current_module(),
        alias.as_deref(),
    );
    if resolved.needs_stdlib {
        collector.effects.needs_stdlib = true;
    }
    collector.checks.push(Check::EnumTag {
        path: path.clone(),
        tag_constant: resolved.name.clone(),
        needs_stdlib: resolved.needs_stdlib,
    });

    let variant_name = variant
        .identifier
        .split('.')
        .next_back()
        .unwrap_or(variant.identifier);
    for (i, field) in variant.fields.iter().enumerate() {
        let field_name = emitter.get_enum_tuple_field_name(variant.ty, variant_name, i);

        let is_source_ref = variant
            .typed_variant_fields
            .and_then(|vf| vf.get(i).map(|f| f.ty.is_ref()))
            .unwrap_or_else(|| emitter.is_enum_field_source_ref(variant.ty, variant_name, i));
        let is_auto_pointer =
            emitter.is_enum_field_pointer(variant.ty, variant_name, i) && !is_source_ref;

        let is_unit = emitter.is_enum_field_unit(variant.ty, variant_name, i);

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
                emitter,
                &field_path,
                field,
                variant.typed_children.get(i).copied().flatten(),
                None,
                collector,
            );
        }
    }
}

/// Collect checks and bindings for a struct pattern (plain struct or enum struct variant).
/// Detect whether a struct pattern is actually an enum struct variant,
/// returning `(enum_id, variant_name)` if so.
fn detect_enum_info(
    emitter: &Emitter,
    ty: &Type,
    identifier: &str,
    typed: Option<&TypedPattern>,
) -> Option<(String, String)> {
    match typed {
        Some(TypedPattern::EnumStructVariant {
            variant_name: vn, ..
        }) => {
            let variant_name_str = unqualified_name(vn);
            let id = emitter.as_enum(ty).unwrap_or_else(|| {
                vn.rsplit_once('.')
                    .map_or(vn.to_string(), |(e, _)| e.to_string())
            });
            Some((id, variant_name_str.to_string()))
        }
        Some(TypedPattern::Struct { .. }) => None,
        _ => emitter.as_enum(ty).map(|id| {
            let variant_name_str = unqualified_name(identifier);
            (id, variant_name_str.to_string())
        }),
    }
}

fn collect_struct_checks(
    emitter: &Emitter,
    path: &AccessPath,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
    path_ty: Option<&Type>,
    collector: &mut PatternCollector,
) {
    let Pattern::Struct {
        fields,
        ty,
        identifier,
        ..
    } = pattern
    else {
        return;
    };

    let (enum_info, child_path) = if let Some(asserted) =
        interface_assert_child_path(emitter, path, ty, path_ty, collector)
    {
        (None, asserted)
    } else {
        let enum_info = detect_enum_info(emitter, ty, identifier, typed);
        if enum_info.is_some() {
            let alias = emitter.module_alias_for_type(ty);
            let resolved = go_name::variant(
                identifier,
                ty,
                emitter.facts.current_module(),
                alias.as_deref(),
            );
            if resolved.needs_stdlib {
                collector.effects.needs_stdlib = true;
            }
            collector.checks.push(Check::EnumTag {
                path: path.clone(),
                tag_constant: resolved.name.clone(),
                needs_stdlib: resolved.needs_stdlib,
            });
        }
        (enum_info, path.clone())
    };

    let typed_fields_map: Option<Vec<(&str, Option<&TypedPattern>)>> = match typed {
        Some(TypedPattern::Struct { pattern_fields, .. })
        | Some(TypedPattern::EnumStructVariant { pattern_fields, .. }) => Some(
            pattern_fields
                .iter()
                .map(|(name, tp)| (name.as_str(), Some(tp)))
                .collect(),
        ),
        _ => None,
    };

    let typed_variant_fields = match typed {
        Some(TypedPattern::EnumStructVariant { variant_fields, .. }) => {
            Some(variant_fields.as_slice())
        }
        _ => None,
    };

    let field_tys: Vec<(&str, &Type)> = match typed {
        Some(TypedPattern::Struct { struct_fields, .. }) => struct_fields
            .iter()
            .map(|f| (f.name.as_str(), &f.ty))
            .collect(),
        Some(TypedPattern::EnumStructVariant { variant_fields, .. }) => variant_fields
            .iter()
            .map(|f| (f.name.as_str(), &f.ty))
            .collect(),
        _ => Vec::new(),
    };

    for field in fields {
        let typed_child = typed_fields_map
            .as_ref()
            .and_then(|m| m.iter().find(|(name, _)| *name == field.name))
            .and_then(|(_, tp)| *tp);

        let field_path = compute_struct_field_path(
            emitter,
            &child_path,
            field,
            ty,
            enum_info.as_ref(),
            typed_variant_fields,
        );
        let field_ty = field_tys
            .iter()
            .find(|(name, _)| *name == field.name)
            .map(|(_, ty)| *ty);
        collect_checks_and_bindings(
            emitter,
            &field_path,
            &field.value,
            typed_child,
            field_ty,
            collector,
        );
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
            && Emitter::pattern_has_bindings(&arm.pattern)
        {
            let typed_alternatives: Vec<Option<&TypedPattern>> =
                if let Some(TypedPattern::Or { alternatives }) = &arm.typed_pattern {
                    alternatives.iter().map(Some).collect()
                } else {
                    vec![None; patterns.len()]
                };
            for (j, alt) in patterns.iter().enumerate() {
                result.push(ExpandedArm {
                    arm_index: i,
                    pattern: alt,
                    typed_pattern: typed_alternatives.get(j).copied().flatten(),
                    has_guard: arm.has_guard(),
                });
            }
            continue;
        }
        result.push(ExpandedArm {
            arm_index: i,
            pattern: &arm.pattern,
            typed_pattern: arm.typed_pattern.as_ref(),
            has_guard: arm.has_guard(),
        });
    }
    result
}

/// An expanded arm reference, possibly one alternative of an or-pattern.
pub(super) struct ExpandedArm<'a> {
    pub arm_index: usize,
    pub pattern: &'a Pattern,
    pub typed_pattern: Option<&'a TypedPattern>,
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

/// Compile expanded arms into a decision tree plus accumulated effects.
pub(super) fn compile_expanded_arms<'a>(
    emitter: &Emitter,
    expanded: &'a [ExpandedArm<'a>],
    subject_ty: &Type,
) -> CompiledDecision {
    let mut effects = PatternEffects::default();
    let arm_infos: Vec<ArmInfo> = expanded
        .iter()
        .map(|ea| {
            let info = collect_pattern_info(emitter, ea.pattern, ea.typed_pattern, subject_ty);
            effects.extend(&info.effects);
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
        effects,
    }
}

/// Collect checks, bindings, and any root type assertion from a single pattern
/// for use outside match emission (let-else, while-let, for-loop, complex let,
/// function param). `subject_ty` is the static type of the scrutinee; sites
/// that pass a wrong type would silently miss root-level Go-interface type
/// assertions, so the public entry takes it by reference rather than `Option`.
pub(crate) fn collect_pattern_info(
    emitter: &Emitter,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
    subject_ty: &Type,
) -> PatternInfo {
    let mut collector = PatternCollector::new();
    collect_checks_and_bindings(
        emitter,
        &AccessPath::root(),
        pattern,
        typed,
        Some(subject_ty),
        &mut collector,
    );
    let root_assertion = extract_root_assertion(&mut collector.checks);
    PatternInfo {
        root_assertion,
        checks: collector.checks,
        bindings: collector.bindings,
        effects: collector.effects,
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

/// Hoist a root type assertion as `asserted := subject.(T)` for irrefutable
/// destructure paths; the pattern compiler has already verified the type.
pub(super) fn apply_root_assertion<'s>(
    emitter: &mut Emitter,
    output: &mut String,
    info: &PatternInfo,
    subject: &'s str,
) -> std::borrow::Cow<'s, str> {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return std::borrow::Cow::Borrowed(subject);
    };
    if !info.requires_asserted_subject() {
        return std::borrow::Cow::Borrowed(subject);
    }
    let [go_type] = assertion.go_types.as_slice() else {
        unreachable!("multi-type root assertions only reach match destructure paths")
    };
    let expression = format!("{}.({})", subject, go_type);
    let var = emitter.hoist_tmp_value(output, "asserted", &expression);
    std::borrow::Cow::Owned(var)
}

/// Hoist a root type assertion as comma-ok for refutable contexts (while-let,
/// select arms, or-pattern let-else). Returns `(effective_subject, ok_var)`.
pub(super) fn apply_refutable_root_assertion<'s>(
    emitter: &mut Emitter,
    output: &mut String,
    info: &PatternInfo,
    subject: &'s str,
) -> (std::borrow::Cow<'s, str>, Option<String>) {
    let Some(assertion) = info.root_assertion.as_ref() else {
        return (std::borrow::Cow::Borrowed(subject), None);
    };
    let needs_asserted = info.requires_asserted_subject();
    match assertion.go_types.as_slice() {
        [go_type] => {
            let asserted_lhs = if needs_asserted {
                let v = emitter.fresh_var(Some("asserted"));
                emitter.declare(&v);
                v
            } else {
                "_".to_string()
            };
            let ok = emitter.fresh_var(Some("ok"));
            emitter.declare(&ok);
            write_line!(
                output,
                "{}, {} := {}.({})",
                asserted_lhs,
                ok,
                subject,
                go_type
            );
            let effective = if needs_asserted {
                std::borrow::Cow::Owned(asserted_lhs)
            } else {
                std::borrow::Cow::Borrowed(subject)
            };
            (effective, Some(ok))
        }
        multiple => {
            // No-binding interface or-pattern (`A | B`): no single asserted
            // form is possible across types.
            let oks: Vec<String> = multiple
                .iter()
                .map(|t| {
                    let ok = emitter.fresh_var(Some("ok"));
                    emitter.declare(&ok);
                    write_line!(output, "_, {} := {}.({})", ok, subject, t);
                    ok
                })
                .collect();
            (
                std::borrow::Cow::Borrowed(subject),
                Some(format!("({})", oks.join(" || "))),
            )
        }
    }
}

/// Combine an optional `ok` variable with rendered checks into a guard
/// condition; returns `"true"` when both are absent.
pub(super) fn compose_refutable_condition(
    ok_var: Option<&str>,
    checks: &[Check],
    effective_subject: &str,
) -> String {
    let cond = render_condition(checks, effective_subject);
    match ok_var {
        None => cond,
        Some(ok) if cond == "true" => ok.to_string(),
        Some(ok) => format!("{} && {}", ok, cond),
    }
}

/// Render checks as a Go condition string.
pub(super) fn render_condition(checks: &[Check], subject_var: &str) -> String {
    if checks.is_empty() {
        return "true".to_string();
    }

    let conditions: Vec<String> = checks.iter().map(|c| c.render(subject_var)).collect();

    conditions.join(" && ")
}

pub(crate) fn emit_tree_bindings(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
) {
    emit_tree_bindings_with_consumers(emitter, output, bindings, subject_var, &[]);
}

pub(crate) fn emit_tree_bindings_with_consumers(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
    consumers: &[&syntax::ast::Expression],
) -> Vec<(String, Option<crate::bindings::BindingValue>)> {
    let mut installed_inlines = Vec::new();
    for binding in bindings {
        let Some(ref go_name) = binding.go_name else {
            emitter.scope.bind(&binding.lisette_name, "");
            continue;
        };

        let access_expression = binding.path.render(subject_var);

        if !consumers.is_empty()
            && crate::inline_uses::analyze_inline_candidate(&binding.lisette_name, consumers)
                == crate::inline_uses::InlineDecision::Inline
        {
            let previous = emitter
                .scope
                .resolve_identifier_binding(&binding.lisette_name)
                .cloned();
            let safe_text = binding.path.render_composable(subject_var);
            emitter.scope.bind_inline_expr(
                &binding.lisette_name,
                crate::bindings::InlineExpr::new(safe_text),
            );
            installed_inlines.push((binding.lisette_name.clone(), previous));
            continue;
        }

        if emitter.scope.has_binding_for_go_name(go_name) {
            let fresh = emitter.fresh_var(Some(&binding.lisette_name));
            emitter.scope.bind(&binding.lisette_name, &fresh);
            emitter.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, access_expression);
        } else {
            let name = emitter.scope.bind(&binding.lisette_name, go_name.clone());
            if emitter.try_declare(&name) {
                write_line!(output, "{} := {}", name, access_expression);
            } else {
                let fresh = emitter.fresh_var(Some(&binding.lisette_name));
                emitter.scope.bind(&binding.lisette_name, &fresh);
                emitter.try_declare(&fresh);
                write_line!(output, "{} := {}", fresh, access_expression);
            }
        }
    }
    installed_inlines
}

pub(crate) fn drop_inline_overlays(
    emitter: &mut Emitter,
    installed: &[(String, Option<crate::bindings::BindingValue>)],
) {
    for (name, previous) in installed {
        match previous {
            Some(crate::bindings::BindingValue::GoName(go)) => {
                emitter.scope.bind(name.as_str(), go.as_str());
            }
            Some(crate::bindings::BindingValue::InlineExpr(expr)) => {
                emitter.scope.bind_inline_expr(name.as_str(), expr.clone());
            }
            None => {
                emitter.scope.remove_binding(name);
            }
        }
    }
}

/// Emit bindings as Go `=` assignments (for pre-declared variables in or-patterns).
/// Only emits for bindings that are already registered in the bindings map
/// (i.e., pre-declared with `emit_binding_declarations_with_type`).
pub(super) fn emit_tree_assignments(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
) {
    for binding in bindings {
        if binding.go_name.is_none() {
            continue;
        }

        // Only assign to variables that were pre-declared as Go names
        let Some(registered_name) = emitter.scope.resolve_binding_go_name(&binding.lisette_name)
        else {
            continue;
        };
        let name = registered_name.to_string();
        let access_expression = binding.path.render(subject_var);
        write_line!(output, "{} = {}", name, access_expression);
    }
}
