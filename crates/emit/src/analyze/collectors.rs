use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::names::go_name;
use crate::{Planner, PreludeType};
use syntax::EcoString;
use syntax::ast::{Expression, Generic, Visibility};
use syntax::program::{DefinitionBody, File};

impl Planner<'_> {
    pub(crate) fn collect_local_method_facts(&mut self, files: &[&File]) {
        for item in files.iter().flat_map(|file| &file.items) {
            match item {
                Expression::Interface {
                    visibility: Visibility::Public,
                    method_signatures,
                    ..
                } => {
                    for method in method_signatures {
                        let func = method.function_definition_view();
                        self.module
                            .record_exported_method_name(func.name.to_string());
                    }
                }
                Expression::ImplBlock {
                    receiver_name,
                    methods,
                    ..
                } => self.record_impl_method_facts(receiver_name, methods),
                _ => {}
            }
        }
    }

    fn record_impl_method_facts(&mut self, receiver_name: &str, methods: &[Expression]) {
        for method in methods {
            if let Expression::Function {
                name,
                visibility: Visibility::Public,
                ..
            } = method
            {
                self.module.record_exported_method_name(name.to_string());
            }
            if is_display_to_string(method)
                && !self
                    .facts
                    .is_ufcs_method(&self.facts.qualified_current(receiver_name), "to_string")
            {
                self.module
                    .record_user_to_string_type(receiver_name.to_string());
            }
        }
    }

    /// Detect free top-level private Lisette names (free functions and
    /// constants) whose natural Go form would collide after `escape_reserved`
    /// — e.g. `len` escapes to `len_` and clashes with a sibling `len_`. The
    /// verbatim name claims its Go form; each escaped collider is remapped
    /// to `name_2`, `name_3`, etc. until unique. Public functions go through
    /// `snake_to_camel` and a separate identifier path, so they are not
    /// considered here.
    pub(crate) fn collect_escape_remap(&mut self, files: &[&File]) {
        let entries: Vec<(&str, String)> = files
            .iter()
            .flat_map(|f| &f.items)
            .filter_map(|item| match item {
                Expression::Function {
                    name,
                    visibility: Visibility::Private,
                    ..
                } => Some((name.as_str(), go_name::escape_reserved(name).into_owned())),
                Expression::Const { identifier, .. } => Some((
                    identifier.as_str(),
                    go_name::escape_reserved(identifier).into_owned(),
                )),
                _ => None,
            })
            .collect();

        let mut taken: HashSet<String> = entries
            .iter()
            .filter(|(name, natural)| *name == natural)
            .map(|(_, natural)| natural.clone())
            .collect();

        for (name, natural) in &entries {
            if *name == natural || taken.insert(natural.clone()) {
                continue;
            }
            let fresh = (2..)
                .map(|n| format!("{}_{}", name, n))
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
            .filter(|name| reserved.contains(name.as_str()))
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

    pub(crate) fn collect_module_aliases(&mut self, files: &[&File]) {
        for file in files {
            for import in file.imports() {
                let Some(alias) = import.effective_alias(self.facts.go_package_names()) else {
                    continue;
                };
                self.module
                    .record_module_alias(file.id, import.name.to_string(), alias);
            }
        }
    }

    pub(crate) fn collect_local_make_function_code(&mut self) -> HashMap<u32, Vec<String>> {
        let module_prefix = format!("{}.", self.facts.current_module());
        let mut code: HashMap<u32, Vec<String>> = HashMap::default();

        let local_enums: Vec<_> = self
            .facts
            .iter_definitions()
            .filter_map(|(key, definition)| {
                let syntax::program::Definition {
                    name: Some(name),
                    name_span: Some(name_span),
                    body: DefinitionBody::Enum { variants, .. },
                    ..
                } = definition
                else {
                    return None;
                };
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

        for (key, variants, file_id) in local_enums {
            self.module.set_active_file(file_id);
            for variant in &variants {
                let fn_code = self.create_make_function_code(&key, &variant.name);
                code.entry(file_id).or_default().push(fn_code);
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
