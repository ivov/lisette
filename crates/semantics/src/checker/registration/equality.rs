use rustc_hash::FxHashMap as HashMap;

use syntax::EcoString;
use syntax::program::DefinitionBody;
use syntax::types::{Symbol, Type, substitute};

use super::TaskState;
use crate::checker::infer::expressions::comparison::bound_implied;
use crate::store::Store;

/// A usable `equals`'s visibility (`None` public, `Some(module)` private), else `None`.
fn usable_equals_entry(store: &Store, id: &str) -> Option<Option<String>> {
    if store.equals_bound_mismatch.contains(id) {
        return None;
    }
    let definition = store.get_definition(id)?;
    let (methods, generics_len) = match &definition.body {
        DefinitionBody::Struct {
            methods, generics, ..
        }
        | DefinitionBody::Enum {
            methods, generics, ..
        } => (methods, generics.len()),
        _ => return None,
    };
    methods
        .get("equals")?
        .equals_receiver_vars(id, generics_len)?;
    let method_key = format!("{id}.equals");
    let method = store.get_definition(&method_key)?;
    if method.visibility.is_public() {
        Some(None)
    } else {
        Some(store.module_for_qualified_name(id).map(str::to_string))
    }
}

impl TaskState<'_> {
    /// Build the usable-`equals` verdict Go emission consumes, on the merged store.
    pub fn record_usable_equals(&mut self, store: &mut Store) {
        let module_ids: Vec<String> = store.modules.keys().cloned().collect();
        for module_id in &module_ids {
            self.record_bound_mismatched_equals(store, module_id);
        }

        let ids: Vec<Symbol> = store
            .modules
            .values()
            .flat_map(|module| module.definitions.iter())
            .filter_map(|(qualified, definition)| match &definition.body {
                DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. }
                    if methods.contains_key("equals") =>
                {
                    Some(qualified.clone())
                }
                _ => None,
            })
            .collect();

        for id in ids {
            if let Some(visibility) = usable_equals_entry(store, id.as_str()) {
                store.usable_equals.insert(id.to_string(), visibility);
            }
        }
    }

    /// Record one module's bound-mismatched `equals` types into the verdict's negative set.
    fn record_bound_mismatched_equals(&mut self, store: &mut Store, module_id: &str) {
        let candidates: Vec<(Symbol, Vec<EcoString>)> = {
            let module = store.get_module(module_id).expect("module must exist");
            module
                .definitions
                .iter()
                .filter_map(|(qualified, definition)| {
                    let (methods, arity) = match &definition.body {
                        DefinitionBody::Struct {
                            methods, generics, ..
                        }
                        | DefinitionBody::Enum {
                            methods, generics, ..
                        } => (methods, generics.len()),
                        _ => return None,
                    };
                    let method_ty = methods.get("equals")?;
                    let vars = method_ty.equals_receiver_vars(qualified.as_str(), arity)?;
                    Some((qualified.clone(), vars))
                })
                .collect()
        };
        let mut mismatched = Vec::new();
        for (qualified, vars) in &candidates {
            if self.equals_bounds_mismatch(store, qualified, vars) {
                mismatched.push(qualified.to_string());
            }
        }
        store.equals_bound_mismatch.extend(mismatched);
    }

    /// Whether the `equals` carries generic bounds the type does not imply, comparing by
    /// position (`vars[i]` is the method's variable in slot `i`).
    fn equals_bounds_mismatch(
        &mut self,
        store: &Store,
        qualified: &Symbol,
        vars: &[EcoString],
    ) -> bool {
        let Some(definition) = store.get_definition(qualified.as_str()) else {
            return false;
        };
        let (generics, method_ty) = match &definition.body {
            DefinitionBody::Struct {
                generics, methods, ..
            }
            | DefinitionBody::Enum {
                generics, methods, ..
            } => {
                let Some(method) = methods.get("equals") else {
                    return false;
                };
                (generics.clone(), method.clone())
            }
            _ => return false,
        };
        if generics.is_empty() {
            return false;
        }
        let method_bounds = method_bounds_by_var(store, &method_ty);
        let empty: Vec<Type> = Vec::new();
        // Rename the method's variables to the type's, so an alpha-equivalent bound matches.
        let alpha: HashMap<EcoString, Type> = vars
            .iter()
            .zip(&generics)
            .map(|(var, generic)| (var.clone(), Type::Parameter(generic.name.clone())))
            .collect();

        self.scopes.push();
        self.put_in_scope(&generics);
        let before = self.sink.len();
        let mut mismatch = false;
        for (position, generic) in generics.iter().enumerate() {
            let mut type_bounds: Vec<Type> = Vec::new();
            for bound in &generic.bounds {
                if let Some(ty) = self.resolve_type_bound(store, bound, &generic.span, qualified) {
                    type_bounds.push(ty);
                }
            }
            let method_set = method_bounds.get(&vars[position]).unwrap_or(&empty);
            if !method_set
                .iter()
                .all(|mb| bound_implied(store, &type_bounds, &substitute(mb, &alpha)))
            {
                mismatch = true;
                break;
            }
        }
        self.sink.truncate(before);
        self.scopes.pop();
        mismatch
    }
}

/// The bounds a method type carries, keyed by its type-variable name.
fn method_bounds_by_var(store: &Store, method_ty: &Type) -> HashMap<EcoString, Vec<Type>> {
    let func = match method_ty {
        Type::Forall { body, .. } => body.as_ref(),
        other => other,
    };
    let mut map: HashMap<EcoString, Vec<Type>> = HashMap::default();
    if let Type::Function(f) = func {
        for bound in &f.bounds {
            let resolved = store.deep_resolve_alias(&bound.ty);
            if resolved.get_qualified_id().is_some() {
                map.entry(bound.param_name.clone())
                    .or_default()
                    .push(resolved);
            }
        }
    }
    map
}
