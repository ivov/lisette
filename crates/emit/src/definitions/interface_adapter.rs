use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::layout::{FunctionLayout, SlotOrigin, ValueLayout};
use crate::control_flow::propagation::plain_return;
use crate::names::go_name;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define_many, expression_statement,
};
use crate::plan::cleanup::clean_up;
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
#[cfg(debug_assertions)]
use crate::plan::verify::{verify_final_function_body, verify_local_scopes};
use crate::plan::visit::identify_body_locals;
use crate::utils::fresh_receiver_name;
use crate::write_line;
use ecow::EcoString;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::StructFields;
use syntax::ast::{Expression, FunctionDefinitionView};
use syntax::go_names;
use syntax::go_names::ConformanceCandidate;
use syntax::program::{
    Definition, DefinitionBody, DotAccessResolution, InterfaceRequirement, Method,
    interface_requirements, methods_for_type, resolved_definition,
};
use syntax::types::{
    SubstitutionMap, Symbol, Type, build_substitution_map, substitute, unqualified_name,
};
#[derive(Debug, Clone)]
pub(crate) struct AdapterPlan {
    concrete_id: EcoString,
    interface_id: EcoString,
    concrete_ty: Type,
    methods: Vec<AdapterMethod>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterMethod {
    /// The interface's Go method name.
    go_name: String,
    source_name: EcoString,
    /// The implementation's Go method name.
    source_go_name: String,
    /// An unconstrained generic receiver may expose either physical signature.
    alternate: Option<FunctionLayout>,
    /// The implementation's Go slots.
    source: FunctionLayout,
    /// The interface's Go slots.
    target: FunctionLayout,
    ret: AdapterReturn,
}

#[derive(Debug, Clone)]
enum AdapterReturn {
    Discard { user_returns_value: bool },
    ZeroFill,
    Forward,
    Bridge,
}

impl AdapterMethod {
    fn changes_signature(&self) -> bool {
        let return_changes = match self.ret {
            AdapterReturn::Discard { user_returns_value } => user_returns_value,
            AdapterReturn::ZeroFill | AdapterReturn::Bridge => true,
            AdapterReturn::Forward => false,
        };
        self.alternate.is_some()
            || return_changes
            || self.go_name != self.source_go_name
            || !self.source.same_parameter_representation(&self.target)
    }
}

impl Planner<'_> {
    /// Source checking proves the logical bound, but Go cannot express a union
    /// of methods returning tagged Partial and methods returning (value, error).
    pub(crate) fn bound_needs_dispatch(&self, bound: &Type) -> bool {
        interface_requirements(&self.facts.peel_alias(bound), |id| {
            self.facts.definition(id)
        })
        .iter()
        .any(|requirement| {
            !go_name::is_go_import(&requirement.declaring_interface)
                && requirement
                    .ty
                    .get_function_ret()
                    .is_some_and(|ty| self.facts.peel_alias(ty).is_partial())
        })
    }

    /// Generic Lisette interfaces call a method's tagged twin.
    pub(crate) fn requirement_uses_tagged_twin(&self, requirement: &InterfaceRequirement) -> bool {
        !go_name::is_go_import(&requirement.declaring_interface)
            && self.facts.method_uses_tagged_return(&requirement.name)
    }

