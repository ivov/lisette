use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::emit_facts::ref_stripped_type_parameter;
use crate::lookup::facts::value_layout::{FunctionLayout, ResultProducer, ValueLayout};
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::go_callable_return_abi;
use crate::lowering::abi::return_abi;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::functions::BodyOrigin;
use crate::output::finish::verify::BodyResult;
use crate::target::ir::bodies::effect_statement;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, define_many, directed_first,
};
use crate::target::ir::declarations::{
    DeclarationKind, FieldDeclaration, FunctionSignature, GoDeclaration,
};
use crate::target::ir::go_expression::GoParameter;
use crate::target::ir::go_expression::Position;
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name;
use crate::target::names::go_name::{GoExport, MemberSpelling};
use crate::target::names::receiver::fresh_receiver_name;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi};
use ecow::EcoString;
use rustc_hash::FxHashSet;
use syntax::ast::{Expression, FunctionDefinitionView};
use syntax::program::{CallOperation, CheckedMethod, ConformanceWitness, InterfaceConversion};
use syntax::program::{
    CheckedCallee, DotAccessResolution, InterfaceRequirement, interface_requirements,
};
use syntax::program::{SlotOrigin, is_go_import, is_prelude_symbol, strip_go_prefix};
use syntax::types::{Type, unqualified_name};

