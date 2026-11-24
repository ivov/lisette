use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::lookup::facts::package_names::PackageNames;
use crate::target::names::go_name;
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::File;

pub(in crate::lowering) fn collect_escape_remap(
    names: &PackageNames,
    files: &[&File],
) -> HashMap<String, String> {
    let entries: Vec<(&str, String)> = files
        .iter()
        .flat_map(|f| &f.items)
        .filter_map(|item| match item {
            Expression::Function {
                name,
                visibility: Visibility::Private,
                ..
            } => {
                let natural =
                    go_name::escape_reserved(&go_name::snake_to_lower_camel(name)).into_owned();
                Some((name.as_str(), natural))
            }
            _ => None,
        })
        .collect();

    let mut remap = HashMap::default();
    let mut taken: HashSet<String> = entries
        .iter()
        .filter(|(name, natural)| *name == natural)
        .map(|(_, natural)| natural.clone())
        .collect();

    for (name, natural) in &entries {
        if *name == natural {
            continue;
        }
        let fresh = names.fresh_package_name(natural, |candidate| taken.contains(candidate));
        taken.insert(fresh.clone());
        remap.insert((*name).to_string(), fresh);
    }
    remap
}

pub(in crate::lowering) fn collect_generic_renames(
    names: &PackageNames,
    files: &[&File],
) -> HashMap<String, String> {
    let mut generic_names: HashSet<EcoString> = HashSet::default();
    for item in files.iter().flat_map(|file| &file.items) {
        collect_item_generic_names(item, &mut generic_names);
    }
    let mut renames = HashMap::default();
    if generic_names.is_empty() {
        return renames;
    }

    let mut colliding: Vec<&EcoString> = generic_names
        .iter()
        .filter(|name| names.is_package_block_name(go_name::escape_type_name(name).as_ref()))
        .collect();
    if colliding.is_empty() {
        return renames;
    }
    colliding.sort();

    let mut taken: HashSet<String> = generic_names.iter().map(EcoString::to_string).collect();
    for name in colliding {
        let fresh = names.fresh_package_name(name, |candidate| taken.contains(candidate));
        taken.insert(fresh.clone());
        renames.insert(name.to_string(), fresh);
    }
    renames
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
