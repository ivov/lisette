use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::ast::{Annotation, Generic};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ConstraintAtom {
    Comparable,
    /// Implies `Comparable`; renders as `cmp.Ordered`.
    Ordered,
    Named(Annotation),
}

impl ConstraintAtom {
    pub(crate) fn implies_comparable(&self) -> bool {
        matches!(self, ConstraintAtom::Comparable | ConstraintAtom::Ordered)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParamConstraintSet {
    pub(crate) name: EcoString,
    pub(crate) explicit: Vec<ConstraintAtom>,
    pub(crate) inferred_comparable: bool,
}

impl ParamConstraintSet {
    pub(crate) fn add_explicit(&mut self, atom: ConstraintAtom) -> bool {
        if self.explicit.contains(&atom) {
            return false;
        }
        self.explicit.push(atom);
        true
    }

    pub(crate) fn mark_inferred_comparable(&mut self) -> bool {
        if self.inferred_comparable {
            return false;
        }
        self.inferred_comparable = true;
        true
    }

    pub(crate) fn requires_comparable(&self) -> bool {
        self.inferred_comparable || self.explicit.iter().any(ConstraintAtom::implies_comparable)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GenericConstraintTable {
    by_symbol: HashMap<String, Vec<ParamConstraintSet>>,
}

impl GenericConstraintTable {
    /// Idempotent.
    pub(crate) fn ensure_seeded(&mut self, symbol: &str, generics: &[Generic]) {
        if self.by_symbol.contains_key(symbol) {
            return;
        }
        let mut sets = Vec::with_capacity(generics.len());
        for generic in generics {
            let mut set = ParamConstraintSet {
                name: generic.name.clone(),
                ..Default::default()
            };
            for bound in &generic.bounds {
                set.add_explicit(classify_bound_annotation(bound));
            }
            sets.push(set);
        }
        self.by_symbol.insert(symbol.to_string(), sets);
    }

    pub(crate) fn get(&self, symbol: &str) -> Option<&[ParamConstraintSet]> {
        self.by_symbol.get(symbol).map(Vec::as_slice)
    }

    pub(crate) fn add_explicit(&mut self, symbol: &str, param: &str, atom: ConstraintAtom) -> bool {
        let Some(sets) = self.by_symbol.get_mut(symbol) else {
            return false;
        };
        let Some(set) = sets.iter_mut().find(|s| s.name.as_str() == param) else {
            return false;
        };
        set.add_explicit(atom)
    }

    pub(crate) fn mark_inferred_comparable(&mut self, symbol: &str, param: &str) -> bool {
        let Some(sets) = self.by_symbol.get_mut(symbol) else {
            return false;
        };
        let Some(set) = sets.iter_mut().find(|s| s.name.as_str() == param) else {
            return false;
        };
        set.mark_inferred_comparable()
    }
}

pub(crate) fn classify_bound_annotation(annotation: &Annotation) -> ConstraintAtom {
    if let Annotation::Constructor { name, .. } = annotation
        && let Some(builtin) = classify_builtin_name(name)
    {
        return builtin;
    }
    ConstraintAtom::Named(annotation.clone())
}

pub(crate) fn classify_builtin_name(name: &str) -> Option<ConstraintAtom> {
    match name {
        "Comparable" | "prelude.Comparable" => Some(ConstraintAtom::Comparable),
        "Ordered" | "prelude.Ordered" | "go:cmp.Ordered" | "cmp.Ordered" => {
            Some(ConstraintAtom::Ordered)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Span;

    fn ctor(name: &str) -> Annotation {
        Annotation::Constructor {
            name: name.into(),
            params: vec![],
            span: Span::dummy(),
        }
    }

    fn set(name: &str) -> ParamConstraintSet {
        ParamConstraintSet {
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn classify_recognizes_prelude_comparable() {
        assert_eq!(
            classify_bound_annotation(&ctor("Comparable")),
            ConstraintAtom::Comparable
        );
        assert_eq!(
            classify_bound_annotation(&ctor("prelude.Comparable")),
            ConstraintAtom::Comparable
        );
    }

    #[test]
    fn classify_recognizes_prelude_ordered() {
        assert_eq!(
            classify_bound_annotation(&ctor("Ordered")),
            ConstraintAtom::Ordered
        );
        assert_eq!(
            classify_bound_annotation(&ctor("prelude.Ordered")),
            ConstraintAtom::Ordered
        );
        assert_eq!(
            classify_bound_annotation(&ctor("go:cmp.Ordered")),
            ConstraintAtom::Ordered
        );
    }

    #[test]
    fn classify_passes_named_bounds_through() {
        let ann = ctor("Named");
        assert_eq!(classify_bound_annotation(&ann), ConstraintAtom::Named(ann));
    }

    #[test]
    fn add_explicit_dedups() {
        let mut s = set("T");
        assert!(s.add_explicit(ConstraintAtom::Comparable));
        assert!(!s.add_explicit(ConstraintAtom::Comparable));
    }

    #[test]
    fn ordered_implies_comparable() {
        let mut s = set("T");
        s.add_explicit(ConstraintAtom::Ordered);
        assert!(s.requires_comparable());
    }
}
