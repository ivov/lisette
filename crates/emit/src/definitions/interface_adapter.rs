use crate::EmitFacts;
use crate::Planner;
use crate::abi::ReturnSlot;
use crate::abi::callable::{CallableReturnAbi, LoweredReturnAbi};
use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::control_flow::propagation::plain_return;
use crate::definitions::functions::BodyOrigin;
use crate::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define_many, directed_first,
    expression_statement,
};
use crate::ir::declarations::{
    DeclarationKind, FieldDeclaration, FunctionSignature, GoDeclaration,
};
use crate::ir::go_expression::GoParameter;
use crate::ir::go_expression::Position;
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::go_name;
use crate::names::go_name::GO_IMPORT_PREFIX;
use crate::names::receiver::fresh_receiver_name;
use crate::types::go_type::GoType;
use ecow::EcoString;
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::StructFields;
use syntax::ast::{Expression, FunctionDefinitionView};
use syntax::program::SlotOrigin;
use syntax::program::{CallOperation, CheckedMethod, ConformanceWitness, InterfaceConversion};
use syntax::program::{
    Definition, DefinitionBody, DotAccessResolution, InterfaceRequirement, Method,
    interface_requirements,
};
use syntax::types::{SubstitutionMap, Type, build_substitution_map, substitute, unqualified_name};
#[derive(Debug, Clone)]
pub(crate) struct AdapterPlan {
    concrete_id: EcoString,
    interface_id: EcoString,
    interface_ty: Type,
    concrete_ty: Type,
    methods: Vec<AdapterMethod>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct AdapterMethod {
    go_name: String,
    source_name: EcoString,
    source_go_name: String,
    alternate: Option<FunctionLayout>,
    source: FunctionLayout,
    target: FunctionLayout,
}

impl AdapterMethod {
    fn changes_signature(&self) -> bool {
        self.alternate.is_some()
            || self.go_name != self.source_go_name
            || !self.source.same_representation(&self.target)
    }
}

impl Planner<'_> {
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
            Expression::Identifier { .. } => self
                .facts
                .checked(expression)
                .function_value()
                .and_then(|recorded| recorded.definition.as_deref())
                .is_some_and(|symbol| self.facts.definition_selects_tagged_twin(symbol)),
            _ => false,
        }
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
            Type::Nominal { id, .. } if go_name::is_go_import(id) || go_name::is_prelude_id(id) => {
                return false;
            }
            Type::Nominal { id, .. }
                if !self
                    .facts
                    .definition(id)
                    .is_some_and(Definition::is_interface) =>
            {
                return self.facts.has_tagged_twin(id, member);
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
                requirement.name == member && self.facts.requirement_uses_tagged_twin(requirement)
            })
        })
    }

    pub(crate) fn emit_tagged_twin(
        &mut self,
        method: FunctionDefinitionView<'_>,
        go_name: &str,
        generic_context: &[(EcoString, Vec<Type>)],
    ) -> GoDeclaration {
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
            self.facts.return_abi(return_ty, ReturnSlot::Method),
            Some(return_ty),
        );
        let target = self.function_layout(
            parameters,
            &result,
            CallableReturnAbi::Value,
            Some(return_ty),
        );
        let twin = AdapterMethod {
            go_name: go_name::tagged_twin(go_name),
            source_name: method.name.clone(),
            source_go_name: go_name.to_string(),
            alternate: None,
            source,
            target,
        };
        let line = self.facts.source_position(&method.name_span);
        self.emit_adapter_method(
            MethodReceiver::Itself(&receiver.ty),
            generic_context,
            &twin,
            line.as_ref(),
        )
    }

    pub(crate) fn bound_dispatch_requirement(
        &self,
        expression: &Expression,
    ) -> Option<(Type, InterfaceRequirement)> {
        self.bound_requirement(expression)
            .filter(|(bound, _)| self.facts.bound_needs_dispatch(bound))
    }

    pub(crate) fn bound_requirement(
        &self,
        expression: &Expression,
    ) -> Option<(Type, InterfaceRequirement)> {
        let Expression::DotAccess {
            expression: receiver,
            ..
        } = expression.unwrap_parens()
        else {
            return None;
        };
        if !matches!(
            self.facts.strip_and_peel(&receiver.get_type()),
            Type::Parameter(_)
        ) {
            return None;
        }
        let CallOperation::BoundMethod { bound, method } = &self
            .facts
            .checked(expression.unwrap_parens())
            .callee()
            .operation
        else {
            return None;
        };
        let bound = self.facts.peel_alias(bound);
        let requirement = interface_requirements(&bound, |id| self.facts.definition(id))
            .into_iter()
            .find(|requirement| requirement.name == *method)?;
        Some((bound, requirement))
    }

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
        let layout = self.requirement_layout(&requirement, false)?;
        Some(layout.with_return_abi(self, CallableReturnAbi::Value, SlotOrigin::Lisette))
    }

    fn requirement_layout(
        &self,
        requirement: &InterfaceRequirement,
        go: bool,
    ) -> Option<FunctionLayout> {
        let function = requirement.ty.as_function_type()?;
        let declared = if go && !go_name::is_go_import(&requirement.declaring_interface) {
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
            self.facts.return_abi(
                &declared.return_type,
                ReturnSlot::Go {
                    hints: &requirement.method.go_hints,
                },
            )
        } else {
            self.facts.return_abi(
                requirement
                    .method
                    .ty
                    .get_function_ret()
                    .unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &requirement.name,
                    method: &requirement.method,
                },
            )
        };
        Some(self.function_layout(parameters, &result, abi, Some(&declared.return_type)))
    }

    pub(crate) fn dynamic_interface_adapter(
        &self,
        source: &Type,
        target: &Type,
        witness: Option<&ConformanceWitness>,
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
            if !seen.insert(self.facts.method_go_name(&requirement.name, false)) {
                continue;
            }
            let selected = match witness {
                Some(witness) => self.selected_bound_requirement(&name, witness, &requirement),
                None => requirement.clone(),
            };
            let local = self.requirement_layout(&selected, false)?;
            let foreign = self.requirement_layout(&selected, true)?;
            let target = match witness {
                Some(_) => self.requirement_layout(
                    &requirement,
                    go_name::is_go_import(&requirement.declaring_interface),
                )?,
                None => local.with_return_abi(self, CallableReturnAbi::Value, SlotOrigin::Lisette),
            };
            methods.push(AdapterMethod {
                source_go_name: self.facts.requirement_go_name(&selected),
                go_name: self.facts.requirement_go_name(&requirement),
                source_name: selected.name,
                source: local,
                alternate: Some(foreign),
                target,
            });
        }
        Some(AdapterPlan {
            concrete_id: name,
            interface_id: id.as_eco().clone(),
            interface_ty: target.clone(),
            concrete_ty: source.clone(),
            generic_context: self.adapter_generic_context(source, &methods),
            methods,
        })
    }

    fn selected_bound_requirement(
        &self,
        parameter: &str,
        witness: &ConformanceWitness,
        requirement: &InterfaceRequirement,
    ) -> InterfaceRequirement {
        let checked = witness
            .methods
            .iter()
            .find(|method| {
                method.required_interface == requirement.declaring_interface
                    && method.required_name == requirement.name
            })
            .expect("internal error: a checked conformance selects every required method");
        let bound = checked.selected_definition.without_last_segment();
        self.scope
            .type_params()
            .iter()
            .filter(|candidate| candidate.name == parameter)
            .flat_map(|candidate| &candidate.bounds)
            .map(|candidate| self.facts.peel_alias(candidate))
            .filter(|candidate| candidate.get_qualified_id() == bound)
            .flat_map(|candidate| {
                interface_requirements(&candidate, |id| self.facts.definition(id))
            })
            .find(|candidate| candidate.name == checked.selected_name)
            .expect("internal error: a checked conformance selects a method of a bound")
    }

    fn dynamic_method_interface(&mut self, method_name: &str, layout: &FunctionLayout) -> GoType {
        let parameters: Vec<GoType> = layout
            .parameters
            .iter()
            .map(|parameter| parameter.go_type(self))
            .collect();
        let result = layout.result_go_type(self).unwrap_or_default();
        GoType::method_interface(method_name, &parameters, &result)
    }

    fn lower_dynamic_adapter_body(
        &mut self,
        method: &AdapterMethod,
        alternate: &FunctionLayout,
        receiver: &GoIdentifier,
        parameters: &[GoIdentifier],
    ) -> Vec<Statement> {
        let inner = GoExpression::conversion(
            GoType::new("any"),
            GoExpression::selector(
                GoExpression::identifier(receiver.clone()),
                "inner".to_string(),
            ),
        );
        let mut statements = Vec::new();
        if !method.source.same_representation(alternate) {
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
        let foreign_name = self.facts.method_go_name(&method.source_name, false);
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
        let (field_ty, subst_map) = self
            .facts
            .declared_field_ty(struct_ty, variant, field_name)?;
        Some(match subst_map {
            Some(subst_map) => substitute(&field_ty, &subst_map),
            None => field_ty,
        })
    }

    pub(crate) fn interface_adapter(
        &self,
        conversion: &InterfaceConversion,
    ) -> Option<AdapterPlan> {
        let source_ty = &conversion.source;
        let target = self.facts.peel_alias(&conversion.target);
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
                        .any(|bound| self.facts.bound_needs_dispatch(bound))
            })
        {
            return self.dynamic_interface_adapter(source_ty, &target, conversion.witness.as_ref());
        }
        let Type::Nominal { id: source_id, .. } = &source_stripped else {
            return None;
        };
        let methods =
            self.adapted_methods(&source_stripped, &target, conversion.witness.as_ref())?;
        Some(AdapterPlan {
            concrete_id: source_id.as_eco().clone(),
            interface_id: target_id.as_eco().clone(),
            interface_ty: target.clone(),
            concrete_ty: source_ty.clone(),
            generic_context: self.adapter_generic_context(source_ty, &methods),
            methods,
        })
    }

    fn adapted_methods(
        &self,
        source_stripped: &Type,
        target: &Type,
        witness: Option<&ConformanceWitness>,
    ) -> Option<Vec<AdapterMethod>> {
        let requirements = interface_requirements(target, |id| self.facts.definition(id));
        if requirements.is_empty() {
            return None;
        }
        let witness = witness
            .expect("internal error: an interface conversion that requires methods has a checked conformance");
        let source_requirements =
            interface_requirements(source_stripped, |id| self.facts.definition(id));

        let mut methods = Vec::new();
        let mut any_adapted = false;
        let mut seen = HashSet::default();
        for requirement in requirements {
            let selector = self.facts.method_go_name(&requirement.name, false);
            if !seen.insert(selector) {
                continue;
            }
            let checked = witness
                .methods
                .iter()
                .find(|method| {
                    method.required_interface == requirement.declaring_interface
                        && method.required_name == requirement.name
                })
                .expect("internal error: a checked conformance selects every required method");
            let source_requirement = source_requirements
                .iter()
                .find(|source| source.name == checked.selected_name);
            let method = self.build_adapter_method(
                &requirement,
                source_stripped,
                source_requirement,
                checked,
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
        concrete_ty: &Type,
        source_requirement: Option<&InterfaceRequirement>,
        checked: &CheckedMethod,
    ) -> Option<AdapterMethod> {
        let implementation_name = &checked.selected_name;
        let function = checked.declared_signature.as_function_type()?;
        let (_, parameters) = function.params.split_first()?;
        let param_types: Vec<Type> = parameters
            .iter()
            .map(|param| substitute(&param.ty, &checked.substitutions))
            .collect();
        let return_type = substitute(&function.return_type, &checked.substitutions);

        let source_owner = source_requirement
            .map(|source| source.declaring_interface.as_str())
            .or_else(|| {
                checked
                    .selected_definition
                    .rsplit_once('.')
                    .map(|(owner, _)| owner)
            })
            .or_else(|| concrete_ty.get_qualified_id())?;
        let source_go_method = go_name::is_go_import(source_owner)
            .then(|| format!("{source_owner}.{implementation_name}"));
        let source_return = source_requirement
            .and_then(|source| source.method.ty.get_function_ret())
            .unwrap_or(&function.return_type);
        let user_abi = if let Some(id) = &source_go_method {
            self.facts.go_callable_return(id).cloned().map_or_else(
                || CallableReturnAbi::unlowered(source_return),
                CallableReturnAbi::Lowered,
            )
        } else if let Some(source) = source_requirement {
            self.facts.return_abi(
                source.method.ty.get_function_ret().unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &source.name,
                    method: &source.method,
                },
            )
        } else if self.facts.requirement_uses_tagged_twin(requirement) {
            CallableReturnAbi::Value
        } else {
            self.facts
                .return_abi(&function.return_type, ReturnSlot::Method)
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
            None => self.facts.return_abi(
                requirement
                    .method
                    .ty
                    .get_function_ret()
                    .unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &requirement.name,
                    method: &requirement.method,
                },
            ),
        };
        let parameters = self.interface_parameter_layouts(go_method.as_deref(), &param_types, 0);
        let target = if abi_matches_type(&interface_abi, &peeled_return) {
            let result = self.interface_result_layout(go_method.as_deref(), &peeled_return);
            self.function_layout(parameters, &result, interface_abi, Some(interface_return))
        } else {
            self.function_layout(
                parameters,
                &user_result,
                source.return_abi.clone(),
                Some(&function.return_type),
            )
        };

        let selected_exported = checked.is_exported;
        let source_go_name = match source_requirement {
            Some(source) => self.facts.requirement_go_name(source),
            None if source_go_method.is_none()
                && self.facts.requirement_uses_tagged_twin(requirement) =>
            {
                go_name::tagged_twin(
                    &self
                        .facts
                        .method_go_name(implementation_name, selected_exported),
                )
            }
            None => self
                .facts
                .method_go_name(implementation_name, selected_exported),
        };
        Some(AdapterMethod {
            go_name: self.facts.requirement_go_name(requirement),
            source_name: implementation_name.clone(),
            source_go_name,
            alternate: None,
            source,
            target,
        })
    }

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

    pub(crate) fn ensure_adapter_type(&mut self, plan: AdapterPlan) -> String {
        let key = (plan.concrete_ty.clone(), plan.interface_ty.clone());
        let cacheable = plan.generic_context.is_empty();
        if cacheable
            && let Some(name) = self.adapter_registry.lookup(&key)
            && !self.is_declared(name)
        {
            return name.to_string();
        }

        let scope = &self.scope;
        let base_name = self
            .adapter_registry
            .register(cacheable.then_some(key), |index| {
                let mut name = adapter_type_name(&plan, index);
                while scope.is_go_name_declared(&name) {
                    name.push('_');
                }
                name
            });
        let generic_parameters = self.plan_generics(&plan.generic_context);
        let receiver_type = GoType::named(
            &base_name,
            &plan
                .generic_context
                .iter()
                .map(|(name, _)| GoType::new(self.generic_go_name(name).into_owned()))
                .collect::<Vec<_>>(),
            None,
        );
        let adapter_type = receiver_type.code.clone();

        let mut declarations = vec![GoDeclaration::new(DeclarationKind::Struct {
            name: base_name,
            parameters: generic_parameters,
            fields: vec![FieldDeclaration {
                documentation: Default::default(),
                name: Some("inner".to_string()),
                ty: self.go_type(&plan.concrete_ty),
                tag: None,
            }],
        })];
        for method in &plan.methods {
            let line = self.facts.adapter_method_line(&plan, method);
            declarations.push(self.emit_adapter_method(
                MethodReceiver::Adapter(&receiver_type),
                &plan.generic_context,
                method,
                line.as_ref(),
            ));
        }
        self.namespace.collect_adapters(declarations);
        adapter_type
    }

    fn emit_adapter_method(
        &mut self,
        receiver: MethodReceiver<'_>,
        generic_context: &[(EcoString, Vec<Type>)],
        method: &AdapterMethod,
        line: Option<&Position>,
    ) -> GoDeclaration {
        self.with_declaration_scope(|this| {
            this.set_type_params(generic_context);
            let (receiver_type, receiver_name, wraps_inner) = match receiver {
                MethodReceiver::Adapter(ty) => (ty.clone(), "a".to_string(), true),
                MethodReceiver::Itself(ty) => {
                    let receiver_type = this.go_type(ty);
                    let name = fresh_receiver_name(&receiver_type.code, |name| {
                        this.shadows_declaration(name)
                    });
                    (receiver_type, name, false)
                }
            };
            let receiver_name = this.declare_adapter_method_binding(receiver_name);
            let param_names: Vec<GoIdentifier> = (0..method.target.parameters.len())
                .map(|i| this.declare_adapter_method_binding(format!("arg{}", i)))
                .collect();

            let parameters = param_names
                .iter()
                .zip(&method.target.parameters)
                .map(|(name, layout)| GoParameter::new(name.clone(), layout.go_type(this)))
                .collect();

            let go_method_name = method.go_name.clone();
            let mut bindings = Vec::with_capacity(param_names.len() + 1);
            bindings.push(receiver_name.clone());
            bindings.extend(param_names);
            let (go_result, body) = this.build_adapter_body(method, wraps_inner, &bindings, line);
            let mut plan = GoDeclaration::new(DeclarationKind::Function {
                signature: FunctionSignature {
                    receiver: Some(GoParameter::new(receiver_name, receiver_type)),
                    name: go_method_name,
                    generics: Vec::new(),
                    parameters,
                    result: go_result,
                },
                body,
            });
            plan.position = line.cloned();
            plan
        })
    }

    fn declare_adapter_method_binding(&mut self, preferred: String) -> GoIdentifier {
        if self.try_declare(&preferred) {
            return GoIdentifier::local(preferred, self.scope.new_local_id());
        }
        let name = self.fresh_like(&GoIdentifier::name(preferred));
        self.declare(&name);
        name
    }

    fn build_adapter_body(
        &mut self,
        method: &AdapterMethod,
        wraps_inner: bool,
        parameters: &[GoIdentifier],
        line: Option<&Position>,
    ) -> (GoType, LoweredBlock) {
        let (go_result, statements) = self.plan_adapter_body(method, wraps_inner, parameters);
        let statements = directed_first(line.cloned(), statements);
        let body = self.finish_function_body(
            LoweredBlock { statements },
            &mut parameters.to_vec(),
            !go_result.is_empty(),
            BodyOrigin::Adapter,
        );
        (go_result, body)
    }

    fn plan_adapter_body(
        &mut self,
        method: &AdapterMethod,
        wraps_inner: bool,
        bindings: &[GoIdentifier],
    ) -> (GoType, Vec<Statement>) {
        let (receiver, parameters) = bindings
            .split_first()
            .expect("an adapter method binds its receiver");
        if let Some(alternate) = &method.alternate {
            let go_result = method.target.result_go_type(self).unwrap_or_default();
            return (
                go_result,
                self.lower_dynamic_adapter_body(method, alternate, receiver, parameters),
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

        let return_type = method.source.result_layout.logical_type();
        let go_result = method.target.result_go_type(self).unwrap_or_default();
        if method.target.return_abi == CallableReturnAbi::Void {
            statements.push(expression_statement(inner_call));
        } else if method.source.return_abi == CallableReturnAbi::Void {
            let zero = self.go_zero(return_type);
            statements.push(expression_statement(inner_call));
            statements.push(plain_return(zero));
        } else if method.source.same_result_representation(&method.target) {
            statements.push(plain_return(inner_call));
        } else {
            self.plan_function_result_bridge(
                &mut statements,
                inner_call,
                &method.source,
                &method.target,
            );
        }
        (go_result, statements)
    }
}

enum MethodReceiver<'a> {
    Adapter(&'a GoType),
    Itself(&'a Type),
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
            method
                .source
                .result_layout
                .logical_type()
                .contains_type(&parameter)
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

impl EmitFacts<'_> {
    pub(crate) fn requirement_uses_tagged_twin(&self, requirement: &InterfaceRequirement) -> bool {
        !go_name::is_go_import(&requirement.declaring_interface)
            && self.method_uses_tagged_return(&requirement.name)
    }

    fn has_tagged_twin(&self, owner: &str, member: &str) -> bool {
        !go_name::is_go_import(owner)
            && !go_name::is_prelude_id(owner)
            && self.method_uses_tagged_return(member)
            && self.emits_go_method(owner, member)
    }

    pub(crate) fn declared_field_ty(
        &self,
        struct_ty: &Type,
        variant: Option<&str>,
        field_name: &str,
    ) -> Option<(Type, Option<SubstitutionMap>)> {
        let stripped = self.strip_and_peel(struct_ty);
        let Type::Nominal { id, params, .. } = &stripped else {
            return None;
        };
        let (field_ty, generics) = match (&self.definition(id.as_str())?.body, variant) {
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
        self.resolve_to_function_type(ty).is_some()
    }

    pub(crate) fn interface_method(&self, interface_ty: &Type, name: &str) -> Option<Method> {
        if !self.is_interface(interface_ty) {
            return None;
        }
        interface_requirements(&self.peel_alias(interface_ty), |id| self.definition(id))
            .into_iter()
            .find(|requirement| requirement.name == name)
            .map(|requirement| requirement.method)
    }

    pub(crate) fn definition_selects_tagged_twin(&self, symbol: &str) -> bool {
        symbol
            .rsplit_once('.')
            .is_some_and(|(owner, name)| self.has_tagged_twin(&self.peel_alias_id(owner), name))
    }

    fn requirement_go_name(&self, requirement: &InterfaceRequirement) -> String {
        let name = self.method_go_name(&requirement.name, false);
        if self.requirement_uses_tagged_twin(requirement) {
            go_name::tagged_twin(&name)
        } else {
            name
        }
    }

    fn adapter_method_line(&self, plan: &AdapterPlan, method: &AdapterMethod) -> Option<Position> {
        self.method(&plan.concrete_id, &method.source_name)
            .and_then(|source| source.name_span.as_ref())
            .and_then(|span| self.source_position(span))
            .or_else(|| {
                self.definition(&plan.interface_id)
                    .and_then(|interface| interface.name_span.as_ref())
                    .and_then(|span| self.source_position(span))
            })
    }

    pub(crate) fn bound_needs_dispatch(&self, bound: &Type) -> bool {
        interface_requirements(&self.peel_alias(bound), |id| self.definition(id))
            .iter()
            .any(|requirement| {
                !go_name::is_go_import(&requirement.declaring_interface)
                    && requirement.ty.get_function_ret().is_some_and(|ty| {
                        matches!(
                            self.return_abi(ty, ReturnSlot::Method),
                            CallableReturnAbi::Lowered(LoweredReturnAbi::Partial { .. })
                        )
                    })
            })
    }
}