#[derive(Debug, Clone)]
pub(in crate::lowering) struct AdapterPlan {
    concrete_id: EcoString,
    interface_id: EcoString,
    interface_ty: Type,
    concrete_ty: Type,
    methods: Vec<AdapterMethod>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct AdapterMethod {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequirementAbi {
    Lisette,
    Go,
}

impl Planner<'_> {
    pub(in crate::lowering) fn member_spelling(&self, expression: &Expression) -> MemberSpelling {
        let selects_twin = matches!(
            expression,
            Expression::DotAccess {
                resolution: DotAccessResolution::InstanceMethodValue { .. }
                    | DotAccessResolution::InstanceMethod { .. },
                ..
            } | Expression::Identifier { .. }
        ) && self
            .facts
            .checked(expression)
            .function_value()
            .is_some_and(|callee| callee_selects_tagged_counterpart(&self.facts, callee));

        if selects_twin {
            MemberSpelling::TaggedCounterpart
        } else {
            MemberSpelling::Plain
        }
    }

    pub(in crate::lowering) fn emit_tagged_counterpart(
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
            .map(|param| self.facts.value_layout(&param.ty, SlotOrigin::Lisette))
            .collect();
        let result = self.facts.value_layout(return_ty, SlotOrigin::Lisette);
        let source = FunctionLayout {
            producer: ResultProducer::Lisette,
            ..self.facts.function_layout(
                parameters.clone(),
                &result,
                return_abi(&self.facts, return_ty, ReturnSlot::Method),
                Some(return_ty),
            )
        };

        let target = self.facts.function_layout(
            parameters,
            &result,
            CallableReturnAbi::Value,
            Some(return_ty),
        );

        let twin = AdapterMethod {
            go_name: go_name::tagged_counterpart(go_name),
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

    pub(in crate::lowering) fn bound_dispatch_requirement(
        &self,
        expression: &Expression,
    ) -> Option<(Type, InterfaceRequirement)> {
        self.bound_requirement(expression)
            .filter(|(bound, _)| bound_needs_dispatch(&self.facts, bound))
    }

    fn bound_requirement(&self, expression: &Expression) -> Option<(Type, InterfaceRequirement)> {
        let CallOperation::BoundMethod { bound, requirement } =
            &self.facts.checked(expression).function_value()?.operation
        else {
            return None;
        };
        Some((bound.clone(), (**requirement).clone()))
    }

    pub(in crate::lowering) fn go_bound_requirement(
        &self,
        expression: &Expression,
    ) -> Option<InterfaceRequirement> {
        self.bound_requirement(expression)
            .map(|(_, requirement)| requirement)
            .filter(|requirement| is_go_import(&requirement.declaring_interface))
    }

    pub(in crate::lowering) fn bound_dispatch_layout(
        &self,
        expression: &Expression,
    ) -> Option<FunctionLayout> {
        let (_, requirement) = self.bound_dispatch_requirement(expression)?;
        self.requirement_value_layout(&requirement)
    }

    fn requirement_value_layout(
        &self,
        requirement: &InterfaceRequirement,
    ) -> Option<FunctionLayout> {
        let layout = self.requirement_layout(requirement, RequirementAbi::Lisette)?;
        Some(layout.with_return_abi(&self.facts, CallableReturnAbi::Value, SlotOrigin::Lisette))
    }

    fn requirement_layout(
        &self,
        requirement: &InterfaceRequirement,
        abi: RequirementAbi,
    ) -> Option<FunctionLayout> {
        let function = requirement.ty.as_function_type()?;
        let declared =
            if abi == RequirementAbi::Go && !is_go_import(&requirement.declaring_interface) {
                function
            } else {
                requirement.method.ty.as_function_type()?
            };

        let parameters = function
            .params
            .iter()
            .zip(&declared.params)
            .map(|(parameter, declared)| {
                let origin = match abi {
                    RequirementAbi::Go => {
                        SlotOrigin::of_go_declaration(SlotOrigin::GoParameter, &declared.ty, |id| {
                            self.facts.definition(id)
                        })
                    }
                    RequirementAbi::Lisette => SlotOrigin::Lisette,
                };

                self.facts
                    .value_layout_with_declaration(&parameter.ty, origin, &declared.ty)
            })
            .collect();

        let origin = match abi {
            RequirementAbi::Go => {
                SlotOrigin::of_go_declaration(SlotOrigin::GoReturn, &declared.return_type, |id| {
                    self.facts.definition(id)
                })
            }
            RequirementAbi::Lisette => SlotOrigin::Lisette,
        };

        let result = self.facts.target_layout_with_declaration(
            &function.return_type,
            origin,
            &declared.return_type,
        );
        let return_abi = match abi {
            RequirementAbi::Go => {
                requirement_go_return_abi(&self.facts, requirement, &declared.return_type)
            }
            RequirementAbi::Lisette => requirement_return_abi(&self.facts, requirement),
        };

        Some(self.facts.function_layout(
            parameters,
            &result,
            return_abi,
            Some(&declared.return_type),
        ))
    }

    pub(in crate::lowering) fn dynamic_interface_adapter(
        &self,
        source: &Type,
        target: &Type,
        witness: Option<&ConformanceWitness>,
    ) -> Option<AdapterPlan> {
        let name = ref_stripped_type_parameter(source)?;
        let Type::Nominal { id, .. } = target else {
            return None;
        };
        let mut methods = Vec::new();
        let mut seen = FxHashSet::default();
        for requirement in interface_requirements(target, |id| self.facts.definition(id)) {
            if !seen.insert(
                self.facts
                    .method_go_name(&requirement.name, GoExport::Unexported),
            ) {
                continue;
            }
            let selected = match witness {
                Some(witness) => witness
                    .method_for(&requirement)
                    .selected_requirement
                    .clone()
                    .expect("internal error: a checked conformance selects a method of a bound"),
                None => requirement.clone(),
            };

            let local = self.requirement_layout(&selected, RequirementAbi::Lisette)?;
            let foreign = self.requirement_layout(&selected, RequirementAbi::Go)?;
            let target = match witness {
                Some(_) => {
                    let abi = if is_go_import(&requirement.declaring_interface) {
                        RequirementAbi::Go
                    } else {
                        RequirementAbi::Lisette
                    };
                    self.requirement_layout(&requirement, abi)?
                }
                None => self.requirement_value_layout(&selected)?,
            };

            methods.push(AdapterMethod {
                source_go_name: requirement_go_name(&self.facts, &selected),
                go_name: requirement_go_name(&self.facts, &requirement),
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

    fn dynamic_method_interface(&self, method_name: &str, layout: &FunctionLayout) -> GoType {
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
            let local = self.fresh_local("receiver");
            let ok = self.fresh_local("ok");
            statements.push(define_many(
                vec![local.clone(), ok.clone()],
                GoExpression::type_assertion(inner.clone(), local_type),
            ));
            let mut branch = Vec::new();
            let arguments = self.plan_layout_changed_arguments(
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

            self.plan_function_result_layout_change(
                &mut branch,
                call,
                &method.source,
                &method.target,
            );
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
        let foreign_name = self
            .facts
            .method_go_name(&method.source_name, GoExport::Unexported);
        let foreign_type = self.dynamic_method_interface(&foreign_name, alternate);
        let foreign = GoExpression::type_assertion(inner, foreign_type);
        let arguments = self.plan_layout_changed_arguments(
            &mut statements,
            parameters
                .iter()
                .map(|parameter| GoExpression::identifier(parameter.clone()))
                .collect(),
            alternate,
            &method.target,
        );

        let call = GoExpression::call(GoExpression::selector(foreign, foreign_name), arguments);
        self.plan_function_result_layout_change(&mut statements, call, alternate, &method.target);
        statements
    }

    pub(in crate::lowering) fn interface_adapter(
        &self,
        conversion: &InterfaceConversion,
    ) -> Option<AdapterPlan> {
        let source_ty = &conversion.source;
        let target = conversion.target_interface.as_ref()?;
        let Type::Nominal { id: target_id, .. } = target else {
            return None;
        };
        let source_shape = &conversion.source_shape;
        if let Type::Parameter(name) = source_shape
            && self.scope.type_params().iter().any(|parameter| {
                &parameter.name == name
                    && parameter
                        .bounds
                        .iter()
                        .any(|bound| bound_needs_dispatch(&self.facts, bound))
            })
        {
            return self.dynamic_interface_adapter(source_ty, target, conversion.witness.as_ref());
        }
        let Type::Nominal { id: source_id, .. } = source_shape else {
            return None;
        };
        let methods = self.adapted_methods(source_shape, target, conversion.witness.as_ref())?;
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

        let mut methods = Vec::new();
        let mut any_adapted = false;
        let mut seen = FxHashSet::default();
        for requirement in requirements {
            let selector = self
                .facts
                .method_go_name(&requirement.name, GoExport::Unexported);
            if !seen.insert(selector) {
                continue;
            }
            let checked = witness.method_for(&requirement);
            let method = self.build_adapter_method(
                &requirement,
                source_stripped,
                checked.selected_requirement.as_ref(),
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
        let instantiated = checked.instantiated_declared_signature.as_function_type()?;
        let (_, parameters) = instantiated.params.split_first()?;
        let param_types: Vec<Type> = parameters.iter().map(|param| param.ty.clone()).collect();
        let return_type = instantiated.return_type.as_ref().clone();

        let source_owner = source_requirement
            .map(|source| source.declaring_interface.as_str())
            .or_else(|| {
                checked
                    .selected_definition
                    .rsplit_once('.')
                    .map(|(owner, _)| owner)
            })
            .or_else(|| concrete_ty.get_qualified_id())?;

        let source_go_method =
            is_go_import(source_owner).then(|| format!("{source_owner}.{implementation_name}"));
        let source_return = source_requirement
            .and_then(|source| source.method.ty.get_function_ret())
            .unwrap_or(&function.return_type);
        let user_abi = if let Some(id) = &source_go_method {
            let hints = self
                .facts
                .method(source_owner, implementation_name)
                .map_or(&[][..], |method| method.go_hints.as_slice());
            go_callable_return_abi(&self.facts, id, source_return, hints)
        } else if let Some(source) = source_requirement {
            requirement_return_abi(&self.facts, source)
        } else if requirement_uses_tagged_counterpart(&self.facts, requirement) {
            CallableReturnAbi::Value
        } else {
            return_abi(&self.facts, &function.return_type, ReturnSlot::Method)
        };

        let user_result = self
            .facts
            .interface_result_layout(source_go_method.as_deref(), &return_type);
        let producer = if source_go_method.is_none() && source_requirement.is_none() {
            ResultProducer::Lisette
        } else {
            ResultProducer::Go
        };
        let source = FunctionLayout {
            producer,
            ..self.facts.function_layout(
                self.interface_parameter_layouts(
                    source_go_method.as_deref(),
                    &param_types,
                    usize::from(source_requirement.is_none()),
                ),
                &user_result,
                user_abi,
                Some(source_return),
            )
        };

        let interface_return = &requirement.method.ty.as_function_type()?.return_type;
        let go_method = is_go_import(&requirement.declaring_interface).then(|| {
            requirement
                .declaring_interface
                .with_segment(&requirement.method.source_name)
        });
        let interface_abi = if go_method.is_some() {
            requirement_go_return_abi(&self.facts, requirement, interface_return)
        } else {
            requirement_return_abi(&self.facts, requirement)
        };
        let parameters = self.interface_parameter_layouts(go_method.as_deref(), &param_types, 0);
        let target = if interface_abi.fits_return_type(&self.facts, &return_type) {
            let result = self
                .facts
                .interface_result_layout(go_method.as_deref(), &return_type);
            self.facts
                .function_layout(parameters, &result, interface_abi, Some(interface_return))
        } else {
            self.facts.function_layout(
                parameters,
                &user_result,
                source.return_abi.clone(),
                Some(&function.return_type),
            )
        };

        Some(AdapterMethod {
            go_name: requirement_go_name(&self.facts, requirement),
            source_name: implementation_name.clone(),
            source_go_name: adapter_source_go_name(
                &self.facts,
                requirement,
                source_requirement,
                Some(source_owner),
                checked,
            ),
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
                    Some(slot) => self.facts.value_layout_with_declaration(
                        ty,
                        slot.origin,
                        &slot.declared_type,
                    ),
                    None => self.facts.value_layout(ty, SlotOrigin::Lisette),
                }
            })
            .collect()
    }

    pub(in crate::lowering) fn ensure_adapter_type(&mut self, plan: AdapterPlan) -> String {
        let key = (plan.concrete_ty.clone(), plan.interface_ty.clone());
        let cacheable = plan.generic_context.is_empty();
        if cacheable
            && let Some(name) = self.adapter_registry.lookup(&key)
            && !self.scope.is_go_name_declared(name)
        {
            return name.to_string();
        }

        let scope = &self.scope;
        let base_name = self
            .adapter_registry
            .register(cacheable.then_some(key), |index| {
                let mut name = plan.type_name(index);
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
            let line = adapter_method_line(&self.facts, &plan, method);
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
        if self.scope.try_declare_go_name(&preferred) {
            return GoIdentifier::local(preferred, self.scope.new_local_id());
        }
        let name = self.fresh_local(&GoIdentifier::name(preferred));
        self.scope.declare_go_name(&name);
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
        let body_result = if go_result.is_empty() {
            BodyResult::Void
        } else {
            BodyResult::Value
        };
        let body = self.finish_function_body(
            LoweredBlock { statements },
            &mut parameters.to_vec(),
            body_result,
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
        let arguments = self.plan_layout_changed_arguments(
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
            statements.push(effect_statement(inner_call));
        } else if method.source.return_abi == CallableReturnAbi::Void {
            let zero = self.go_zero(return_type);
            statements.push(effect_statement(inner_call));
            statements.push(plain_return(zero));
        } else if method.source.same_result_representation(&method.target) {
            statements.push(plain_return(inner_call));
        } else {
            self.plan_function_result_layout_change(
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

pub(in crate::lowering) fn bound_needs_dispatch(facts: &EmitFacts<'_>, bound: &Type) -> bool {
    interface_requirements(bound, |id| facts.definition(id))
        .iter()
        .any(|requirement| {
            !is_go_import(&requirement.declaring_interface)
                && matches!(
                    requirement_return_abi(facts, requirement),
                    CallableReturnAbi::Lowered(LoweredReturnAbi::Partial { .. })
                )
        })
}

pub(in crate::lowering) fn requirement_go_return_abi(
    facts: &EmitFacts<'_>,
    requirement: &InterfaceRequirement,
    declared_return: &Type,
) -> CallableReturnAbi {
    go_callable_return_abi(
        facts,
        &requirement
            .declaring_interface
            .with_segment(&requirement.method.source_name),
        declared_return,
        &requirement.method.go_hints,
    )
}

fn requirement_return_abi(
    facts: &EmitFacts<'_>,
    requirement: &InterfaceRequirement,
) -> CallableReturnAbi {
    return_abi(
        facts,
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
}

fn requirement_uses_tagged_counterpart(
    facts: &EmitFacts<'_>,
    requirement: &InterfaceRequirement,
) -> bool {
    !is_go_import(&requirement.declaring_interface)
        && facts.method_uses_tagged_return(&requirement.name)
}

pub(in crate::lowering) fn has_tagged_counterpart(
    facts: &EmitFacts<'_>,
    owner: &str,
    member: &str,
) -> bool {
    !is_go_import(owner)
        && !is_prelude_symbol(owner)
        && facts.method_uses_tagged_return(member)
        && facts.emits_go_method(owner, member)
}

fn callee_selects_tagged_counterpart(facts: &EmitFacts<'_>, callee: &CheckedCallee) -> bool {
    match callee.interface_requirement() {
        Some(requirement) => requirement_uses_tagged_counterpart(facts, requirement),
        None => callee
            .definition
            .as_deref()
            .is_some_and(|symbol| definition_selects_tagged_counterpart(facts, symbol)),
    }
}

pub(in crate::lowering) fn definition_selects_tagged_counterpart(
    facts: &EmitFacts<'_>,
    symbol: &str,
) -> bool {
    symbol
        .rsplit_once('.')
        .is_some_and(|(owner, name)| has_tagged_counterpart(facts, owner, name))
}

fn adapter_source_go_name(
    facts: &EmitFacts<'_>,
    requirement: &InterfaceRequirement,
    source_requirement: Option<&InterfaceRequirement>,
    source_owner: Option<&str>,
    checked: &CheckedMethod,
) -> String {
    match source_requirement {
        Some(source) => requirement_go_name(facts, source),
        None => {
            let export = if checked.is_exported {
                GoExport::Exported
            } else {
                GoExport::Unexported
            };
            let spelling = if !source_owner.is_some_and(is_go_import)
                && requirement_uses_tagged_counterpart(facts, requirement)
            {
                MemberSpelling::TaggedCounterpart
            } else {
                MemberSpelling::Plain
            };

            facts.method_selector_go_name(source_owner, &checked.selected_name, export, spelling)
        }
    }
}

fn requirement_go_name(facts: &EmitFacts<'_>, requirement: &InterfaceRequirement) -> String {
    let spelling = if requirement_uses_tagged_counterpart(facts, requirement) {
        MemberSpelling::TaggedCounterpart
    } else {
        MemberSpelling::Plain
    };
    facts.method_selector_go_name(
        Some(&requirement.declaring_interface),
        &requirement.name,
        GoExport::Unexported,
        spelling,
    )
}

fn adapter_method_line(
    facts: &EmitFacts<'_>,
    plan: &AdapterPlan,
    method: &AdapterMethod,
) -> Option<Position> {
    facts
        .method(&plan.concrete_id, &method.source_name)
        .and_then(|source| source.name_span.as_ref())
        .and_then(|span| facts.source_position(span))
        .or_else(|| {
            facts
                .definition(&plan.interface_id)
                .and_then(|interface| interface.name_span.as_ref())
                .and_then(|span| facts.source_position(span))
        })
}

impl AdapterPlan {
    fn type_name(&self, index: usize) -> String {
        let concrete_name = self
            .concrete_id
            .rsplit('.')
            .next()
            .unwrap_or(self.concrete_id.as_str());
        let go_path = strip_go_prefix(&self.interface_id).unwrap_or(self.interface_id.as_str());
        let iface_name = unqualified_name(go_path);
        format!(
            "{}{}_{}_{}",
            go_name::ADAPTER_TYPE_PREFIX,
            concrete_name,
            iface_name,
            index
        )
    }
}
