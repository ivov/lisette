use rustc_hash::{FxHashMap, FxHashSet};

use crate::lookup::facts::package_names::PackageNames;
use crate::target::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::File;

pub(in crate::lowering) fn collect_escape_remap(
    names: &PackageNames,
    files: &[&File],
) -> FxHashMap<String, String> {
    let entries: Vec<(&str, String)> = files
        .iter()
        .flat_map(|f| &f.items)
        .filter_map(|item| {
            let Expression::Function {
                name,
                visibility: Visibility::Private,
                ..
            } = item
            else {
                return None;
            };
            let natural =
                go_name::escape_reserved(&go_name::snake_to_lower_camel(name)).into_owned();
            Some((name.as_str(), natural))
        })
        .collect();

    let taken: FxHashSet<String> = entries
        .iter()
        .filter(|(name, natural)| *name == natural)
        .map(|(_, natural)| natural.clone())
        .collect();
    let pending = entries
        .iter()
        .filter(|(name, natural)| *name != natural)
        .map(|(name, natural)| (*name, natural.as_str()));
    assign_fresh_names(names, taken, pending)
}

pub(in crate::lowering) fn collect_generic_renames(
    names: &PackageNames,
    files: &[&File],
) -> FxHashMap<String, String> {
    let mut generic_names: FxHashSet<EcoString> = FxHashSet::default();
    for item in files.iter().flat_map(|file| &file.items) {
        collect_item_generic_names(item, &mut generic_names);
    }
    if generic_names.is_empty() {
        return FxHashMap::default();
    }

    let mut colliding: Vec<&EcoString> = generic_names
        .iter()
        .filter(|name| names.is_package_block_name(go_name::escape_type_name(name).as_ref()))
        .collect();
    if colliding.is_empty() {
        return FxHashMap::default();
    }
    colliding.sort();

    let taken: FxHashSet<String> = generic_names.iter().map(EcoString::to_string).collect();
    let pending = colliding
        .into_iter()
        .map(|name| (name.as_str(), name.as_str()));
    assign_fresh_names(names, taken, pending)
}

fn assign_fresh_names<'n>(
    names: &PackageNames,
    mut taken: FxHashSet<String>,
    pending: impl Iterator<Item = (&'n str, &'n str)>,
) -> FxHashMap<String, String> {
    let mut renames = FxHashMap::default();
    for (name, base) in pending {
        let fresh = names.fresh_package_name(base, |candidate| taken.contains(candidate));
        taken.insert(fresh.clone());
        renames.insert(name.to_string(), fresh);
    }
    renames
}

fn collect_item_generic_names(item: &Expression, out: &mut FxHashSet<EcoString>) {
    let extend = |out: &mut FxHashSet<EcoString>, generics: &[Generic]| {
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
        Expression::Const { .. }
        | Expression::VariableDeclaration { .. }
        | Expression::PackageImport { .. }
        | body_expression!() => {}
    }
}
