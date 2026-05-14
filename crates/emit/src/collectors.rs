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
                            .record_exported_method_name(func.name.to_string());
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
                            self.module.record_exported_method_name(name.to_string());
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
            self.module.record_escape_remap((*name).to_string(), fresh);
        }
    }

    pub(crate) fn collect_module_aliases(&mut self, files: &[&File]) {
        for file in files {
            for import in file.imports() {
                let Some(alias) = import.effective_alias(self.facts.go_package_names()) else {
                    continue;
                };
                self.module
                    .record_module_alias(import.name.to_string(), alias);
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
                        .record_unconstrained_impl_receiver(receiver_name.to_string());
                    continue;
                }
                self.record_bound_imports(generics);
                self.module.record_impl_bounds(receiver_name, generics);
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
                if self.facts.is_current_module(module)
                    || go_name::is_go_import(module)
                    || module == go_name::PRELUDE_MODULE
                {
                    continue;
                }
                let canonical = self
                    .module
                    .module_for_alias(module)
                    .unwrap_or(module)
                    .to_string();
                self.require_module_import(&canonical);
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
            for variant in &variants {
                let fn_code = self.create_make_function_code(&key, &variant.name);
                code.entry(file_id).or_default().push(fn_code);
            }
        }

        code
    }
}
