use crate::Emitter;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::write_line;
use ecow::EcoString;
use syntax::program::{Definition, Interface};
use syntax::types::Type;
pub(crate) struct AdapterPlan {
    pub(crate) concrete_id: EcoString,
    pub(crate) interface_id: EcoString,
    pub(crate) concrete_ty: Type,
    pub(crate) methods: Vec<AdapterMethod>,
}

pub(crate) struct AdapterMethod {
    pub(crate) name: EcoString,
    pub(crate) param_types: Vec<Type>,
    pub(crate) return_type: Type,
}

impl Emitter<'_> {
    pub(crate) fn lookup_struct_field_ty(
        &self,
        struct_ty: &Type,
        field_name: &str,
    ) -> Option<Type> {
        let Type::Nominal { id, .. } = struct_ty.strip_refs() else {
            return None;
        };
        let Some(Definition::Struct { fields, .. }) = self.ctx.definitions.get(id.as_str()) else {
            return None;
        };
        fields
            .iter()
            .find(|f| f.name == field_name)
            .map(|f| f.ty.clone())
    }

    pub(crate) fn is_go_function_alias(&self, ty: &Type) -> bool {
        let Type::Nominal { id, .. } = ty else {
            return false;
        };
        if !id.starts_with(GO_IMPORT_PREFIX) {
            return false;
        }
        self.resolve_to_function_type(ty).is_some()
    }

    pub(crate) fn resolve_to_function_type(&self, ty: &Type) -> Option<Type> {
        if matches!(ty, Type::Function { .. }) {
            return Some(ty.clone());
        }
        if let Some(underlying) = ty.get_underlying()
            && matches!(underlying, Type::Function { .. })
        {
            return Some(underlying.clone());
        }
        let peeled = self.peel_alias(ty);
        if matches!(peeled, Type::Function { .. }) {
            Some(peeled)
        } else {
            None
        }
    }

    fn collect_all_interface_methods(&self, iface: &Interface) -> Vec<(EcoString, Type)> {
        let mut result: Vec<(EcoString, Type)> = Vec::new();
        let mut seen: std::collections::HashSet<EcoString> = std::collections::HashSet::new();
        let mut queue: Vec<&Interface> = vec![iface];
        while let Some(current) = queue.pop() {
            for (name, ty) in &current.methods {
                if seen.insert(name.clone()) {
                    result.push((name.clone(), ty.clone()));
                }
            }
            for parent_ty in &current.parents {
                let parent = self.peel_alias(parent_ty);
                let Type::Nominal { id, .. } = &parent else {
                    continue;
                };
                if let Some(Definition::Interface {
                    definition: parent_def,
                    ..
                }) = self.ctx.definitions.get(id.as_str())
                {
                    queue.push(parent_def);
                }
            }
        }
        result
    }

    pub(crate) fn needs_adapter(&self, source_ty: &Type, target_ty: &Type) -> Option<AdapterPlan> {
        let target = self.peel_alias(target_ty);
        let Type::Nominal { id: target_id, .. } = &target else {
            return None;
        };
        if !target_id.starts_with(GO_IMPORT_PREFIX) {
            return None;
        }
        let Some(Definition::Interface { definition, .. }) =
            self.ctx.definitions.get(target_id.as_str())
        else {
            return None;
        };

        let source_stripped = source_ty.strip_refs();
        let Type::Nominal { id: source_id, .. } = &source_stripped else {
            return None;
        };
        if source_id.starts_with(GO_IMPORT_PREFIX) {
            return None;
        }
        let Some(Definition::Struct {
            methods: struct_methods,
            ..
        }) = self.ctx.definitions.get(source_id.as_str())
        else {
            return None;
        };

        let all_iface_methods = self.collect_all_interface_methods(definition);
        let mut methods = Vec::with_capacity(all_iface_methods.len());
        let mut any_adapted = false;

        for (method_name, _iface_method_ty) in &all_iface_methods {
            let impl_ty = struct_methods.get(method_name)?;
            let Type::Function {
                params,
                return_type,
                ..
            } = impl_ty.unwrap_forall()
            else {
                return None;
            };
            let method_params: Vec<Type> = if params.is_empty() {
                Vec::new()
            } else {
                params[1..].to_vec()
            };
            let return_ty = (**return_type).clone();
            if return_ty.is_result()
                || return_ty.is_partial()
                || return_ty.is_option()
                || return_ty.tuple_arity().is_some_and(|n| n >= 2)
            {
                any_adapted = true;
            }
            methods.push(AdapterMethod {
                name: method_name.clone(),
                param_types: method_params,
                return_type: return_ty,
            });
        }

        if !any_adapted {
            return None;
        }

        Some(AdapterPlan {
            concrete_id: source_id.as_eco().clone(),
            interface_id: target_id.as_eco().clone(),
            concrete_ty: source_ty.clone(),
            methods,
        })
    }

    fn concrete_dedup_key(concrete_ty: &Type, concrete_id: &EcoString) -> EcoString {
        let mut depth = 0usize;
        let mut t = concrete_ty.clone();
        while t.is_ref() {
            depth += 1;
            t = t.inner().expect("Ref<T> must have inner").clone();
        }
        if depth == 0 {
            concrete_id.clone()
        } else {
            EcoString::from("*".repeat(depth) + concrete_id.as_str())
        }
    }

    pub(crate) fn ensure_adapter_type(&mut self, plan: AdapterPlan) -> String {
        let key = (
            Self::concrete_dedup_key(&plan.concrete_ty, &plan.concrete_id),
            plan.interface_id.clone(),
        );
        if let Some(name) = self.synthesized_adapter_types.get(&key) {
            return name.clone();
        }

        let index = self.synthesized_adapter_types.len();
        let adapter_name = Self::adapter_type_name(&plan, index);
        self.synthesized_adapter_types
            .insert(key, adapter_name.clone());

        let concrete_go_ty = self.go_type_as_string(&plan.concrete_ty);

        let mut decl = String::new();
        write_line!(decl, "type {} struct {{", adapter_name);
        write_line!(decl, "inner {}", concrete_go_ty);
        write_line!(decl, "}}");
        decl.push('\n');

        for method in &plan.methods {
            self.emit_adapter_method(&mut decl, &adapter_name, method);
            decl.push('\n');
        }

        self.pending_adapter_types.push(decl);
        adapter_name
    }

    fn emit_adapter_method(
        &mut self,
        decl: &mut String,
        adapter_name: &str,
        method: &AdapterMethod,
    ) {
        self.enter_scope();

        let param_names: Vec<String> = (0..method.param_types.len())
            .map(|i| format!("arg{}", i))
            .collect();
        for name in &param_names {
            self.declare(name);
        }

        let param_type_strs: Vec<String> = method
            .param_types
            .iter()
            .map(|t| self.go_type_as_string(t))
            .collect();
        let params_decl: Vec<String> = param_names
            .iter()
            .zip(param_type_strs.iter())
            .map(|(n, t)| format!("{} {}", n, t))
            .collect();

        let inner_call = format!("a.inner.{}({})", method.name, param_names.join(", "));

        let (go_ret, body) = match self.emit_return_adapter(&inner_call, &method.return_type) {
            Some((ret, body)) => (ret, body),
            None => {
                if method.return_type.is_unit() {
                    (String::new(), format!("{}\n", inner_call))
                } else {
                    let ret = self.go_type_as_string(&method.return_type);
                    (ret, format!("return {}\n", inner_call))
                }
            }
        };

        let ret_suffix = if go_ret.is_empty() {
            String::new()
        } else {
            format!(" {}", go_ret)
        };
        write_line!(
            decl,
            "func (a {}) {}({}){} {{",
            adapter_name,
            method.name,
            params_decl.join(", "),
            ret_suffix
        );
        decl.push_str(&body);
        write_line!(decl, "}}");

        self.exit_scope();
    }

    pub(crate) fn resolve_tuple_slot_types(&mut self, inferred: Vec<Type>) -> Vec<Type> {
        let return_slots = self.current_return_context.as_ref().and_then(|ret| {
            let Type::Tuple(slots) = ret else {
                return None;
            };
            (slots.len() == inferred.len()).then(|| slots.clone())
        });

        let Some(return_slots) = return_slots else {
            return inferred;
        };

        if self.position.is_tail() {
            return return_slots;
        }

        return_slots
            .iter()
            .zip(inferred.iter())
            .map(|(declared, inferred_slot)| {
                if self.needs_adapter(inferred_slot, declared).is_some()
                    || (declared.get_qualified_id().is_some()
                        && declared.get_qualified_id() == inferred_slot.get_qualified_id())
                {
                    declared.clone()
                } else {
                    inferred_slot.clone()
                }
            })
            .collect()
    }

    fn adapter_type_name(plan: &AdapterPlan, index: usize) -> String {
        let concrete_name = plan
            .concrete_id
            .rsplit('.')
            .next()
            .unwrap_or(plan.concrete_id.as_str());
        let go_path = plan
            .interface_id
            .strip_prefix(GO_IMPORT_PREFIX)
            .unwrap_or(plan.interface_id.as_str());
        let iface_name = go_path.rsplit('.').next().unwrap_or(go_path);
        format!("_lisAdapter_{}_{}_{}", concrete_name, iface_name, index)
    }
}
