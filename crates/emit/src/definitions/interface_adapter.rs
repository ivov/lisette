use crate::EmitEffects;
use crate::Planner;
use crate::ReturnContext;
use crate::abi::AbiShape;
use crate::abi::transition::emit_return_adapter;
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
}

pub(crate) struct AdapterMethod {
    pub(crate) name: EcoString,
    pub(crate) param_types: Vec<Type>,
    pub(crate) return_type: Type,
    pub(crate) user_shape: Option<AbiShape>,
    pub(crate) interface_shape: Option<AbiShape>,
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

    /// Adapter is needed when any method's natural emit shape differs
    /// from the interface's hint-shifted shape (e.g. `#[go(comma_ok)]`
    /// shifts `*T` to `(*T, bool)`).
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
        let Type::Nominal {
            id: source_id,
            params: source_params,
            ..
        } = &source_stripped
        else {
            return None;
        };
        if source_id.starts_with(GO_IMPORT_PREFIX) {
            return None;
        }
        let Some(Definition {
            body:
                DefinitionBody::Struct {
                    generics: struct_generics,
                    methods: struct_methods,
                    ..
                },
            ..
        }) = self.facts.definition(source_id.as_str())
        else {
            return None;
        };

        let subst_map = build_substitution_map(struct_generics, source_params);

        let all_interface_methods = self.collect_all_interface_methods(target_id, definition);
        let mut methods = Vec::with_capacity(all_interface_methods.len());
        let mut any_adapted = false;

