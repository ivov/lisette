use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use super::names::go_name;
use crate::{Emitter, PreludeType};
use syntax::ast::{Expression, Visibility};
use syntax::program::{DefinitionBody, File};

impl Emitter<'_> {
    pub(crate) fn collect_local_exported_method_names(&mut self, files: &[&File]) {
        for file in files {
            for item in &file.items {
                if let syntax::ast::Expression::Interface {
                    visibility: syntax::ast::Visibility::Public,
                    method_signatures,
                    ..
                } = item
                {
                    for method in method_signatures {
                        let func = method.to_function_definition();
                        self.module
                            .exported_method_names
                            .insert(func.name.to_string());
                    }
                }

                if let syntax::ast::Expression::ImplBlock { methods, .. } = item {
                    for method in methods {
                        if let syntax::ast::Expression::Function {
                            name,
                            visibility: syntax::ast::Visibility::Public,
                            ..
                        } = method
                        {
                            self.module.exported_method_names.insert(name.to_string());
                        }
                    }
                }
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
            self.module.escape_remap.insert((*name).to_string(), fresh);
        }
    }

    pub(crate) fn collect_module_aliases(&mut self, files: &[&File]) {
        for file in files {
            for import in file.imports() {
                let Some(alias) = import.effective_alias(self.ctx.go_package_names) else {
                    continue;
                };
                self.module
                    .reverse_module_aliases
                    .insert(alias.clone(), import.name.to_string());
                self.module
                    .module_aliases
                    .insert(import.name.to_string(), alias);
            }
        }
    }

    pub(crate) fn collect_impl_bounds(&mut self, files: &[&File]) {
        use syntax::ast::Expression;

        for file in files {
            for item in &file.items {
                let Expression::ImplBlock {
                    receiver_name,
                    generics,
                    ..
                } = item
                else {
                    continue;
                };
                if !generics.iter().any(|g| !g.bounds.is_empty()) {
                    self.module
                        .unconstrained_impl_receivers
                        .insert(receiver_name.to_string());
                    continue;
                }
                self.record_bound_imports(generics);
                self.extend_impl_bounds(receiver_name, generics);
            }
        }
    }

    /// Register cross-module imports for any bound types referenced in these generics.
    /// In-module, Go-imported, and prelude modules don't need explicit imports.
    fn record_bound_imports(&mut self, generics: &[syntax::ast::Generic]) {
        for generic in generics {
            for bound in &generic.bounds {
                let syntax::ast::Annotation::Constructor { name, .. } = bound else {
                    continue;
                };
                let Some((module, _)) = name.split_once('.') else {
                    continue;
                };
                if module == self.current_module
                    || go_name::is_go_import(module)
                    || module == go_name::PRELUDE_MODULE
                {
                    continue;
                }
                let canonical = self.resolve_alias_to_module(module).to_string();
                self.require_module_import(&canonical);
            }
        }
    }

    /// Merge new generic bounds into an existing impl_bounds entry, or insert fresh.
    /// Go requires type parameter constraints on the type definition itself, so
    /// multiple impl blocks with the same receiver must contribute their bounds.
    fn extend_impl_bounds(&mut self, receiver_name: &str, generics: &[syntax::ast::Generic]) {
        let Some(existing_generics) = self.module.impl_bounds.get_mut(receiver_name) else {
            self.module
                .impl_bounds
                .insert(receiver_name.to_string(), generics.to_vec());
            return;
        };
        for new_gen in generics {
            let Some(existing_gen) = existing_generics
                .iter_mut()
                .find(|g| g.name == new_gen.name)
            else {
                continue;
            };
            for bound in &new_gen.bounds {
                if !existing_gen.bounds.contains(bound) {
                    existing_gen.bounds.push(bound.clone());
                }
            }
        }
    }

    pub(crate) fn collect_local_make_function_code(&mut self) -> HashMap<u32, Vec<String>> {
        let module_prefix = format!("{}.", self.current_module);
        let mut code: HashMap<u32, Vec<String>> = HashMap::default();

        let local_enums: Vec<_> = self
            .ctx
            .definitions
            .iter()
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
            for variant in &variants {
                let fn_code = self.create_make_function_code(&key, &variant.name);
                code.entry(file_id).or_default().push(fn_code);
            }
        }

        code
    }
}
