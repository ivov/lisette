use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::names::go_name;
use crate::plan::MakeFunctionPlan;
use crate::{Planner, PreludeType};
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::{DefinitionBody, File};

impl Planner<'_> {
    pub(crate) fn collect_user_to_string_facts(&mut self, files: &[&File]) {
        for (receiver_name, methods) in
            files
                .iter()
                .flat_map(|file| &file.items)
                .filter_map(|item| match item {
                    Expression::ImplBlock {
                        receiver_name,
                        methods,
                        ..
                    } => Some((receiver_name, methods)),
                    _ => None,
                })
        {
            if methods.iter().any(is_display_to_string)
                && !self
                    .facts
                    .is_ufcs_method(&self.facts.qualified_current(receiver_name), "to_string")
            {
                self.module
                    .record_user_to_string_type(receiver_name.to_string());
            }
        }
    }

    /// Record the emitted Go names of top-level private functions and
    /// constants that differ from their source spelling. Colliding private
    /// functions freshen to `name_2`, `name_3`, etc. Constants never
    /// freshen: cross-module references derive a constant's Go name from
    /// the source name alone, so converging constants surface as a Go name
    /// collision instead.
    pub(crate) fn collect_escape_remap(&mut self, files: &[&File]) {
        for item in files.iter().flat_map(|f| &f.items) {
            if let Expression::Const { identifier, .. } = item {
                let natural = go_name::screaming_snake_to_camel(identifier);
                if natural != identifier.as_str() {
                    self.module
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
                self.module
                    .record_escape_remap((*name).to_string(), natural.clone());
                continue;
            }
            let fresh = (2..)
                .map(|n| format!("{}_{}", base, n))
                .find(|c| !taken.contains(c))
                .expect("freshening counter is unbounded");
            taken.insert(fresh.clone());
            self.module.record_escape_remap((*name).to_string(), fresh);
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
            self.module.record_generic_rename(name.to_string(), fresh);
        }
    }

    pub(crate) fn collect_local_make_function_plans(&self) -> HashMap<u32, Vec<MakeFunctionPlan>> {
        let module_prefix = format!("{}.", self.facts.current_module());
        let mut code: HashMap<u32, Vec<MakeFunctionPlan>> = HashMap::default();

        let local_enums: Vec<_> = self
            .facts
            .iter_definitions()
            .filter_map(|(key, definition)| {
                let syntax::program::Definition {
                    name_span: Some(name_span),
                    body: DefinitionBody::Enum { variants, .. },
                    ..
                } = definition
                else {
                    return None;
                };
                let name = key.last_segment();
                if PreludeType::from_name(name).is_some() {
                    return None;
                }
                if !key.starts_with(&module_prefix) {
                    return None;
                }
                let rest = &key[module_prefix.len()..];
                if rest.contains('.') {
                    return None;
                }
                Some((key.to_string(), variants.clone(), name_span.file_id))
            })
            .collect();

        for (enum_id, variants, file_id) in local_enums {
            for variant in variants {
                code.entry(file_id).or_default().push(MakeFunctionPlan {
                    enum_id: enum_id.clone(),
                    variant_name: variant.name.to_string(),
                });
            }
        }

        code
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

fn is_display_to_string(method: &Expression) -> bool {
    if !matches!(method, Expression::Function { .. }) {
        return false;
    }
    let func = method.function_definition_view();
    func.name.as_str() == "to_string"
        && func.params.len() == 1
        && matches!(
            &func.params[0].pattern,
            syntax::ast::Pattern::Identifier { identifier, .. } if identifier == "self"
        )
        && matches!(
            func.return_type,
            syntax::types::Type::Simple(syntax::types::SimpleKind::String)
        )
}
