use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::transition::reencode_return;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::plan::bodies::{LoweredBlock, Statement, expression_statement};
use crate::plan::cleanup::clean_up;
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
#[cfg(debug_assertions)]
use crate::plan::verify::{verify_final_function_body, verify_local_scopes};
use crate::plan::visit::identify_body_locals;
use crate::types::go_type::returns_go_void;
use crate::write_line;
use ecow::EcoString;
use rustc_hash::FxHashSet as HashSet;
use syntax::go_names;
use syntax::go_names::ConformanceCandidate;
use syntax::program::{
    Definition, DefinitionBody, InterfaceRequirement, Method, interface_requirements,
};
use syntax::types::{
    SubstitutionMap, Symbol, Type, build_substitution_map, substitute, unqualified_name,
};
pub(crate) struct AdapterPlan {
    concrete_id: EcoString,
    interface_id: EcoString,
    concrete_ty: Type,
    methods: Vec<AdapterMethod>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

pub(crate) struct AdapterMethod {
    name: EcoString,
    param_types: Vec<Type>,
    return_type: Type,
    ret: AdapterReturn,
}

enum AdapterReturn {
    Discard {
        user_returns_value: bool,
    },
    ZeroFill,
    Forward(CallableReturnAbi),
    Convert {
        user_abi: CallableReturnAbi,
        interface_abi: CallableReturnAbi,
    },
}

impl AdapterReturn {
    fn changes_signature(&self) -> bool {
        match self {
            Self::Discard { user_returns_value } => *user_returns_value,
            Self::ZeroFill | Self::Convert { .. } => true,
            Self::Forward(_) => false,
        }
    }
}

impl Planner<'_> {
    pub(crate) fn lookup_struct_field_ty(
        &self,
        struct_ty: &Type,
        field_name: &str,
    ) -> Option<Type> {
        let (field_ty, subst_map) = self.declared_struct_field_ty(struct_ty, field_name)?;
        Some(match subst_map {
            Some(subst_map) => substitute(&field_ty, &subst_map),
            None => field_ty,
        })
    }

    pub(crate) fn declared_struct_field_ty(
        &self,
        struct_ty: &Type,
        field_name: &str,
    ) -> Option<(Type, Option<SubstitutionMap>)> {
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
        let subst_map = (!generics.is_empty()).then(|| build_substitution_map(generics, params));
        Some((field_ty, subst_map))
    }

    pub(crate) fn is_function_alias(&self, ty: &Type) -> bool {
        let Type::Nominal { .. } = ty else {
            return false;
        };
        self.facts.resolve_to_function_type(ty).is_some()
    }

    /// Build an adapter when the implementation and interface expose different
    /// physical Go return signatures for the same logical methods.
    pub(crate) fn needs_adapter(&self, source_ty: &Type, target_ty: &Type) -> Option<AdapterPlan> {
        let target = self.facts.peel_alias(target_ty);
        let Type::Nominal { id: target_id, .. } = &target else {
            return None;
        };
        let Some(Definition {
            body: DefinitionBody::Interface { .. },
            ..
        }) = self.facts.definition(target_id.as_str())
        else {
            return None;
        };
        let source_stripped = self.facts.peel_alias(&source_ty.strip_refs());
        let Type::Nominal { id: source_id, .. } = &source_stripped else {
            return None;
        };
        let methods = self.adapted_methods(source_id, &source_stripped, &target)?;
        Some(AdapterPlan {
            concrete_id: source_id.as_eco().clone(),
            interface_id: target_id.as_eco().clone(),
            concrete_ty: source_ty.clone(),
            generic_context: self.adapter_generic_context(source_ty, &methods),
            methods,
        })
    }

    /// The adapter's method plans, or None when the source cannot implement
    /// the interface or no method needs adapting.
    fn adapted_methods(
        &self,
        source_id: &Symbol,
        source_stripped: &Type,
        target: &Type,
    ) -> Option<Vec<AdapterMethod>> {
        if source_id.starts_with(GO_IMPORT_PREFIX) {
            return None;
        }
        let impl_methods = match &self.facts.definition(source_id.as_str())?.body {
            DefinitionBody::Struct { methods, .. } | DefinitionBody::Enum { methods, .. } => {
                methods
            }
            _ => return None,
        };

        let own_candidate = |name: &str| ConformanceCandidate::Resolved {
            depth: 0,
            owner: source_id.as_eco().clone(),
            shadowed: self.facts.is_ufcs_method(source_id.as_str(), name),
        };

        let mut methods = Vec::new();
        let mut any_adapted = false;
        let mut seen = HashSet::default();
        for requirement in interface_requirements(target, |id| self.facts.definition(id)) {
            // Parents may spell one Go method with different source names.
            let selector = self.method_go_name(&requirement.name, false);
            if !seen.insert(selector) {
                continue;
            }
            let (_, impl_ty) = go_names::conformance_method(
                impl_methods,
                requirement.declaring_interface.as_str(),
                self.facts
                    .definition(requirement.declaring_interface.as_str())
                    .is_some_and(|definition| definition.visibility.is_public()),
                requirement.name.as_str(),
                &own_candidate,
            )?;
            let method = self.build_adapter_method(&requirement, impl_ty, source_stripped)?;
            any_adapted |= method.ret.changes_signature();
            methods.push(method);
        }
        any_adapted.then_some(methods)
    }

