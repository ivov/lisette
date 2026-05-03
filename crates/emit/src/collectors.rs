use rustc_hash::FxHashMap as HashMap;

use super::names::go_name;
use crate::Emitter;
use syntax::program::{DefinitionBody, File};
use syntax::types::Type;

impl Emitter<'_> {
    pub(crate) fn collect_exported_method_names(&mut self, files: &[&File]) {
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

        for (key, definition) in self.ctx.definitions.iter() {
            match &definition.body {
                DefinitionBody::Interface {
                    definition: iface, ..
                } if definition.visibility.is_public() => {
                    for method_name in iface.methods.keys() {
                        self.module
                            .exported_method_names
                            .insert(method_name.to_string());
                    }
                }
                DefinitionBody::Value { .. }
                    if definition.visibility.is_public()
                        && !go_name::is_go_import(key)
                        && !key.starts_with(go_name::PRELUDE_PREFIX)
                        && key.chars().filter(|c| *c == '.').count() >= 2 =>
                {
                    let method_name = go_name::unqualified_name(key);
                    self.module
                        .exported_method_names
                        .insert(method_name.to_string());
                }
                _ => {}
            }
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

    pub(crate) fn collect_go_call_strategies(&mut self) {
        let candidates: Vec<(String, Type, Vec<String>)> = self
            .ctx
            .definitions
            .iter()
            .filter(|(key, _)| go_name::is_go_import(key))
            .filter_map(|(key, definition)| {
                let ty = match definition.ty() {
                    Type::Forall { body, .. } => body.as_ref().clone(),
                    other => other.clone(),
                };
                let return_ty = match &ty {
                    Type::Function { return_type, .. } => (**return_type).clone(),
                    _ => return None,
                };
                let go_hints = definition.go_hints().to_vec();
                Some((key.to_string(), return_ty, go_hints))
            })
            .collect();

        for (key, return_ty, go_hints) in candidates {
            if let Some(strategy) = self.classify_go_return_type(&return_ty, &go_hints) {
                self.module.go_call_strategies.insert(key, strategy);
            }
        }
    }

    /// Register make function names for all enums; return bodies keyed by declaring file_id.
    pub(crate) fn collect_make_functions(&mut self) -> HashMap<u32, Vec<String>> {
        self.register_prelude_make_functions();

        let module_prefix = format!("{}.", self.current_module);
        let mut code: HashMap<u32, Vec<String>> = HashMap::default();

        // Collect enum info first to avoid borrow conflicts
        let enums: Vec<_> = self
            .ctx
            .definitions
            .iter()
            .filter_map(|(key, definition)| {
                if let syntax::program::Definition {
                    name: Some(name),
                    name_span: Some(name_span),
                    body: DefinitionBody::Enum { variants, .. },
                    ..
                } = definition
                {
                    if name == "Option" || name == "Result" || name == "Partial" {
                        return None;
                    }
                    Some((
                        key.to_string(),
                        name.clone(),
                        variants.clone(),
                        name_span.file_id,
                    ))
                } else {
                    None
                }
            })
            .collect();

        for (_, name, variants, _) in &enums {
            self.register_make_functions(name, variants);
        }

        for (key, _name, variants, file_id) in &enums {
            if !key.starts_with(&module_prefix) {
                continue;
            }
            let rest = &key[module_prefix.len()..];
            if rest.contains('.') {
                continue;
            }
            for variant in variants {
                let fn_code = self.create_make_function_code(key, &variant.name);
                code.entry(*file_id).or_default().push(fn_code);
            }
        }

        code
    }
}
