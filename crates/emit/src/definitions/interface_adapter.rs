use crate::Emitter;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::write_line;
use ecow::EcoString;
use syntax::program::{Definition, DefinitionBody, Interface};
use syntax::types::{Type, unqualified_name};
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
    pub(crate) user_shape: Option<crate::types::abi::AbiShape>,
    pub(crate) interface_shape: Option<crate::types::abi::AbiShape>,
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
        let Some(Definition {
            body: DefinitionBody::Struct { fields, .. },
            ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
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
        fn as_function(ty: &Type) -> Option<Type> {
            if matches!(ty, Type::Function { .. }) {
                return Some(ty.clone());
            }
            ty.get_underlying()
                .filter(|u| matches!(u, Type::Function { .. }))
                .cloned()
        }
        as_function(ty).or_else(|| as_function(&self.peel_alias(ty)))
    }

    /// Collect own + transitively inherited methods, tagged with the id
    /// of the interface that *declared* each one. Methods are registered
    /// under the declaring interface, so hint lookup needs that id.
    fn collect_all_interface_methods(
        &self,
        root_id: &str,
        iface: &Interface,
    ) -> Vec<(EcoString, Type, EcoString)> {
        let mut result: Vec<(EcoString, Type, EcoString)> = Vec::new();
        let mut seen: std::collections::HashSet<EcoString> = std::collections::HashSet::new();
        let mut queue: Vec<(&Interface, EcoString)> = vec![(iface, EcoString::from(root_id))];
        while let Some((current, current_id)) = queue.pop() {
            for (name, ty) in &current.methods {
                if seen.insert(name.clone()) {
                    result.push((name.clone(), ty.clone(), current_id.clone()));
                }
            }
            for parent_ty in &current.parents {
                let parent = self.peel_alias(parent_ty);
                let Type::Nominal { id, .. } = &parent else {
                    continue;
                };
                if let Some(Definition {
                    body:
                        DefinitionBody::Interface {
                            definition: parent_def,
                        },
                    ..
                }) = self.ctx.definitions.get(id.as_str())
                {
                    queue.push((parent_def, id.as_eco().clone()));
                }
            }
        }
        result
    }

    /// Adapter is needed when any method's natural emit shape differs
    /// from the interface's hint-shifted shape (e.g. `#[go(comma_ok)]`
    /// shifts `*T` to `(*T, bool)`).
    pub(crate) fn needs_adapter(&self, source_ty: &Type, target_ty: &Type) -> Option<AdapterPlan> {
        let target = self.peel_alias(target_ty);
        let Type::Nominal { id: target_id, .. } = &target else {
            return None;
        };
        let Some(Definition {
            body: DefinitionBody::Interface { definition },
            ..
        }) = self.ctx.definitions.get(target_id.as_str())
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
        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    methods: struct_methods,
                    ..
                },
            ..
        }) = self.ctx.definitions.get(source_id.as_str())
        else {
            return None;
        };

        let all_interface_methods = self.collect_all_interface_methods(target_id, definition);
        let mut methods = Vec::with_capacity(all_interface_methods.len());
        let mut any_adapted = false;

        for (method_name, _interface_method_ty, declaring_id) in &all_interface_methods {
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

            // Compute the natural shape once and shift it for the interface
            // side if a `#[go(...)]` hint applies, instead of re-walking
            // `peel_alias` twice via two `classify_direct_emission` calls.
            let user_shape = self.classify_direct_emission(&return_ty);
            let interface_hints = self.go_interface_method_hints(declaring_id, method_name);
            let interface_shape = match user_shape.as_ref() {
                Some(crate::types::abi::AbiShape::NullableReturn)
                    if interface_hints.iter().any(|h| h == "comma_ok") =>
                {
                    Some(crate::types::abi::AbiShape::CommaOk)
                }
                other => other.cloned(),
            };
            if user_shape != interface_shape {
                any_adapted = true;
            }

            methods.push(AdapterMethod {
                name: method_name.clone(),
                param_types: method_params,
                return_type: return_ty,
                user_shape,
                interface_shape,
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

    /// `NullableReturn` → `CommaOk` bridge for `#[go(comma_ok)]` methods.
    fn emit_hint_shift_bridge(
        &mut self,
        inner_call: &str,
        return_ty: &Type,
        user_shape: &crate::types::abi::AbiShape,
        interface_shape: &crate::types::abi::AbiShape,
    ) -> Option<(String, String)> {
        use crate::types::abi::AbiShape as A;
        let (A::NullableReturn, A::CommaOk) = (user_shape, interface_shape) else {
            return None;
        };
        let inner = self.peel_alias(return_ty).ok_type();
        let is_interface = self.as_interface(&inner).is_some();
        let val = self.fresh_var(Some("val"));
        self.declare(&val);
        let nil_check = if is_interface {
            self.flags.needs_stdlib = true;
            format!("!lisette.IsNilInterface({})", val)
        } else {
            format!("{} != nil", val)
        };
        let go_ret = self.render_lowered_return_ty(&A::CommaOk, return_ty);
        let body = format!("{val} := {inner_call}\nreturn {val}, {nil_check}\n");
        Some((go_ret, body))
    }

    /// `#[go(...)]` hints on an interface method (user-defined or
    /// Go-imported), looked up by `{interface_id}.{method_name}`.
    pub(crate) fn go_interface_method_hints(
        &self,
        interface_id: &str,
        method_name: &str,
    ) -> Vec<String> {
        let qualified = format!("{}.{}", interface_id, method_name);
        self.ctx
            .definitions
            .get(qualified.as_str())
            .map(|d| d.go_hints().to_vec())
            .unwrap_or_default()
    }

    /// Classify with `#[go(...)]` hints — `comma_ok` shifts the default
    /// `NullableReturn` to `CommaOk` for nilable `Option`s.
    pub(crate) fn classify_with_go_hints(
        &self,
        return_ty: &Type,
        hints: &[String],
    ) -> Option<crate::types::abi::AbiShape> {
        let base = self.classify_direct_emission(return_ty)?;
        if matches!(base, crate::types::abi::AbiShape::NullableReturn)
            && hints.iter().any(|h| h == "comma_ok")
        {
            return Some(crate::types::abi::AbiShape::CommaOk);
        }
        Some(base)
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

        let user_shape = method.user_shape.clone();
        let interface_shape = method.interface_shape.clone();
        let params_str = params_decl.join(", ");

        if user_shape == interface_shape
            && let Some(shape) = user_shape
        {
            let go_ret = self.render_lowered_return_ty(&shape, &method.return_type);
            write_method_header(decl, adapter_name, &method.name, &params_str, &go_ret);
            decl.push_str(&format!("return {}\n", inner_call));
            write_line!(decl, "}}");
            self.exit_scope();
            return;
        }

        if let (Some(user), Some(interface)) = (user_shape, interface_shape)
            && user != interface
            && let Some((go_ret, body)) =
                self.emit_hint_shift_bridge(&inner_call, &method.return_type, &user, &interface)
        {
            write_method_header(decl, adapter_name, &method.name, &params_str, &go_ret);
            decl.push_str(&body);
            write_line!(decl, "}}");
            self.exit_scope();
            return;
        }

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
        let return_slots = self.current_return_context.as_ref().and_then(|ctx| {
            let Type::Tuple(slots) = &ctx.ty else {
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
                let needs_widening = self.needs_adapter(inferred_slot, declared).is_some()
                    || self.as_interface(declared).is_some()
                    || (declared.get_qualified_id().is_some()
                        && declared.get_qualified_id() == inferred_slot.get_qualified_id());
                if needs_widening {
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
        let iface_name = unqualified_name(go_path);
        format!("_lisAdapter_{}_{}_{}", concrete_name, iface_name, index)
    }
}

fn write_method_header(
    decl: &mut String,
    adapter_name: &str,
    method_name: &str,
    params: &str,
    go_ret: &str,
) {
    write_line!(
        decl,
        "func (a {}) {}({}) {} {{",
        adapter_name,
        method_name,
        params,
        go_ret
    );
}
