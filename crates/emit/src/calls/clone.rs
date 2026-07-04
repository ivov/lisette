use syntax::parse::TUPLE_FIELDS;
use syntax::types::{CompoundKind, Type};

use crate::Planner;
use crate::names::go_name;

impl Planner<'_> {
    pub(crate) fn render_clone(&mut self, value: &str, ty: &Type) -> String {
        let peeled = self.facts.peel_alias(ty);
        match &peeled {
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
                args,
            } => match args.first().and_then(|elem| self.element_clone(elem)) {
                Some(clone) => {
                    self.require_stdlib();
                    format!(
                        "{}.SliceCloneFunc({value}, {clone})",
                        go_name::GO_STDLIB_PKG
                    )
                }
                None => {
                    self.require_slices();
                    format!("slices.Clone({value})")
                }
            },
            Type::Compound {
                kind: CompoundKind::Map,
                args,
            } => match args.get(1).and_then(|v| self.element_clone(v)) {
                Some(clone) => {
                    self.require_stdlib();
                    format!("{}.MapCloneFunc({value}, {clone})", go_name::GO_STDLIB_PKG)
                }
                None => {
                    self.require_maps();
                    format!("maps.Clone({value})")
                }
            },
            _ => value.to_string(),
        }
    }

    fn element_clone(&mut self, ty: &Type) -> Option<String> {
        if !self.needs_clone(ty) {
            return None;
        }
        let peeled = self.facts.peel_alias(ty);
        let go_ty = self.go_type_string(ty);
        match &peeled {
            Type::Tuple(elems) => {
                let var = self.fresh_var(Some("e"));
                let statements = self.tuple_clone_statements(&var, elems);
                Some(format!(
                    "func({var} {go_ty}) {go_ty} {{\n{}\nreturn {var}\n}}",
                    statements.join("\n")
                ))
            }
            _ => {
                let var = self.fresh_var(Some("e"));
                let body = self.render_clone(&var, ty);
                Some(format!("func({var} {go_ty}) {go_ty} {{ return {body} }}"))
            }
        }
    }

    fn tuple_clone_statements(&mut self, place: &str, elems: &[Type]) -> Vec<String> {
        let mut statements = Vec::new();
        for (index, elem) in elems.iter().enumerate() {
            let Some(field) = TUPLE_FIELDS.get(index) else {
                break;
            };
            let field_place = format!("{place}.{field}");
            let peeled = self.facts.peel_alias(elem);
            match &peeled {
                Type::Compound {
                    kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                    ..
                } => statements.push(format!(
                    "{field_place} = {}",
                    self.render_clone(&field_place, elem)
                )),
                Type::Tuple(inner) if self.needs_clone(elem) => {
                    statements.extend(self.tuple_clone_statements(&field_place, inner))
                }
                _ => {}
            }
        }
        statements
    }

    fn needs_clone(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        match &peeled {
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                ..
            } => true,
            Type::Tuple(elems) => elems.iter().any(|e| self.needs_clone(e)),
            _ => false,
        }
    }
}