    fn adapter_generic_context(
        &self,
        source_ty: &Type,
        methods: &[AdapterMethod],
    ) -> Vec<(EcoString, Vec<Type>)> {
        let context = self.scope.type_params();
        if context
            .iter()
            .any(|param| adapter_uses_type_parameter(source_ty, methods, &param.name))
        {
            context
                .iter()
                .map(|param| (param.name.clone(), param.bounds.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn build_adapter_method(
        &self,
        requirement: &InterfaceRequirement,
        impl_ty: &Type,
        concrete_ty: &Type,
    ) -> Option<AdapterMethod> {
        let f = impl_ty.as_function_type()?;
        let (receiver_ty, params) = f.params.split_first()?;
        let substitution = method_receiver_substitution(&receiver_ty.ty, concrete_ty)?;
        let param_types = params
            .iter()
            .map(|param| substitute(&param.ty, &substitution))
            .collect();
        let return_type = substitute(&f.return_type, &substitution);

        let user_abi = if self.facts.method_uses_tagged_return(&requirement.name) {
            self.value_return_abi(&return_type)
        } else {
            self.callable_return_abi(&f.return_type)
        };
        let interface_return = &requirement.method.ty.as_function_type()?.return_type;
        let interface_abi = if go_name::is_go_import(&requirement.declaring_interface) {
            self.facts
                .go_callable_return(&format!(
                    "{}.{}",
                    requirement.declaring_interface, requirement.method.source_name
                ))
                .cloned()
                .map_or(CallableReturnAbi::Direct, CallableReturnAbi::Lowered)
        } else {
            self.interface_method_return_abi(&requirement.name, &requirement.method)
        };
        let user_returns_value = !returns_go_void(&f.return_type);
        let ret = if self.interface_method_returns_void(&interface_abi, interface_return) {
            AdapterReturn::Discard { user_returns_value }
        } else if !user_returns_value {
            AdapterReturn::ZeroFill
        } else if !abi_matches_type(&interface_abi, &self.facts.peel_alias(&return_type)) {
            AdapterReturn::Forward(user_abi)
        } else if self.callable_return_go_type(&user_abi, &return_type).code
            == self
                .callable_return_go_type(&interface_abi, &return_type)
                .code
        {
            AdapterReturn::Forward(interface_abi)
        } else {
            AdapterReturn::Convert {
                user_abi,
                interface_abi,
            }
        };

        Some(AdapterMethod {
            name: requirement.name.clone(),
            param_types,
            return_type,
            ret,
        })
    }

    pub(crate) fn interface_method_returns_void(
        &self,
        abi: &CallableReturnAbi,
        return_ty: &Type,
    ) -> bool {
        !abi.is_lowered() && returns_go_void(&self.facts.peel_alias(return_ty))
    }

    /// Go-imported interfaces take their ABI from the Go catalog instead.
    pub(crate) fn interface_method_return_abi(
        &self,
        method_name: &str,
        method: &Method,
    ) -> CallableReturnAbi {
        let return_ty = method
            .ty
            .get_function_ret()
            .expect("interface method must have function type");
        if self.facts.method_uses_tagged_return(method_name) {
            return self.value_return_abi(return_ty);
        }
        with_comma_ok_hint(self.callable_return_abi(return_ty), method)
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
            return name.to_string();
        }

        let index = self.adapter_registry.allocate_index();
        let mut base_name = adapter_type_name(&plan, index);
        while self.is_declared(&base_name) {
            base_name.push('_');
        }
        let (generics_decl, generics_use) = self.adapter_generics(&plan);
        let adapter_type = format!("{}{}", base_name, generics_use);

        let concrete_go_ty = self.use_go_type(&plan.concrete_ty);

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
        self.with_declaration_scope(|this| {
            this.set_type_params(generic_context);
            let receiver_name = this.declare_adapter_method_binding("a".to_string());
            let param_names: Vec<GoIdentifier> = (0..method.param_types.len())
                .map(|i| this.declare_adapter_method_binding(format!("arg{}", i)))
                .collect();

            let params_str = param_names
                .iter()
                .zip(method.param_types.iter())
                .map(|(n, t)| format!("{} {}", n, this.use_go_type(t)))
                .collect::<Vec<_>>()
                .join(", ");

            let go_method_name = this.method_go_name(&method.name, false);
            let inner_call = GoExpression::call(
                GoExpression::selector(
                    GoExpression::selector(
                        GoExpression::identifier(receiver_name.clone()),
                        "inner".to_string(),
                    ),
                    go_method_name.clone(),
                ),
                param_names
                    .iter()
                    .map(|name| GoExpression::identifier(name.clone()))
                    .collect(),
            );

            let mut bindings = Vec::with_capacity(param_names.len() + 1);
            bindings.push(receiver_name.clone());
            bindings.extend(param_names);
            let (go_ret, body) = this.build_adapter_body(method, inner_call, &bindings);
            write_method_header(
                declaration,
                receiver_name.spelling(),
                adapter_name,
                &go_method_name,
                &params_str,
                &go_ret,
            );
            declaration.push_str(&body);
            write_line!(declaration, "}}");
        });
    }

    fn declare_adapter_method_binding(&mut self, preferred: String) -> GoIdentifier {
        if self.try_declare(&preferred) {
            return GoIdentifier::local(preferred, self.scope.new_local_id());
        }
        let name = self.fresh_var(Some(&preferred));
        self.declare(&name);
        self.scope.generated_identifier(&name)
    }

    fn build_adapter_body(
        &mut self,
        method: &AdapterMethod,
        inner_call: GoExpression,
        parameters: &[GoIdentifier],
    ) -> (String, String) {
        let (go_ret, mut statements) = self.plan_adapter_body(method, inner_call);
        let shadowing = identify_body_locals(&mut statements, parameters, &mut self.scope);
        clean_up(&mut statements, &shadowing);
        let mut body = LoweredBlock { statements };
        if !go_ret.is_empty() {
            body.ensure_go_termination();
        }
        #[cfg(debug_assertions)]
        {
            verify_final_function_body(&mut body, !go_ret.is_empty())
                .unwrap_or_else(|error| panic!("{error}"));
            verify_local_scopes(&mut body.statements, &parameters.iter().collect::<Vec<_>>())
                .unwrap_or_else(|error| panic!("{error}"));
        }
        self.collect_imports(&body.statements);
        (go_ret, crate::Renderer.render_setup(&body.statements))
    }

    fn plan_adapter_body(
        &mut self,
        method: &AdapterMethod,
        inner_call: GoExpression,
    ) -> (String, Vec<Statement>) {
        let return_type = &method.return_type;

        let (user_abi, interface_abi) = match &method.ret {
            AdapterReturn::Discard { .. } => {
                return (String::new(), vec![expression_statement(inner_call)]);
            }
            AdapterReturn::ZeroFill => {
                let go_ret = self.use_go_type(return_type);
                let zero = self.zero_value_expression(return_type);
                return (
                    go_ret,
                    vec![expression_statement(inner_call), plain_return(zero)],
                );
            }
            AdapterReturn::Forward(abi) => {
                let go_ret = self.render_callable_return_ty(abi, return_type);
                return (go_ret, vec![plain_return(inner_call)]);
            }
            AdapterReturn::Convert {
                user_abi,
                interface_abi,
            } => (user_abi, interface_abi),
        };

        let logical_ty = self.facts.peel_alias(return_type);
        let go_ret = self.render_callable_return_ty(interface_abi, return_type);
        let statements = reencode_return(self, inner_call, user_abi, interface_abi, &logical_ty);
        (go_ret, statements)
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
    match abi.lowered() {
        None => true,
        Some(LoweredReturnAbi::BareError | LoweredReturnAbi::Result { .. }) => peeled.is_result(),
        Some(LoweredReturnAbi::Partial { .. }) => peeled.is_partial(),
        Some(LoweredReturnAbi::Option(_)) => peeled.is_option(),
        Some(LoweredReturnAbi::Tuple { .. }) => {
            peeled.tuple_arity().is_some_and(|arity| arity >= 2)
        }
    }
}

/// `#[go(comma_ok)]` shifts a nullable `Option` return to comma-ok form.
pub(crate) fn with_comma_ok_hint(base: CallableReturnAbi, method: &Method) -> CallableReturnAbi {
    if base == CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        && method.go_hints.iter().any(|hint| hint == "comma_ok")
    {
        return CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        }));
    }
    base
}
