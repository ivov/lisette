use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi};
use crate::abi::transition::render_lowered_result_return;
use crate::names::go_name;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::write_line;
use ecow::EcoString;
use syntax::program::{Definition, DefinitionBody, Interface};
use syntax::types::{SubstitutionMap, Type, build_substitution_map, substitute, unqualified_name};
pub(crate) struct AdapterPlan {
    pub(crate) concrete_id: EcoString,
    pub(crate) interface_id: EcoString,
    pub(crate) concrete_ty: Type,
    pub(crate) methods: Vec<AdapterMethod>,
    pub(crate) generic_context: Vec<(EcoString, Vec<Type>)>,
}

pub(crate) struct AdapterMethod {
    pub(crate) name: EcoString,
    pub(crate) param_types: Vec<Type>,
    pub(crate) return_type: Type,
    pub(crate) user_abi: CallableReturnAbi,
    pub(crate) interface_abi: CallableReturnAbi,
    pub(crate) user_returns_void: bool,
    pub(crate) interface_returns_void: bool,
}

impl Planner<'_> {
    pub(crate) fn lookup_struct_field_ty(
        &self,
        struct_ty: &Type,
        field_name: &str,
    ) -> Option<Type> {
        let stripped = struct_ty.strip_refs();
        let Type::Nominal { id, params, .. } = &stripped else {
            return None;
        };
        let Some(Definition {
            body: DefinitionBody::Struct {
                fields, generics, ..
            },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };
        let field_ty = fields.iter().find(|f| f.name == field_name)?.ty.clone();
        if generics.is_empty() {
            return Some(field_ty);
        }
        let subst_map = build_substitution_map(generics, params);
        Some(substitute(&field_ty, &subst_map))
    }

    pub(crate) fn is_function_alias(&self, ty: &Type) -> bool {
        let Type::Nominal { .. } = ty else {
            return false;
        };
        self.facts.resolve_to_function_type(ty).is_some()
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
                let parent = self.facts.peel_alias(parent_ty);
                let Type::Nominal { id, .. } = &parent else {
                    continue;
                };
                if let Some(Definition {
                    body:
                        DefinitionBody::Interface {
                            definition: parent_definition,
                        },
                    ..
                }) = self.facts.definition(id.as_str())
                {
                    queue.push((parent_definition, id.as_eco().clone()));
                }
            }
        }
        result
    }

    /// Build an adapter when the implementation and interface expose different
    /// physical Go return signatures for the same logical methods.
    pub(crate) fn needs_adapter(&self, source_ty: &Type, target_ty: &Type) -> Option<AdapterPlan> {
        let target = self.facts.peel_alias(target_ty);
        let Type::Nominal { id: target_id, .. } = &target else {
            return None;
        };
        let Some(Definition {
            body: DefinitionBody::Interface { definition },
            ..
        }) = self.facts.definition(target_id.as_str())
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
        }) = self.facts.definition(source_id.as_str())
        else {
            return None;
        };

        let all_interface_methods = self.collect_all_interface_methods(target_id, definition);
        let mut methods = Vec::with_capacity(all_interface_methods.len());
        let mut any_adapted = false;

        for (method_name, interface_method_ty, declaring_id) in &all_interface_methods {
            let impl_ty = struct_methods.get(method_name)?;
            let (method, adapted) = self.build_adapter_method(
                method_name,
                declaring_id,
                interface_method_ty,
                impl_ty,
                &source_stripped,
            )?;
            if adapted {
                any_adapted = true;
            }
            methods.push(method);
        }

        if !any_adapted {
            return None;
        }

        let generic_context = self.function_state.generic_context();
        let generic_context = if generic_context
            .iter()
            .any(|(name, _)| adapter_uses_type_parameter(source_ty, &methods, name))
        {
            generic_context.to_vec()
        } else {
            Vec::new()
        };

        Some(AdapterPlan {
            concrete_id: source_id.as_eco().clone(),
            interface_id: target_id.as_eco().clone(),
            concrete_ty: source_ty.clone(),
            methods,
            generic_context,
        })
    }

    /// Returns the method plan and whether its physical Go signature differs.
    fn build_adapter_method(
        &self,
        method_name: &EcoString,
        declaring_id: &EcoString,
        interface_method_ty: &Type,
        impl_ty: &Type,
        concrete_ty: &Type,
    ) -> Option<(AdapterMethod, bool)> {
        let f = impl_ty.as_function_type()?;
        let (receiver_ty, params) = f.params.split_first()?;
        let substitution = method_receiver_substitution(receiver_ty, concrete_ty)?;
        let param_types = params
            .iter()
            .map(|param| substitute(param, &substitution))
            .collect();
        let return_type = substitute(&f.return_type, &substitution);

        let user_abi = self.callable_return_abi(&f.return_type);
        let interface_hints = self.go_interface_method_hints(declaring_id, method_name);
        let interface_return = &interface_method_ty.as_function_type()?.return_type;
        let interface_abi =
            self.callable_return_abi_with_go_hints(interface_return, &interface_hints);
        let interface_returns_void = self
            .lowered_return_go_type(&interface_abi, interface_return)
            .code
            == "struct{}";
        let method = AdapterMethod {
            name: method_name.clone(),
            param_types,
            return_type,
            user_abi,
            interface_abi,
            user_returns_void: f.return_type.is_unit(),
            interface_returns_void,
        };
        let adapted = self.adapter_needs_conversion(&method);

        Some((method, adapted))
    }

    /// `#[go(...)]` hints on an interface method (user-defined or
    /// Go-imported), looked up by `{interface_id}.{method_name}`.
    pub(crate) fn go_interface_method_hints(
        &self,
        interface_id: &str,
        method_name: &str,
    ) -> Vec<String> {
        let qualified = format!("{}.{}", interface_id, method_name);
        self.facts
            .definition(qualified.as_str())
            .map(|d| d.go_hints().to_vec())
            .unwrap_or_default()
    }

    /// Classify with `#[go(...)]` hints — `comma_ok` shifts a nullable
    /// `Option` return to comma-ok form.
    pub(crate) fn callable_return_abi_with_go_hints(
        &self,
        return_ty: &Type,
        hints: &[String],
    ) -> CallableReturnAbi {
        let base = self.callable_return_abi(return_ty);
        if let CallableReturnAbi::Option {
            encoding: OptionReturnAbi::Nullable,
            payload,
        } = base
            && hints.iter().any(|h| h == "comma_ok")
        {
            return CallableReturnAbi::Option {
                encoding: OptionReturnAbi::CommaOk,
                payload,
            };
        }
        base
    }

    pub(crate) fn ensure_adapter_type(&mut self, plan: AdapterPlan) -> String {
        let key = (
            concrete_dedup_key(&plan.concrete_ty, &plan.concrete_id),
            plan.interface_id.clone(),
        );
        let cacheable = plan.generic_context.is_empty();
        if cacheable
            && let Some(name) = self.adapter_registry.lookup(&key)
            && !self.is_declared(name)
        {
            return name.clone();
        }

        let index = self.adapter_registry.next_index();
        let mut base_name = adapter_type_name(&plan, index);
        while self.is_declared(&base_name) {
            base_name.push('_');
        }
        let (generics_decl, generics_use) = self.adapter_generics(&plan);
        let adapter_type = format!("{}{}", base_name, generics_use);

        let concrete_go_ty = self.go_type_string(&plan.concrete_ty);

        let mut declaration = String::new();
        write_line!(declaration, "type {}{} struct {{", base_name, generics_decl);
        write_line!(declaration, "inner {}", concrete_go_ty);
        write_line!(declaration, "}}");
        declaration.push('\n');

        for method in &plan.methods {
            self.emit_adapter_method(
                &mut declaration,
                &adapter_type,
                &plan.generic_context,
                method,
            );
            declaration.push('\n');
        }

        if cacheable {
            self.adapter_registry
                .insert(key, adapter_type.clone(), declaration);
        } else {
            self.adapter_registry.push_declaration(declaration);
        }
        adapter_type
    }

    fn adapter_generics(&mut self, plan: &AdapterPlan) -> (String, String) {
        if plan.generic_context.is_empty() {
            return (String::new(), String::new());
        }

        let names: Vec<String> = plan
            .generic_context
            .iter()
            .map(|(name, _)| self.generic_go_name(name).into_owned())
            .collect();
        let decl = self.resolved_generics_to_string(&plan.generic_context);
        let use_str = format!("[{}]", names.join(", "));
        (decl, use_str)
    }

    fn emit_adapter_method(
        &mut self,
        declaration: &mut String,
        adapter_name: &str,
        generic_context: &[(EcoString, Vec<Type>)],
        method: &AdapterMethod,
    ) {
        self.enter_scope();

        for (name, _) in generic_context {
            let go_name = self.generic_go_name(name).into_owned();
            self.declare(&go_name);
        }
        let receiver_name = self.declare_adapter_method_binding("a".to_string());
        let param_names: Vec<String> = (0..method.param_types.len())
            .map(|i| self.declare_adapter_method_binding(format!("arg{}", i)))
            .collect();

        let params_str = param_names
            .iter()
            .zip(method.param_types.iter())
            .map(|(n, t)| format!("{} {}", n, self.go_type_string(t)))
            .collect::<Vec<_>>()
            .join(", ");

        let go_method_name = if self.method_needs_export(&method.name) {
            go_name::snake_to_camel(&method.name)
        } else {
            go_name::escape_keyword(&method.name).into_owned()
        };
        let inner_call = format!(
            "{}.inner.{}({})",
            receiver_name,
            go_method_name,
            param_names.join(", ")
        );

        let (go_ret, body) = self.build_adapter_body(method, &inner_call);
        write_method_header(
            declaration,
            &receiver_name,
            adapter_name,
            &go_method_name,
            &params_str,
            &go_ret,
        );
        declaration.push_str(&body);
        write_line!(declaration, "}}");
        self.exit_scope();
    }

    fn declare_adapter_method_binding(&mut self, preferred: String) -> String {
        if self.try_declare(&preferred) {
            return preferred;
        }
        let name = self.fresh_var(Some(&preferred));
        self.declare(&name);
        name
    }

    fn adapter_needs_conversion(&self, method: &AdapterMethod) -> bool {
        if method.user_returns_void != method.interface_returns_void {
            return true;
        }
        if method.interface_returns_void {
            return false;
        }

        let peeled = self.facts.peel_alias(&method.return_type);
        if !abi_matches_type(&method.interface_abi, &peeled) {
            return false;
        }
        self.lowered_return_go_type(&method.user_abi, &method.return_type)
            .code
            != self
                .lowered_return_go_type(&method.interface_abi, &method.return_type)
                .code
    }

    fn build_adapter_body(&mut self, method: &AdapterMethod, inner_call: &str) -> (String, String) {
        let user_abi = &method.user_abi;
        let interface_abi = &method.interface_abi;
        let return_type = &method.return_type;

        if method.user_returns_void != method.interface_returns_void {
            if method.interface_returns_void {
                return (String::new(), format!("{}\n", inner_call));
            }
            let go_ret = self.go_type_string(return_type);
            let (zero, packages) = self.zero_value(return_type);
            self.require_packages(&packages);
            return (go_ret, format!("{}\nreturn {}\n", inner_call, zero));
        }

        if !self.adapter_needs_conversion(method) {
            if method.interface_returns_void {
                return (String::new(), format!("{}\n", inner_call));
            }
            let peeled = self.facts.peel_alias(return_type);
            let go_ret_abi = if abi_matches_type(interface_abi, &peeled) {
                interface_abi
            } else {
                user_abi
            };
            let go_ret = self.render_lowered_return_ty(go_ret_abi, return_type);
            return (go_ret, format!("return {}\n", inner_call));
        }

        let logical_ty = self.facts.peel_alias(return_type);
        let go_ret = self.render_lowered_return_ty(interface_abi, return_type);
        let (setup, tagged) = self.lower_abi_to_tagged(inner_call, user_abi, &logical_ty);
        let mut body = crate::Renderer.render_setup(&setup);
        if interface_abi.is_passthrough() {
            write_line!(body, "return {}", tagged);
        } else {
            let subject = if user_abi.is_passthrough() {
                let res = self.fresh_var(Some("res"));
                self.declare(&res);
                write_line!(body, "{} := {}", res, tagged);
                res
            } else {
                tagged
            };
            render_lowered_result_return(self, &mut body, &subject, &logical_ty, interface_abi);
        }
        (go_ret, body)
    }

    pub(crate) fn resolve_tuple_slot_types(
        &mut self,
        inferred: Vec<Type>,
        in_tail: bool,
    ) -> Vec<Type> {
        let resolved = self.return_ctx();
        let return_slots = resolved.ty().and_then(|ty| {
            let Type::Tuple(slots) = ty else {
                return None;
            };
            (slots.len() == inferred.len()).then(|| slots.clone())
        });

        let Some(return_slots) = return_slots else {
            return inferred;
        };

        if in_tail {
            return return_slots;
        }

        return_slots
            .iter()
            .zip(inferred.iter())
            .map(|(declared, inferred_slot)| {
                let needs_widening = self.needs_adapter(inferred_slot, declared).is_some()
                    || self.facts.is_interface(declared)
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
}

fn write_method_header(
    declaration: &mut String,
    receiver_name: &str,
    adapter_name: &str,
    method_name: &str,
    params: &str,
    go_ret: &str,
) {
    let ret_suffix = if go_ret.is_empty() {
        String::new()
    } else {
        format!(" {}", go_ret)
    };
    write_line!(
        declaration,
        "func ({} {}) {}({}){} {{",
        receiver_name,
        adapter_name,
        method_name,
        params,
        ret_suffix
    );
}

fn method_receiver_substitution(receiver_ty: &Type, concrete_ty: &Type) -> Option<SubstitutionMap> {
    let receiver = receiver_ty.strip_refs();
    let concrete = concrete_ty.strip_refs();
    let (
        Type::Nominal {
            id: receiver_id,
            params: receiver_params,
            ..
        },
        Type::Nominal {
            id: concrete_id,
            params: concrete_params,
            ..
        },
    ) = (&receiver, &concrete)
    else {
        return None;
    };
    if receiver_id != concrete_id || receiver_params.len() != concrete_params.len() {
        return None;
    }

    receiver_params
        .iter()
        .zip(concrete_params)
        .map(|(receiver_param, concrete_param)| {
            let Type::Parameter(name) = receiver_param else {
                return None;
            };
            Some((name.clone(), concrete_param.clone()))
        })
        .collect()
}

fn concrete_dedup_key(concrete_ty: &Type, concrete_id: &EcoString) -> EcoString {
    let mut depth = 0usize;
    let mut t = concrete_ty.clone();
    while t.is_ref() {
        depth += 1;
        t = t.inner().expect("Ref<T> must have inner").clone();
    }
    let params = match &t {
        Type::Nominal { params, .. } if !params.is_empty() => Some(params),
        _ => None,
    };
    let params_suffix = params
        .map(|ps| {
            let joined = ps
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("<{joined}>")
        })
        .unwrap_or_default();

    EcoString::from(format!(
        "{}{}{}",
        "*".repeat(depth),
        concrete_id.as_str(),
        params_suffix
    ))
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
    format!(
        "{}{}_{}_{}",
        go_name::ADAPTER_TYPE_PREFIX,
        concrete_name,
        iface_name,
        index
    )
}

fn adapter_uses_type_parameter(
    concrete_ty: &Type,
    methods: &[AdapterMethod],
    name: &EcoString,
) -> bool {
    let parameter = Type::Parameter(name.clone());
    concrete_ty.contains_type(&parameter)
        || methods.iter().any(|method| {
            method.return_type.contains_type(&parameter)
                || method
                    .param_types
                    .iter()
                    .any(|param| param.contains_type(&parameter))
        })
}

fn abi_matches_type(abi: &CallableReturnAbi, peeled: &Type) -> bool {
    match abi {
        CallableReturnAbi::Tagged | CallableReturnAbi::Direct => true,
        CallableReturnAbi::Result { .. } => peeled.is_result(),
        CallableReturnAbi::Partial { .. } => peeled.is_partial(),
        CallableReturnAbi::Option { .. } => peeled.is_option(),
        CallableReturnAbi::Tuple { .. } => peeled.tuple_arity().is_some_and(|arity| arity >= 2),
    }
}