    /// Lisette code calls the twin, so Go code alone sees the method's Go shape.
    pub(crate) fn selects_tagged_twin(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::DotAccess {
                expression: receiver,
                member,
                resolution:
                    DotAccessResolution::InstanceMethod { .. }
                    | DotAccessResolution::InstanceMethodValue { .. },
                ..
            } => self.receiver_selects_tagged_twin(&receiver.get_type(), member),
            Expression::Identifier { .. } => resolved_definition(expression)
                .is_some_and(|symbol| self.definition_selects_tagged_twin(symbol)),
            _ => false,
        }
    }

    pub(crate) fn definition_selects_tagged_twin(&self, symbol: &str) -> bool {
        symbol
            .rsplit_once('.')
            .is_some_and(|(owner, name)| self.has_tagged_twin(&self.peel_alias_id(owner), name))
    }

    fn receiver_selects_tagged_twin(&self, receiver_ty: &Type, member: &str) -> bool {
        if !self.facts.method_uses_tagged_return(member) {
            return false;
        }
        let receiver = self.facts.strip_and_peel(receiver_ty);
        let interfaces = match &receiver {
            Type::Parameter(name) => self
                .scope
                .type_params()
                .iter()
                .find(|parameter| &parameter.name == name)
                .map(|parameter| parameter.bounds.clone())
                .unwrap_or_default(),
            Type::Nominal { id, .. }
                if go_name::is_go_import(id) || id.starts_with(go_name::PRELUDE_PREFIX) =>
            {
                return false;
            }
            Type::Nominal { id, .. }
                if !self
                    .facts
                    .definition(id)
                    .is_some_and(Definition::is_interface) =>
            {
                return self.has_tagged_twin(id, member);
            }
            Type::Nominal { .. } => vec![receiver.clone()],
            _ => return false,
        };
        interfaces.iter().any(|interface| {
            interface_requirements(&self.facts.peel_alias(interface), |id| {
                self.facts.definition(id)
            })
            .iter()
            .any(|requirement| {
                requirement.name == member && self.requirement_uses_tagged_twin(requirement)
            })
        })
    }

    fn has_tagged_twin(&self, owner: &str, member: &str) -> bool {
        !go_name::is_go_import(owner)
            && !owner.starts_with(go_name::PRELUDE_PREFIX)
            && self.facts.method_uses_tagged_return(member)
            && self.facts.method(owner, member).is_some()
            && !self.facts.is_ufcs_method(owner, member)
    }

    /// A twin that returns the one tagged value a generic interface result needs.
    pub(crate) fn emit_tagged_twin(
        &mut self,
        method: FunctionDefinitionView<'_>,
        go_name: &str,
        generic_context: &[(EcoString, Vec<Type>)],
    ) -> String {
        let (receiver, params) = method
            .params
            .split_first()
            .expect("a method binds its receiver");
        let return_ty = method.return_type;
        let parameters: Vec<ValueLayout> = params
            .iter()
            .map(|param| self.value_layout(&param.ty, SlotOrigin::Lisette))
            .collect();
        let result = self.value_layout(return_ty, SlotOrigin::Lisette);
        let source = self.function_layout(
            parameters.clone(),
            &result,
            self.method_return_abi(return_ty),
            Some(return_ty),
        );
        let target = self.function_layout(
            parameters,
            &result,
            CallableReturnAbi::Value,
            Some(return_ty),
        );
        let ret = if source.return_abi == CallableReturnAbi::Void {
            AdapterReturn::ZeroFill
        } else if source.same_result_representation(&target) {
            AdapterReturn::Forward
        } else {
            AdapterReturn::Bridge
        };
        let twin = AdapterMethod {
            go_name: go_name::tagged_twin(go_name),
            source_name: method.name.clone(),
            source_go_name: go_name.to_string(),
            alternate: None,
            source,
            target,
            ret,
        };
        let mut declaration = String::new();
        self.emit_adapter_method(
            &mut declaration,
            MethodReceiver::Itself(&receiver.ty),
            generic_context,
            &twin,
        );
        declaration
    }

    fn requirement_go_name(&self, requirement: &InterfaceRequirement) -> String {
        let name = self.method_go_name(&requirement.name, false);
        if self.requirement_uses_tagged_twin(requirement) {
            go_name::tagged_twin(&name)
        } else {
            name
        }
    }

    pub(crate) fn bound_dispatch_requirement(
        &self,
        expression: &Expression,
    ) -> Option<(Type, InterfaceRequirement)> {
        self.bound_requirement(expression)
            .filter(|(bound, _)| self.bound_needs_dispatch(bound))
    }

    /// The requirement of a type parameter's bound that a method access selects.
    pub(crate) fn bound_requirement(
        &self,
        expression: &Expression,
    ) -> Option<(Type, InterfaceRequirement)> {
        let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = expression.unwrap_parens()
        else {
            return None;
        };
        let Type::Parameter(name) = self.facts.strip_and_peel(&receiver.get_type()) else {
            return None;
        };
        let parameter = self
            .scope
            .type_params()
            .iter()
            .find(|parameter| parameter.name == name)?;
        parameter.bounds.iter().find_map(|bound| {
            let bound = self.facts.peel_alias(bound);
            let requirement = interface_requirements(&bound, |id| self.facts.definition(id))
                .into_iter()
                .find(|requirement| requirement.name == *member)?;
            Some((bound, requirement))
        })
    }

    /// The Go interface method a bound method access selects.
    pub(crate) fn go_bound_requirement(
        &self,
        expression: &Expression,
    ) -> Option<InterfaceRequirement> {
        self.bound_requirement(expression)
            .map(|(_, requirement)| requirement)
            .filter(|requirement| go_name::is_go_import(&requirement.declaring_interface))
    }

    pub(crate) fn bound_dispatch_layout(&self, expression: &Expression) -> Option<FunctionLayout> {
        let (_, requirement) = self.bound_dispatch_requirement(expression)?;
        let mut layout = self.requirement_layout(&requirement, false)?;
        let result = self.value_layout(&layout.result_type, SlotOrigin::Lisette);
        layout = self.function_layout(layout.parameters, &result, CallableReturnAbi::Value, None);
        Some(layout)
    }

    fn requirement_layout(
        &self,
        requirement: &InterfaceRequirement,
        go: bool,
    ) -> Option<FunctionLayout> {
        let function = requirement.ty.as_function_type()?;
        let declared = if go && !go_name::is_go_import(&requirement.declaring_interface) {
            // A Go implementation satisfies the instantiated logical signature;
            // it does not inherit the Lisette interface's type parameters.
            function
        } else {
            requirement.method.ty.as_function_type()?
        };
        let result_type = self.facts.peel_alias(&function.return_type);
        let parameters = function
            .params
            .iter()
            .zip(&declared.params)
            .map(|(parameter, declared)| {
                let origin = if go {
                    SlotOrigin::go_parameter(self.facts.resolves_to_unknown(&declared.ty))
                } else {
                    SlotOrigin::Lisette
                };
                self.value_layout_with_declaration(&parameter.ty, origin, &declared.ty)
            })
            .collect();
        let origin = if go {
            SlotOrigin::go_return(self.facts.resolves_to_unknown(&declared.return_type))
        } else {
            SlotOrigin::Lisette
        };
        let result =
            self.value_layout_with_declaration(&result_type, origin, &declared.return_type);
        let abi = if go {
            self.facts
                .classify_go_return_type(&declared.return_type, &requirement.method.go_hints)
                .map_or_else(
                    || CallableReturnAbi::unlowered(&declared.return_type),
                    CallableReturnAbi::Lowered,
                )
        } else {
            self.interface_method_return_abi(&requirement.name, &requirement.method)
        };
        Some(self.function_layout(parameters, &result, abi, Some(&declared.return_type)))
    }

    pub(crate) fn dynamic_interface_adapter(
        &self,
        source: &Type,
        target: &Type,
        canonical: bool,
    ) -> Option<AdapterPlan> {
        let Type::Parameter(name) = source.strip_refs() else {
            return None;
        };
        let Type::Nominal { id, .. } = target else {
            return None;
        };
        let mut methods = Vec::new();
        let mut seen = HashSet::default();
        for requirement in interface_requirements(target, |id| self.facts.definition(id)) {
            if !seen.insert(self.method_go_name(&requirement.name, false)) {
                continue;
            }
            let local = self.requirement_layout(&requirement, false)?;
            let foreign = self.requirement_layout(&requirement, true)?;
            let target = if canonical {
                let result = self.value_layout(&local.result_type, SlotOrigin::Lisette);
                self.function_layout(
                    local.parameters.clone(),
                    &result,
                    CallableReturnAbi::Value,
                    None,
                )
            } else if go_name::is_go_import(&requirement.declaring_interface) {
                foreign.clone()
            } else {
                local.clone()
            };
            let name = self.requirement_go_name(&requirement);
            methods.push(AdapterMethod {
                source_go_name: name.clone(),
                go_name: name,
                source_name: requirement.name,
                source: local,
                alternate: Some(foreign),
                target,
                ret: AdapterReturn::Bridge,
            });
        }
        Some(AdapterPlan {
            concrete_id: name,
            interface_id: id.as_eco().clone(),
            concrete_ty: source.clone(),
            generic_context: self.adapter_generic_context(source, &methods),
            methods,
        })
    }

    fn dynamic_method_interface(&mut self, method_name: &str, layout: &FunctionLayout) -> String {
        let params = layout
            .parameters
            .iter()
            .map(|parameter| {
                let ty = parameter.go_type(self);
                self.use_rendered_go_type(ty)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let result = layout
            .result_go_type(self)
            .map(|ty| self.use_rendered_go_type(ty))
            .unwrap_or_default();
        format!("interface {{ {}({}) {} }}", method_name, params, result)
    }

    fn plan_dynamic_adapter_body(
        &mut self,
        method: &AdapterMethod,
        alternate: &FunctionLayout,
        receiver: &GoIdentifier,
        parameters: &[GoIdentifier],
    ) -> Vec<Statement> {
        let inner = GoExpression::conversion(
            "any".to_string(),
            GoExpression::selector(
                GoExpression::identifier(receiver.clone()),
                "inner".to_string(),
            ),
        );
        let mut statements = Vec::new();
        if !method.source.same_parameter_representation(alternate)
            || !method.source.same_result_representation(alternate)
        {
            let local_type = self.dynamic_method_interface(&method.source_go_name, &method.source);
            let local = self.fresh_local(Some("receiver"));
            let ok = self.fresh_local(Some("ok"));
            statements.push(define_many(
                vec![local.clone(), ok.clone()],
                GoExpression::type_assertion(inner.clone(), local_type),
            ));
            let mut branch = Vec::new();
            let arguments = self.plan_bridged_arguments(
                &mut branch,
                parameters
                    .iter()
                    .map(|parameter| GoExpression::identifier(parameter.clone()))
                    .collect(),
                &method.source,
                &method.target,
            );
            let call = GoExpression::call(
                GoExpression::selector(
                    GoExpression::identifier(local),
                    method.source_go_name.clone(),
                ),
                arguments,
            );
            self.plan_function_result_bridge(&mut branch, call, &method.source, &method.target);
            if method.target.return_abi == CallableReturnAbi::Void {
                branch.push(LoweredStatement::Return(Vec::new()).into());
            }
            statements.push(
                LoweredStatement::If(IfPlan::plain(
                    GoExpression::identifier(ok),
                    LoweredBlock { statements: branch },
                    ElseArm::None,
                ))
                .into(),
            );
        }
        let foreign_name = self.method_go_name(&method.source_name, false);
        let foreign_type = self.dynamic_method_interface(&foreign_name, alternate);
        let foreign = GoExpression::type_assertion(inner, foreign_type);
        let arguments = self.plan_bridged_arguments(
            &mut statements,
            parameters
                .iter()
                .map(|parameter| GoExpression::identifier(parameter.clone()))
                .collect(),
            alternate,
            &method.target,
        );
        let call = GoExpression::call(GoExpression::selector(foreign, foreign_name), arguments);
        self.plan_function_result_bridge(&mut statements, call, alternate, &method.target);
        statements
    }

    pub(crate) fn lookup_field_ty(
        &self,
        struct_ty: &Type,
        variant: Option<&str>,
        field_name: &str,
    ) -> Option<Type> {
        let (field_ty, subst_map) = self.declared_field_ty(struct_ty, variant, field_name)?;
        Some(match subst_map {
            Some(subst_map) => substitute(&field_ty, &subst_map),
            None => field_ty,
        })
    }

    pub(crate) fn declared_field_ty(
        &self,
        struct_ty: &Type,
        variant: Option<&str>,
        field_name: &str,
    ) -> Option<(Type, Option<SubstitutionMap>)> {
        let stripped = self.facts.strip_and_peel(struct_ty);
        let Type::Nominal { id, params, .. } = &stripped else {
            return None;
        };
        let (field_ty, generics) = match (&self.facts.definition(id.as_str())?.body, variant) {
            (
                DefinitionBody::Struct {
                    fields, generics, ..
                },
                None,
            ) => {
                let field = fields
                    .iter()
                    .find(|field| field.name == field_name)
                    .or_else(|| {
                        let StructFields::Tuple(fields) = fields else {
                            return None;
                        };
                        fields.get(field_name.parse::<usize>().ok()?)
                    })?;
                (field.ty.clone(), generics)
            }
            (
                DefinitionBody::Enum {
                    variants, generics, ..
                },
                Some(variant_name),
            ) => {
                let variant = variants
                    .iter()
                    .find(|variant| variant.name == unqualified_name(variant_name))?;
                let field = variant
                    .fields
                    .iter()
                    .find(|field| field.name == field_name)?;
                (field.ty.clone(), generics)
            }
            _ => return None,
        };
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
        if let Type::Parameter(name) = &source_stripped
            && self.scope.type_params().iter().any(|parameter| {
                &parameter.name == name
                    && parameter
                        .bounds
                        .iter()
                        .any(|bound| self.bound_needs_dispatch(bound))
            })
        {
            return self.dynamic_interface_adapter(source_ty, &target, false);
        }
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
        let impl_methods = methods_for_type(source_stripped, &HashMap::default(), |id| {
            self.facts.definition(id)
        });
        let source_requirements =
            interface_requirements(source_stripped, |id| self.facts.definition(id));

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
            let (impl_name, impl_ty) = go_names::conformance_method(
                &impl_methods,
                requirement.declaring_interface.as_str(),
                self.facts
                    .definition(requirement.declaring_interface.as_str())
                    .is_some_and(|definition| definition.visibility.is_public()),
                requirement.name.as_str(),
                &own_candidate,
            )?;
            let source_requirement = source_requirements
                .iter()
                .find(|source| source.name == *impl_name);
            let method = self.build_adapter_method(
                &requirement,
                impl_name,
                impl_ty,
                source_stripped,
                source_requirement,
            )?;
            any_adapted |= method.changes_signature();
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
        impl_name: &EcoString,
        impl_ty: &Type,
        concrete_ty: &Type,
        source_requirement: Option<&InterfaceRequirement>,
    ) -> Option<AdapterMethod> {
        let f = impl_ty.as_function_type()?;
        let (receiver_ty, params) = f.params.split_first()?;
        let substitution = if source_requirement.is_some() {
            SubstitutionMap::default()
        } else {
            method_receiver_substitution(&receiver_ty.ty, concrete_ty)?
        };
        let param_types: Vec<Type> = params
            .iter()
            .map(|param| substitute(&param.ty, &substitution))
            .collect();
        let return_type = substitute(&f.return_type, &substitution);

        let source_owner = source_requirement
            .map(|source| source.declaring_interface.as_str())
            .or_else(|| concrete_ty.get_qualified_id())?;
        let source_go_method =
            go_name::is_go_import(source_owner).then(|| format!("{source_owner}.{impl_name}"));
        let source_return = source_requirement
            .and_then(|source| source.method.ty.get_function_ret())
            .unwrap_or(&f.return_type);
        let user_abi = if let Some(id) = &source_go_method {
            self.facts.go_callable_return(id).cloned().map_or_else(
                || CallableReturnAbi::unlowered(source_return),
                CallableReturnAbi::Lowered,
            )
        } else if let Some(source) = source_requirement {
            self.interface_method_return_abi(&source.name, &source.method)
        } else if self.requirement_uses_tagged_twin(requirement) {
            CallableReturnAbi::Value
        } else {
            self.method_return_abi(&f.return_type)
        };
        let peeled_return = self.facts.peel_alias(&return_type);
        let user_result = self.interface_result_layout(source_go_method.as_deref(), &peeled_return);
        let source = self.function_layout(
            self.interface_parameter_layouts(
                source_go_method.as_deref(),
                &param_types,
                usize::from(source_requirement.is_none()),
            ),
            &user_result,
            user_abi,
            Some(source_return),
        );

        let interface_return = &requirement.method.ty.as_function_type()?.return_type;
        let go_method = go_name::is_go_import(&requirement.declaring_interface).then(|| {
            format!(
                "{}.{}",
                requirement.declaring_interface, requirement.method.source_name
            )
        });
        let interface_abi = match &go_method {
            Some(go_method) => self
                .facts
                .go_callable_return(go_method)
                .cloned()
                .map_or_else(
                    || CallableReturnAbi::unlowered(interface_return),
                    CallableReturnAbi::Lowered,
                ),
            None => self.interface_method_return_abi(&requirement.name, &requirement.method),
        };
        let interface_returns_void = interface_abi == CallableReturnAbi::Void;
        let parameters = self.interface_parameter_layouts(go_method.as_deref(), &param_types, 0);
        let target = if abi_matches_type(&interface_abi, &peeled_return) {
            let result = self.interface_result_layout(go_method.as_deref(), &peeled_return);
            self.function_layout(parameters, &result, interface_abi, Some(interface_return))
        } else {
            self.function_layout(
                parameters,
                &user_result,
                source.return_abi.clone(),
                Some(&f.return_type),
            )
        };

        let user_returns_value = source.return_abi != CallableReturnAbi::Void;
        let ret = if interface_returns_void {
            AdapterReturn::Discard { user_returns_value }
        } else if !user_returns_value {
            AdapterReturn::ZeroFill
        } else if source.same_result_representation(&target) {
            AdapterReturn::Forward
        } else {
            AdapterReturn::Bridge
        };

        let source_go_name = match source_requirement {
            Some(source) => self.requirement_go_name(source),
            None if source_go_method.is_none()
                && self.requirement_uses_tagged_twin(requirement) =>
            {
                go_name::tagged_twin(&self.method_go_name(impl_name, false))
            }
            None => self.method_go_name(impl_name, false),
        };
        Some(AdapterMethod {
            go_name: self.requirement_go_name(requirement),
            source_name: impl_name.clone(),
            source_go_name,
            alternate: None,
            source,
            target,
            ret,
        })
    }

    /// Go-imported interface slots come from the Go catalog.
    fn interface_parameter_layouts(
        &self,
        go_method: Option<&str>,
        param_types: &[Type],
        offset: usize,
    ) -> Vec<ValueLayout> {
        param_types
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                match go_method.and_then(|id| self.facts.go_callable_parameter(id, index + offset))
                {
                    Some(slot) => {
                        self.value_layout_with_declaration(ty, slot.origin, &slot.declared_type)
                    }
                    None => self.value_layout(ty, SlotOrigin::Lisette),
                }
            })
            .collect()
    }

    fn interface_result_layout(&self, go_method: Option<&str>, return_type: &Type) -> ValueLayout {
        match go_method.and_then(|id| self.facts.go_callable_return_slot(id)) {
            Some(slot) => {
                self.value_layout_with_declaration(return_type, slot.origin, &slot.declared_type)
            }
            None => self.value_layout(return_type, SlotOrigin::Lisette),
        }
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
        let return_ty = self.facts.peel_alias(return_ty);
        if self.facts.method_uses_tagged_return(method_name) {
            return CallableReturnAbi::Value;
        }
        with_comma_ok_hint(self.method_return_abi(&return_ty), method)
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
                MethodReceiver::Adapter(&adapter_type),
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
        receiver: MethodReceiver<'_>,
        generic_context: &[(EcoString, Vec<Type>)],
        method: &AdapterMethod,
    ) {
        self.with_declaration_scope(|this| {
            this.set_type_params(generic_context);
            let (receiver_type, receiver_name, wraps_inner) = match receiver {
                MethodReceiver::Adapter(name) => (name.to_string(), "a".to_string(), true),
                MethodReceiver::Itself(ty) => {
                    let receiver_type = this.use_go_type(ty);
                    let name =
                        fresh_receiver_name(&receiver_type, |name| this.shadows_declaration(name));
                    (receiver_type, name, false)
                }
            };
            let receiver_name = this.declare_adapter_method_binding(receiver_name);
            let param_names: Vec<GoIdentifier> = (0..method.target.parameters.len())
                .map(|i| this.declare_adapter_method_binding(format!("arg{}", i)))
                .collect();

            let params_str = param_names
                .iter()
                .zip(&method.target.parameters)
                .map(|(name, layout)| {
                    let go_type = layout.go_type(this);
                    format!("{} {}", name, this.use_rendered_go_type(go_type))
                })
                .collect::<Vec<_>>()
                .join(", ");

            let go_method_name = method.go_name.clone();
            let mut bindings = Vec::with_capacity(param_names.len() + 1);
            bindings.push(receiver_name.clone());
            bindings.extend(param_names);
            let (go_ret, body) = this.build_adapter_body(method, wraps_inner, &bindings);
            write_method_header(
                declaration,
                receiver_name.spelling(),
                &receiver_type,
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
        let name = self.fresh_local(Some(&preferred));
        self.declare(&name);
        name
    }

    fn build_adapter_body(
        &mut self,
        method: &AdapterMethod,
        wraps_inner: bool,
        parameters: &[GoIdentifier],
    ) -> (String, String) {
        let (go_ret, mut statements) = self.plan_adapter_body(method, wraps_inner, parameters);
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

    /// `bindings` holds the receiver, then one binding per parameter.
    fn plan_adapter_body(
        &mut self,
        method: &AdapterMethod,
        wraps_inner: bool,
        bindings: &[GoIdentifier],
    ) -> (String, Vec<Statement>) {
        let (receiver, parameters) = bindings
            .split_first()
            .expect("an adapter method binds its receiver");
        if let Some(alternate) = &method.alternate {
            let go_ret = method
                .target
                .result_go_type(self)
                .map(|ty| self.use_rendered_go_type(ty))
                .unwrap_or_default();
            return (
                go_ret,
                self.plan_dynamic_adapter_body(method, alternate, receiver, parameters),
            );
        }
        let mut statements = Vec::new();
        let arguments = self.plan_bridged_arguments(
            &mut statements,
            parameters
                .iter()
                .map(|name| GoExpression::identifier(name.clone()))
                .collect(),
            &method.source,
            &method.target,
        );
        let mut target = GoExpression::identifier(receiver.clone());
        if wraps_inner {
            target = GoExpression::selector(target, "inner".to_string());
        }
        let inner_call = GoExpression::call(
            GoExpression::selector(target, method.source_go_name.clone()),
            arguments,
        );

        let return_type = &method.source.result_type;
        // A unit result still fills the interface's generic result slot.
        let go_ret = match (&method.ret, method.target.result_go_type(self)) {
            (AdapterReturn::Discard { .. }, _) => String::new(),
            (_, Some(go_type)) => self.use_rendered_go_type(go_type),
            (_, None) => self.use_go_type(return_type),
        };
        match method.ret {
            AdapterReturn::Discard { .. } => statements.push(expression_statement(inner_call)),
            AdapterReturn::ZeroFill => {
                let zero = self.zero_value_expression(return_type);
                statements.push(expression_statement(inner_call));
                statements.push(plain_return(zero));
            }
            AdapterReturn::Forward => statements.push(plain_return(inner_call)),
            AdapterReturn::Bridge => self.plan_function_result_bridge(
                &mut statements,
                inner_call,
                &method.source,
                &method.target,
            ),
        }
        (go_ret, statements)
    }
}

enum MethodReceiver<'a> {
    Adapter(&'a str),
    Itself(&'a Type),
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
            method.source.result_type.contains_type(&parameter)
                || method
                    .source
                    .parameters
                    .iter()
                    .any(|param| param.logical_type().contains_type(&parameter))
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
