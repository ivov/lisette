use rustc_hash::FxHashSet as HashSet;

use crate::Planner;
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::File;

impl Planner<'_> {
    /// Derive package constants that Go can represent as `const` declarations.
    /// This is package-wide because Go package declarations are not ordered:
    /// a constant's eligibility must not depend on which file or item renders
    /// first.
    pub(crate) fn derive_package_go_consts(&mut self, files: &[&File]) {
        let candidates: Vec<(String, &Expression)> = files
            .iter()
            .flat_map(|file| &file.items)
            .filter_map(|item| {
                let Expression::Const {
                    identifier,
                    expression,
                    ..
                } = item
                else {
                    return None;
                };
                Some((
                    self.facts.qualified_current(identifier),
                    expression.value()?,
                ))
            })
            .collect();

        loop {
            let newly_eligible: Vec<String> = candidates
                .iter()
                .filter(|(name, expression)| {
                    !self.package.is_go_const_binding(name)
                        && self.is_go_constant_expression(expression)
                })
                .map(|(name, _)| name.clone())
                .collect();
            if newly_eligible.is_empty() {
                break;
            }
            self.package.extend_go_const_bindings(newly_eligible);
        }
    }

    /// Record emitted Go names of private functions that differ from their source spelling.
    pub(crate) fn collect_escape_remap(&mut self, files: &[&File]) {
        let entries: Vec<(&str, String, String)> = files
            .iter()
            .flat_map(|f| &f.items)
            .filter_map(|item| match item {
                Expression::Function {
                    name,
                    visibility: Visibility::Private,
                    ..
                } => {
                    let base = go_name::snake_to_lower_camel(name);
                    let natural = go_name::escape_reserved(&base).into_owned();
                    Some((name.as_str(), base, natural))
                }
                _ => None,
            })
            .collect();

        let mut taken: HashSet<String> = entries
            .iter()
            .filter(|(name, _, natural)| *name == natural)
            .map(|(_, _, natural)| natural.clone())
            .collect();

        for (name, base, natural) in &entries {
            if *name == natural {
                continue;
            }
            if taken.insert(natural.clone()) {
                self.package
                    .record_escape_remap((*name).to_string(), natural.clone());
                continue;
            }
            let fresh = (2..)
                .map(|n| format!("{}_{}", base, n))
                .find(|c| !taken.contains(c))
                .expect("freshening counter is unbounded");
            taken.insert(fresh.clone());
            self.package.record_escape_remap((*name).to_string(), fresh);
        }
    }

    pub(crate) fn collect_generic_renames(&mut self, files: &[&File]) {
        let mut generic_names: HashSet<EcoString> = HashSet::default();
        for item in files.iter().flat_map(|file| &file.items) {
            collect_item_generic_names(item, &mut generic_names);
        }
        if generic_names.is_empty() {
            return;
        }

        let mut taken: HashSet<String> = self.package.package_block_names().cloned().collect();
        let mut colliding: Vec<&EcoString> = generic_names
            .iter()
            .filter(|name| taken.contains(go_name::escape_type_name(name).as_ref()))
            .collect();
        if colliding.is_empty() {
            return;
        }
        colliding.sort();

        taken.extend(generic_names.iter().map(EcoString::to_string));

        for name in colliding {
            let fresh = go_name::fresh_suffixed(name, |candidate| taken.contains(candidate));
            taken.insert(fresh.clone());
            self.package.record_generic_rename(name.to_string(), fresh);
        }
    }
}

fn collect_item_generic_names(item: &Expression, out: &mut HashSet<EcoString>) {
    let extend = |out: &mut HashSet<EcoString>, generics: &[Generic]| {
        out.extend(generics.iter().map(|g| g.name.clone()));
    };
    match item {
        Expression::Function { generics, .. }
        | Expression::Struct { generics, .. }
        | Expression::Enum { generics, .. }
        | Expression::TypeAlias { generics, .. } => extend(out, generics),
        Expression::Interface {
            generics,
            method_signatures,
            ..
        } => {
            extend(out, generics);
            for method in method_signatures {
                if let Expression::Function { generics, .. } = method {
                    extend(out, generics);
                }
            }
        }
        Expression::ImplBlock {
            generics, methods, ..
        } => {
            extend(out, generics);
            for method in methods {
                if let Expression::Function { generics, .. } = method {
                    extend(out, generics);
                }
            }
        }
        _ => {}
    }
}