        for (method_name, _interface_method_ty, declaring_id) in &all_interface_methods {
            let impl_ty = struct_methods.get(method_name)?;
            let (method, adapted) =
                self.build_adapter_method(method_name, declaring_id, impl_ty, &subst_map)?;
            if adapted {
                any_adapted = true;
            }
            methods.push(method);
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

    /// Returns `(method, meaningful)`; `meaningful` is set when the user
    /// shape differs from the hint-shifted interface shape.
    fn build_adapter_method(
        &self,
        method_name: &EcoString,
        declaring_id: &EcoString,
        impl_ty: &Type,
        subst_map: &SubstitutionMap,
    ) -> Option<(AdapterMethod, bool)> {
        let Type::Function(f) = impl_ty.unwrap_forall() else {
            return None;
        };
        let params = &f.params;
        let param_types: Vec<Type> = if params.is_empty() {
            Vec::new()
        } else {
            params[1..]
                .iter()
                .map(|p| substitute(p, subst_map))
                .collect()
        };
        let return_type = substitute(&f.return_type, subst_map);

        // Compute the natural shape once and shift it for the interface side
        // if a `#[go(...)]` hint applies, instead of re-walking `peel_alias`
        // twice via two `classify_direct_emission` calls.
        let user_shape = self.classify_direct_emission(&return_type);
        let interface_hints = self.go_interface_method_hints(declaring_id, method_name);
        let interface_shape = match user_shape.as_ref() {
            Some(AbiShape::NullableReturn) if interface_hints.iter().any(|h| h == "comma_ok") => {
                Some(AbiShape::CommaOk)
            }
            other => other.cloned(),
        };
        let adapted = user_shape != interface_shape;

        Some((
            AdapterMethod {
                name: method_name.clone(),
                param_types,
                return_type,
                user_shape,
                interface_shape,
            },
            adapted,
        ))
    }

    /// `NullableReturn` → `CommaOk` bridge for `#[go(comma_ok)]` methods.
    fn emit_hint_shift_bridge(
        &mut self,
        inner_call: &str,
        return_ty: &Type,
        user_shape: &AbiShape,
        interface_shape: &AbiShape,
        fx: &mut EmitEffects,
    ) -> Option<(String, String)> {
        let (AbiShape::NullableReturn, AbiShape::CommaOk) = (user_shape, interface_shape) else {
            return None;
        };
        let inner = self.facts.peel_alias(return_ty).ok_type();
        let is_interface = self.facts.is_interface(&inner);
        let val = self.fresh_var(Some("val"));
        self.declare(&val);
        let nil_check = if is_interface {
            fx.require_stdlib();
            format!("!lisette.IsNilInterface({})", val)
        } else {
            format!("{} != nil", val)
        };
        let go_ret = self.render_lowered_return_ty(&AbiShape::CommaOk, return_ty, fx);
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
        self.facts
            .definition(qualified.as_str())
            .map(|d| d.go_hints().to_vec())
            .unwrap_or_default()
    }

    /// Classify with `#[go(...)]` hints — `comma_ok` shifts the default
    /// `NullableReturn` to `CommaOk` for nilable `Option`s.
    pub(crate) fn classify_with_go_hints(
        &self,
        return_ty: &Type,
        hints: &[String],
    ) -> Option<AbiShape> {
        let base = self.classify_direct_emission(return_ty)?;
        if matches!(base, AbiShape::NullableReturn) && hints.iter().any(|h| h == "comma_ok") {
            return Some(AbiShape::CommaOk);
        }
        Some(base)
    }

    pub(crate) fn ensure_adapter_type(
        &mut self,
        plan: AdapterPlan,
        fx: &mut EmitEffects,
    ) -> String {
        let key = (
            concrete_dedup_key(&plan.concrete_ty, &plan.concrete_id),
            plan.interface_id.clone(),
        );
        if let Some(name) = self.adapter_registry.lookup(&key) {
            return name.clone();
        }

        let index = self.adapter_registry.next_index();
        let adapter_name = adapter_type_name(&plan, index);

        let concrete_go_ty = self.go_type_string(&plan.concrete_ty, fx);

        let mut declaration = String::new();
        write_line!(declaration, "type {} struct {{", adapter_name);
        write_line!(declaration, "inner {}", concrete_go_ty);
        write_line!(declaration, "}}");
        declaration.push('\n');

        for method in &plan.methods {
            self.emit_adapter_method(&mut declaration, &adapter_name, method, fx);
            declaration.push('\n');
        }

        self.adapter_registry
            .insert(key, adapter_name.clone(), declaration);
        adapter_name
    }

    fn emit_adapter_method(
        &mut self,
        declaration: &mut String,
        adapter_name: &str,
        method: &AdapterMethod,
        fx: &mut EmitEffects,
    ) {
        self.enter_scope();

        let param_names: Vec<String> = (0..method.param_types.len())
            .map(|i| format!("arg{}", i))
            .collect();
        for name in &param_names {
            self.declare(name);
        }

        let params_str = param_names
            .iter()
            .zip(method.param_types.iter())
            .map(|(n, t)| format!("{} {}", n, self.go_type_string(t, fx)))
            .collect::<Vec<_>>()
            .join(", ");

        let go_method_name = if self.method_needs_export(&method.name) {
            go_name::snake_to_camel(&method.name)
        } else {
            go_name::escape_keyword(&method.name).into_owned()
        };
        let inner_call = format!("a.inner.{}({})", go_method_name, param_names.join(", "));

        let (go_ret, body) = self.build_adapter_body(method, &inner_call, fx);
        self.finish_adapter_method(
            declaration,
            adapter_name,
            &go_method_name,
            &params_str,
            &go_ret,
            &body,
        );
    }

    fn build_adapter_body(
        &mut self,
        method: &AdapterMethod,
        inner_call: &str,
        fx: &mut EmitEffects,
    ) -> (String, String) {
        let user_shape = &method.user_shape;
        let interface_shape = &method.interface_shape;

        if user_shape == interface_shape
            && let Some(shape) = user_shape
        {
            let go_ret = self.render_lowered_return_ty(shape, &method.return_type, fx);
            return (go_ret, format!("return {}\n", inner_call));
        }

        if let (Some(user), Some(interface)) = (user_shape, interface_shape)
            && user != interface
            && let Some(bridge) =
                self.emit_hint_shift_bridge(inner_call, &method.return_type, user, interface, fx)
        {
            return bridge;
        }

        let adapter = emit_return_adapter(self, inner_call, &method.return_type, fx);
        if let Some(adapter) = adapter {
            return adapter;
        }

        if method.return_type.is_unit() {
            (String::new(), format!("{}\n", inner_call))
        } else {
            let ret = self.go_type_string(&method.return_type, fx);
            (ret, format!("return {}\n", inner_call))
        }
    }

    fn finish_adapter_method(
        &mut self,
        declaration: &mut String,
        adapter_name: &str,
        method_name: &str,
        params: &str,
        go_ret: &str,
        body: &str,
    ) {
        write_method_header(declaration, adapter_name, method_name, params, go_ret);
        declaration.push_str(body);
        write_line!(declaration, "}}");
        self.exit_scope();
    }

    pub(crate) fn resolve_tuple_slot_types(
        &mut self,
        inferred: Vec<Type>,
        in_tail: bool,
        ambient: Option<&ReturnContext>,
    ) -> Vec<Type> {
        let Some(resolved) = ambient.cloned() else {
            return inferred;
        };
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
        "func (a {}) {}({}){} {{",
        adapter_name,
        method_name,
        params,
        ret_suffix
    );
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
