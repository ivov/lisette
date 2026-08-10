use rustc_hash::FxHashSet as HashSet;

use crate::Planner;
use crate::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::File;

impl Planner<'_> {
    /// Record the emitted Go names of top-level private functions and
    /// constants that differ from their source spelling. Colliding private
    /// functions freshen to `name_2`, `name_3`, etc. Constants never
    /// freshen: cross-package references derive a constant's Go name from
    /// the source name alone, so converging constants surface as a Go name
    /// collision instead.
    pub(crate) fn collect_escape_remap(&mut self, files: &[&File]) {
        for item in files.iter().flat_map(|f| &f.items) {
            if let Expression::Const { identifier, .. } = item {
                let natural = go_name::screaming_snake_to_camel(identifier);
                if natural != identifier.as_str() {
                    self.package
                        .record_escape_remap(identifier.to_string(), natural);
                }
            }
        }

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

        let reserved = self.package_block_names(files);
        let mut colliding: Vec<&EcoString> = generic_names
            .iter()
            .filter(|name| reserved.contains(go_name::escape_type_name(name).as_ref()))
            .collect();
        if colliding.is_empty() {
            return;
        }
        colliding.sort();

        let mut taken = reserved;
        taken.extend(generic_names.iter().map(EcoString::to_string));

        for name in colliding {
            let fresh = (2..)
                .map(|n| format!("{}_{}", name, n))
                .find(|candidate| !taken.contains(candidate))
                .expect("freshening counter is unbounded");
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
