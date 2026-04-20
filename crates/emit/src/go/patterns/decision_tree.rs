use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use syntax::ast::{MatchArm, Pattern, RestPattern, StructFieldPattern, TypedPattern};
use syntax::parse::TUPLE_FIELDS;
use syntax::types::Type;

use crate::Emitter;
use crate::go::names::go_name;
use crate::go::patterns::bindings::emit_pattern_literal;
use crate::go::write_line;

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
                PathSegment::NewtypeCast(ty) => result = format!("{}({})", ty, result),
            }
        }
        result
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
            // Type assertions are emitted via type switches, not boolean conditions.
            // This path is only reached when a guard prevents switch compilation.
            Check::TypeAssert { .. } => "false".to_string(),
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

    pub(crate) fn as_type_switch_case(&self) -> Option<(Vec<&str>, &AccessPath)> {
        match self {
            Check::TypeAssert { go_type, path } => Some((vec![go_type.as_str()], path)),
            Check::Or { alternatives } => {
                let [
                    Check::TypeAssert {
                        go_type,
                        path: shared_path,
                    },
                ] = alternatives.first()?.as_slice()
                else {
                    return None;
                };
                let mut labels = Vec::with_capacity(alternatives.len());
                labels.push(go_type.as_str());
                for alt in &alternatives[1..] {
                    let [Check::TypeAssert { go_type, path }] = alt.as_slice() else {
                        return None;
                    };
                    if path != shared_path {
                        return None;
                    }
                    labels.push(go_type.as_str());
                }
                Some((labels, shared_path))
            }
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

/// Accumulates checks and bindings during pattern compilation.
struct PatternCollector {
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
}

impl PatternCollector {
    fn new() -> Self {
        Self {
            checks: Vec::new(),
            bindings: Vec::new(),
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
    checks: Vec<Check>,
    bindings: Vec<PatternBinding>,
    has_guard: bool,
}

/// Build a Decision tree from a list of arm infos.
fn build_tree(arms: Vec<ArmInfo>) -> Decision {
    if arms.is_empty() {
        return Decision::Unreachable;
    }

    // If the first arm has no checks (catchall), it matches unconditionally
    if arms[0].checks.is_empty() && !arms[0].has_guard {
        return Decision::Success {
            arm_index: arms[0].arm_index,
            bindings: arms[0].bindings.clone(),
        };
    }

    // If the first arm has no checks but has a guard, wrap in Guard node
    if arms[0].checks.is_empty() && arms[0].has_guard {
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
/// Returns Some(Switch) if ALL non-catchall arms have a single switchable
/// check (EnumTag or Literal) on the same path, with no guards.
fn try_build_switch(arms: &[ArmInfo]) -> Option<Decision> {
    let first_checked_arm = arms.iter().find(|a| !a.checks.is_empty())?;
    let first_check = first_checked_arm.checks.first()?;

    let (kind, switch_path) = if first_check.as_enum_tag().is_some() {
        (SwitchKind::EnumTag, first_check.path()?.clone())
    } else if first_check.as_literal().is_some() {
        (SwitchKind::Value, first_check.path()?.clone())
    } else if let Some((_, path)) = first_check.as_type_switch_case() {
        (SwitchKind::TypeSwitch, path.clone())
    } else {
        return None;
    };

    for arm in arms {
        if arm.checks.is_empty() {
            continue;
        }
        // Type switches handle guards inside the case body; other switches cannot.
        if arm.has_guard && !matches!(kind, SwitchKind::TypeSwitch) {
            return None;
        }
        let first = arm.checks.first()?;

        let arm_path = match &kind {
            SwitchKind::EnumTag => {
                first.as_enum_tag()?;
                first.path()?
            }
            SwitchKind::Value => {
                first.as_literal()?;
                if arm.checks.len() != 1 {
                    return None;
                }
                first.path()?
            }
            SwitchKind::TypeSwitch => first.as_type_switch_case()?.1,
        };
        if arm_path != &switch_path {
            return None;
        }
    }

    let mut branch_map: HashMap<String, (bool, Vec<ArmInfo>)> = HashMap::default();
    let mut branch_order: Vec<String> = Vec::new();
    let mut fallback_arms = Vec::new();

    for arm in arms {
        if arm.checks.is_empty() {
            fallback_arms.push(arm.clone());
            continue;
        }

        let first_check = &arm.checks[0];
        let (case_label, needs_stdlib) = match &kind {
            SwitchKind::EnumTag => {
                let (tag, needs) = first_check.as_enum_tag().unwrap();
                (tag.to_string(), needs)
            }
            SwitchKind::Value => {
                let lit = first_check.as_literal().unwrap();
                (lit.to_string(), false)
            }
            SwitchKind::TypeSwitch => {
                let (labels, _) = first_check.as_type_switch_case().unwrap();
                (labels.join(", "), false)
            }
        };

        let inner_arm = ArmInfo {
            arm_index: arm.arm_index,
            checks: arm.checks[1..].to_vec(),
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

    let branches = branch_order
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

    Some(Decision::Switch {
        path: switch_path,
        kind,
        branches,
        fallback,
    })
}

/// Build a Chain (if/else if/else) from the arms.
fn build_chain(arms: Vec<ArmInfo>) -> Decision {
    let mut tests = Vec::new();

    for (i, arm) in arms.iter().enumerate() {
        if arm.checks.is_empty() && !arm.has_guard {
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

        tests.push(ChainTest {
            checks: arm.checks.clone(),
            decision,
        });
    }

    // No catchall found — remaining arms are all checked
    Decision::Chain {
        tests,
        fallback: Box::new(Decision::Unreachable),
    }
}

/// Recursively walk a pattern, collecting checks and bindings.
///
/// `path_ty` is the expected type of the value at `path` — used to detect
/// when a struct pattern is matched against a Go interface (type switch).
fn collect_checks_and_bindings(
    emitter: &mut Emitter,
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
            collect_enum_variant_checks(emitter, path, pattern, typed, collector);
        }

        Pattern::Struct { .. } => {
            collect_struct_checks(emitter, path, pattern, typed, path_ty, collector);
        }

        Pattern::Tuple { elements, .. } => {
            let typed_elements: Vec<Option<&TypedPattern>> = match typed {
                Some(TypedPattern::Tuple { elements: te, .. }) => te.iter().map(Some).collect(),
                _ => vec![None; elements.len()],
            };

            for (i, element) in elements.iter().enumerate() {
                let field_name = TUPLE_FIELDS.get(i).expect("oversize tuple arity");
                let field_path = path.push(PathSegment::Field(field_name.to_string()));
                collect_checks_and_bindings(
                    emitter,
                    &field_path,
                    element,
                    typed_elements.get(i).copied().flatten(),
                    None,
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
    emitter: &mut Emitter,
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

        let alternatives: Vec<Vec<Check>> = patterns
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut alt_collector = PatternCollector::new();
                let tc = typed_alternatives.get(i).copied().flatten();
                collect_checks_and_bindings(emitter, path, p, tc, path_ty, &mut alt_collector);
                alt_collector.checks
            })
            .collect();

        if alternatives.iter().any(|checks| checks.is_empty()) {
            return;
        }

        collector.checks.push(Check::Or { alternatives });
    }
}

/// Compute the access path for a struct field, handling enum struct variants
/// and auto-pointer dereference.
fn compute_struct_field_path(
    emitter: &mut Emitter,
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

/// Collect checks and bindings for an enum variant pattern (tuple or tagged).
fn collect_enum_variant_checks(
    emitter: &mut Emitter,
    path: &AccessPath,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
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
        if emitter.is_newtype_struct(ty) {
            collect_newtype_checks(emitter, path, &variant_data, collector);
        } else {
            collect_tuple_struct_checks(emitter, path, fields, &typed_children, collector);
        }
        return;
    }

    if emitter.is_go_value_enum(ty) {
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
            return;
        };
        let variant_name = go_name::unqualified_name(identifier);
        let module = go_name::module_of_type_id(id.as_str());
        let qualifier = emitter.go_pkg_qualifier(module);
        let go_literal = if qualifier.is_empty() || qualifier == emitter.current_module() {
            variant_name.to_string()
        } else {
            format!("{}.{}", qualifier, variant_name)
        };
        collector.checks.push(Check::Literal {
            path: path.clone(),
            go_literal,
        });
        return;
    }

    if emitter.as_enum(ty).is_none() && identifier.contains('.') {
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
    emitter: &mut Emitter,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
) {
    let Some(underlying_ty) = emitter.get_newtype_underlying(variant.ty) else {
        return;
    };
    let go_underlying_ty = emitter.go_type_as_string(&underlying_ty);
    let field_path = path.push(PathSegment::NewtypeCast(go_underlying_ty));
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
    emitter: &mut Emitter,
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
    emitter: &mut Emitter,
    path: &AccessPath,
    variant: &EnumVariantData,
    collector: &mut PatternCollector,
) {
    let alias = emitter.module_alias_for_type(variant.ty);
    let resolved = go_name::variant(
        variant.identifier,
        variant.ty,
        emitter.current_module(),
        alias.as_deref(),
    );
    if resolved.needs_stdlib {
        emitter.flags.needs_stdlib = true;
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
    emitter: &mut Emitter,
    ty: &Type,
    identifier: &str,
    typed: Option<&TypedPattern>,
) -> Option<(String, String)> {
    match typed {
        Some(TypedPattern::EnumStructVariant {
            variant_name: vn, ..
        }) => {
            let variant_name_str = vn.split('.').next_back().unwrap_or(vn);
            let id = emitter.as_enum(ty).unwrap_or_else(|| {
                vn.rsplit_once('.')
                    .map_or(vn.to_string(), |(e, _)| e.to_string())
            });
            Some((id, variant_name_str.to_string()))
        }
        Some(TypedPattern::Struct { .. }) => None,
        _ => emitter.as_enum(ty).map(|id| {
            let variant_name_str = identifier.split('.').next_back().unwrap_or(identifier);
            (id, variant_name_str.to_string())
        }),
    }
}

fn collect_struct_checks(
    emitter: &mut Emitter,
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

    let enum_info = if path_ty.is_some_and(|st| emitter.as_interface(st).is_some()) {
        let go_type = emitter.go_type_as_string(ty);
        collector.checks.push(Check::TypeAssert {
            path: path.clone(),
            go_type,
        });
        None
    } else {
        let enum_info = detect_enum_info(emitter, ty, identifier, typed);
        if enum_info.is_some() {
            let alias = emitter.module_alias_for_type(ty);
            let resolved =
                go_name::variant(identifier, ty, emitter.current_module(), alias.as_deref());
            if resolved.needs_stdlib {
                emitter.flags.needs_stdlib = true;
            }
            collector.checks.push(Check::EnumTag {
                path: path.clone(),
                tag_constant: resolved.name.clone(),
                needs_stdlib: resolved.needs_stdlib,
            });
        }
        enum_info
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

    for field in fields {
        let typed_child = typed_fields_map
            .as_ref()
            .and_then(|m| m.iter().find(|(name, _)| *name == field.name))
            .and_then(|(_, tp)| *tp);

        let field_path = compute_struct_field_path(
            emitter,
            path,
            field,
            ty,
            enum_info.as_ref(),
            typed_variant_fields,
        );
        collect_checks_and_bindings(
            emitter,
            &field_path,
            &field.value,
            typed_child,
            None,
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

/// Compile expanded arms into a decision tree.
pub(super) fn compile_expanded_arms<'a>(
    emitter: &mut Emitter,
    expanded: &'a [ExpandedArm<'a>],
    subject_ty: &Type,
) -> Decision {
    let arm_infos: Vec<ArmInfo> = expanded
        .iter()
        .map(|ea| {
            let mut collector = PatternCollector::new();
            collect_checks_and_bindings(
                emitter,
                &AccessPath::root(),
                ea.pattern,
                ea.typed_pattern,
                Some(subject_ty),
                &mut collector,
            );
            ArmInfo {
                arm_index: ea.arm_index,
                checks: collector.checks,
                bindings: collector.bindings,
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

    build_tree(arm_infos)
}

/// Collect checks and bindings from a single pattern for use outside match
/// emission (let-else, while-let, for-loop, complex let).
pub(crate) fn collect_pattern_info(
    emitter: &mut Emitter,
    pattern: &Pattern,
    typed: Option<&TypedPattern>,
) -> (Vec<Check>, Vec<PatternBinding>) {
    let mut collector = PatternCollector::new();
    collect_checks_and_bindings(
        emitter,
        &AccessPath::root(),
        pattern,
        typed,
        None,
        &mut collector,
    );
    (collector.checks, collector.bindings)
}

/// Render checks as a Go condition string.
pub(crate) fn render_condition(checks: &[Check], subject_var: &str) -> String {
    if checks.is_empty() {
        return "true".to_string();
    }

    let conditions: Vec<String> = checks.iter().map(|c| c.render(subject_var)).collect();

    conditions.join(" && ")
}

/// Emit bindings as Go `:=` declarations.
pub(crate) fn emit_tree_bindings(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
) {
    for binding in bindings {
        let Some(ref go_name) = binding.go_name else {
            emitter.scope.bindings.add(&binding.lisette_name, "");
            continue;
        };

        let access_expression = binding.path.render(subject_var);

        if emitter.scope.bindings.has_go_name(go_name) {
            let fresh = emitter.fresh_var(Some(&binding.lisette_name));
            emitter.scope.bindings.add(&binding.lisette_name, &fresh);
            emitter.try_declare(&fresh);
            write_line!(output, "{} := {}", fresh, access_expression);
        } else {
            let name = emitter
                .scope
                .bindings
                .add(&binding.lisette_name, go_name.clone());
            if emitter.try_declare(&name) {
                write_line!(output, "{} := {}", name, access_expression);
            } else {
                let fresh = emitter.fresh_var(Some(&binding.lisette_name));
                emitter.scope.bindings.add(&binding.lisette_name, &fresh);
                emitter.try_declare(&fresh);
                write_line!(output, "{} := {}", fresh, access_expression);
            }
        }
    }
}

/// Emit bindings as Go `=` assignments (for pre-declared variables in or-patterns).
/// Only emits for bindings that are already registered in the bindings map
/// (i.e., pre-declared with `emit_binding_declarations_with_type`).
pub(crate) fn emit_tree_assignments(
    emitter: &mut Emitter,
    output: &mut String,
    bindings: &[PatternBinding],
    subject_var: &str,
) {
    for binding in bindings {
        if binding.go_name.is_none() {
            continue;
        }

        // Only assign to variables that were pre-declared
        let Some(registered_name) = emitter.scope.bindings.get(&binding.lisette_name) else {
            continue;
        };
        let name = registered_name.to_string();
        let access_expression = binding.path.render(subject_var);
        write_line!(output, "{} = {}", name, access_expression);
    }
}
