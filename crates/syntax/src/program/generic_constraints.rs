use rustc_hash::FxHashMap as HashMap;

use ecow::EcoString;

use crate::ast::Annotation;
use crate::types::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum GenericConstraint {
    Comparable,
    Ordered,
    Named(Annotation),
}

impl GenericConstraint {
    pub fn from_annotation(annotation: &Annotation) -> Self {
        if let Annotation::Constructor { name, .. } = annotation {
            match name.as_str() {
                "Comparable" | "prelude.Comparable" => return Self::Comparable,
                "Ordered" | "prelude.Ordered" | "go:cmp.Ordered" | "cmp.Ordered" => {
                    return Self::Ordered;
                }
                _ => {}
            }
        }
        Self::Named(annotation.clone())
    }
}

#[derive(Debug, Clone, Default)]
pub struct GenericConstraints {
    pub parameter: EcoString,
    pub explicit: Vec<GenericConstraint>,
    pub inferred_comparable: bool,
}

pub type GenericConstraintsByDefinition = HashMap<Symbol, Vec<GenericConstraints>>;
