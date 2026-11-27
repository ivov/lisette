use syntax::ast::Literal;
use syntax::types::Type;

use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::types::go_type::GoType;

use super::Check;

#[derive(Clone, Debug, PartialEq, Eq)]
enum PathSegment {
    Field(String),
    ValueField(String),
    TupleElement { index: usize, value_field: bool },
    Index(usize),
    SliceFrom(usize),
    ArraySliceFrom { offset: usize, go_type: GoType },
    Deref,
    NewtypeCast(GoType),
    AssertedAs(GoType),
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum SubjectRoot<'a> {
    Var(&'a GoExpression),
    Elements(&'a [GoExpression]),
    Narrowed {
        subject: &'a GoExpression,
        go_type: &'a GoType,
        value: &'a GoExpression,
    },
}

impl SubjectRoot<'_> {
    fn resolve<'p>(&self, segments: &'p [PathSegment]) -> (GoExpression, &'p [PathSegment]) {
        match self {
            Self::Var(var) => ((*var).clone(), segments),
            Self::Elements(elements) => {
                let Some(PathSegment::TupleElement { index, .. }) = segments.first() else {
                    unreachable!("a tuple-element subject only resolves tuple element paths")
                };
                (elements[*index].clone(), &segments[1..])
            }
            Self::Narrowed {
                subject,
                go_type,
                value,
            } => match segments.split_first() {
                Some((PathSegment::AssertedAs(asserted), rest)) if asserted == *go_type => {
                    ((*value).clone(), rest)
                }
                _ => ((*subject).clone(), segments),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::lowering) struct AccessPath {
    segments: Vec<PathSegment>,
}

impl AccessPath {
    pub(in crate::lowering) fn root() -> Self {
        Self { segments: vec![] }
    }

    pub(super) fn is_root(&self) -> bool {
        self.segments.is_empty()
    }

    fn push(&self, segment: PathSegment) -> Self {
        let mut new = self.clone();
        new.segments.push(segment);
        new
    }

    pub(in crate::lowering) fn render(&self, subject: SubjectRoot<'_>) -> GoExpression {
        let (root, segments) = subject.resolve(&self.segments);
        let mut result = root;
        for segment in segments {
            result = match segment {
                PathSegment::Field(name) => GoExpression::selector(result, name.clone()),
                PathSegment::ValueField(name) => GoExpression::value_field(result, name.clone()),
                PathSegment::TupleElement { index, value_field } => {
                    let name = go_name::tuple_field(*index).to_string();
                    if *value_field {
                        GoExpression::value_field(result, name)
                    } else {
                        GoExpression::selector(result, name)
                    }
                }
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

    pub(super) fn checked_tuple_element(&self, arity: usize) -> Option<usize> {
        let Some(PathSegment::TupleElement { index, .. }) = self.segments.first() else {
            return None;
        };
        Some(*index).filter(|index| *index < arity)
    }
}

/// A place that a pattern reads. An interface place cannot be read through
/// until a type assertion narrows it, and `concrete` is the only way to do that.
pub(super) struct Place {
    path: AccessPath,
    interface: bool,
}

impl Place {
    pub(super) fn new(facts: &EmitFacts<'_>, path: AccessPath, ty: &Type) -> Self {
        Self {
            path,
            interface: facts.is_interface(ty),
        }
    }

    pub(super) fn path(&self) -> &AccessPath {
        &self.path
    }

    pub(super) fn is_interface(&self) -> bool {
        self.interface
    }

    pub(super) fn literal_check(&self, go_literal: GoExpression, value: Option<Literal>) -> Check {
        Check::Literal {
            path: self.path.clone(),
            go_literal,
            value,
            on_interface: self.interface,
        }
    }

    pub(super) fn concrete(
        &self,
        narrowed_go_type: impl FnOnce() -> GoType,
        checks: &mut Vec<Check>,
    ) -> ConcretePath {
        if !self.interface {
            return ConcretePath {
                path: self.path.clone(),
            };
        }
        let go_type = narrowed_go_type();
        checks.push(Check::TypeAssert {
            path: self.path.clone(),
            go_type: go_type.clone(),
        });
        ConcretePath {
            path: self.path.push(PathSegment::AssertedAs(go_type)),
        }
    }
}

/// A path whose Go value has the static type that the pattern names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::lowering) struct ConcretePath {
    path: AccessPath,
}

impl ConcretePath {
    pub(in crate::lowering) fn path(&self) -> &AccessPath {
        &self.path
    }

    pub(super) fn field(
        &self,
        facts: &EmitFacts<'_>,
        receiver_ty: &Type,
        name: String,
    ) -> AccessPath {
        if facts.field_read_cannot_panic(receiver_ty) {
            self.path.push(PathSegment::ValueField(name))
        } else {
            self.path.push(PathSegment::Field(name))
        }
    }

    pub(super) fn tuple_element(
        &self,
        facts: &EmitFacts<'_>,
        tuple_ty: &Type,
        index: usize,
    ) -> AccessPath {
        self.path.push(PathSegment::TupleElement {
            index,
            value_field: facts.field_read_cannot_panic(tuple_ty),
        })
    }

    pub(super) fn boxed_field(
        &self,
        facts: &EmitFacts<'_>,
        receiver_ty: &Type,
        name: String,
    ) -> AccessPath {
        self.field(facts, receiver_ty, name)
            .push(PathSegment::Deref)
    }

    pub(super) fn raw_field(&self, name: String) -> AccessPath {
        self.path.push(PathSegment::Field(name))
    }

    pub(super) fn index(&self, index: usize) -> AccessPath {
        self.path.push(PathSegment::Index(index))
    }

    pub(super) fn slice_from(&self, offset: usize) -> AccessPath {
        self.path.push(PathSegment::SliceFrom(offset))
    }

    pub(super) fn array_slice_from(&self, offset: usize, go_type: GoType) -> AccessPath {
        self.path
            .push(PathSegment::ArraySliceFrom { offset, go_type })
    }

    pub(super) fn deref(&self) -> AccessPath {
        self.path.push(PathSegment::Deref)
    }

    pub(super) fn newtype_cast(&self, go_type: GoType) -> AccessPath {
        self.path.push(PathSegment::NewtypeCast(go_type))
    }
}
