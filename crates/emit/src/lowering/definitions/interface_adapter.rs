use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::return_abi_rules::interface_method_return_abi;
use crate::lookup::facts::value_layout::{FunctionLayout, ResultProducer, ValueLayout};
use crate::lowering::abi::ReturnSlot;
use crate::lowering::abi::go_callable_return_abi;
use crate::lowering::abi::return_abi;
use crate::lowering::control_flow::propagation::plain_return;
use crate::lowering::definitions::functions::BodyOrigin;
use crate::lowering::planner::queries::PlannerQueries;
use crate::output::finish::verify::BodyResult;
use crate::target::ir::bodies::effect_statement;
use crate::target::ir::bodies::{LoweredBlock, Statement, directed_first};
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
use crate::target::types::return_abi::CallableReturnAbi;
use ecow::EcoString;
use rustc_hash::FxHashSet;
use syntax::ast::{Binding, Expression, FunctionDefinitionView};
use syntax::program::{CallOperation, CheckedMethod, ConformanceWitness, InterfaceConversion};
use syntax::program::{
    CalleeName, CheckedCallee, DotAccessResolution, InterfaceRequirement, MethodName,
    interface_requirements,
};
use syntax::program::{SlotOrigin, is_go_import, is_prelude_symbol, strip_go_prefix};
use syntax::types::{Symbol, Type, unqualified_name};

#[derive(Debug, Clone)]
pub(in crate::lowering) struct AdapterPlan {
    concrete_id: Symbol,
    interface_id: Symbol,
    interface_ty: Type,
    concrete_ty: Type,
    methods: Vec<AdapterMethod>,
    generic_context: Vec<(EcoString, Vec<Type>)>,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct AdapterMethod {
    go_name: String,
    source_name: EcoString,
    source_owner: Option<Symbol>,
    source_go_name: String,
    source: FunctionLayout,
    target: FunctionLayout,
}

impl AdapterMethod {
    fn changes_signature(&self) -> bool {
        self.go_name != self.source_go_name || !self.source.same_representation(&self.target)
    }
}

#[derive(Clone, Copy)]
pub(in crate::lowering) struct MethodParameters<'a> {
    pub(in crate::lowering) receiver: &'a Binding,
    pub(in crate::lowering) rest: &'a [Binding],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum MethodBody {
    InPlain,
    InCounterpart,
}

impl Planner<'_> {
    pub(in crate::lowering) fn emit_tagged_counterpart(
        &mut self,
        method: FunctionDefinitionView<'_>,
        method_parameters: MethodParameters<'_>,
        go_name: &str,
        generic_context: &[(EcoString, Vec<Type>)],
        method_body: MethodBody,
    ) -> GoDeclaration {
        let MethodParameters {
            receiver,
            rest: params,
        } = method_parameters;
        let return_ty = method.return_type;
        let parameters: Vec<ValueLayout> = params
            .iter()
            .map(|param| self.facts.value_layout(&param.ty, SlotOrigin::Lisette))
            .collect();
        let result = self.facts.value_layout(return_ty, SlotOrigin::Lisette);
        let layout = |abi| FunctionLayout {
            producer: ResultProducer::Lisette,
            ..self
                .facts
                .function_layout(parameters.clone(), &result, abi, Some(return_ty))
        };
        let plain = layout(return_abi(&self.facts, return_ty, ReturnSlot::Method));
        let tagged = layout(CallableReturnAbi::Value);
        let plain_go_name = go_name.to_string();
        let tagged_go_name = go_name::tagged_counterpart(go_name);
        let (go_name, target, source_go_name, source) = match method_body {
            MethodBody::InPlain => (tagged_go_name, tagged, plain_go_name, plain),
            MethodBody::InCounterpart => (plain_go_name, plain, tagged_go_name, tagged),
        };

        let twin = AdapterMethod {
            go_name,
            source_name: method.name.clone(),
            source_owner: None,
            source_go_name,
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

    pub(in crate::lowering) fn ensure_adapter_type(&mut self, plan: AdapterPlan) -> GoType {
        let key = (plan.concrete_ty.clone(), plan.interface_ty.clone());
        let cacheable = plan.generic_context.is_empty();
        if cacheable
            && let Some(name) = self.adapter_registry.lookup(&key)
            && !self.scope.is_go_name_declared(name)
        {
            return GoType::named(name, &[], None);
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

        let generic_parameters = self.queries().plan_generics(&plan.generic_context);
        let receiver_type = GoType::named(
            &base_name,
            &plan
                .generic_context
                .iter()
                .map(|(name, _)| GoType::new(self.names.generic_go_name(name).into_owned()))
                .collect::<Vec<_>>(),
            None,
        );

        let mut declarations = vec![GoDeclaration::new(DeclarationKind::Struct {
            name: base_name,
            parameters: generic_parameters,
            fields: vec![FieldDeclaration {
                documentation: Default::default(),
                name: Some("inner".to_string()),
                ty: self.queries().go_type(&plan.concrete_ty),
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
        receiver_type
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
                    let receiver_type = this.queries().go_type(ty);
                    let name = fresh_receiver_name(receiver_type.leading_name(), |name| {
                        this.queries().shadows_declaration(name)
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
                .map(|(name, layout)| {
                    GoParameter::new(name.clone(), layout.go_type(&this.queries()))
                })
                .collect();

            let go_method_name = method.go_name.clone();
            let bindings = AdapterBindings {
                receiver: receiver_name.clone(),
                parameters: param_names,
            };
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
        bindings: &AdapterBindings,
        line: Option<&Position>,
    ) -> (Option<GoType>, LoweredBlock) {
        let (go_result, statements) = self.plan_adapter_body(method, wraps_inner, bindings);
        let statements = directed_first(line.cloned(), statements);
        let body_result = if go_result.is_some() {
            BodyResult::Value
        } else {
            BodyResult::Void
        };
        let mut parameters = vec![bindings.receiver.clone()];
        parameters.extend(bindings.parameters.iter().cloned());
        let body = self.finish_function_body(
            LoweredBlock { statements },
            &mut parameters,
            body_result,
            BodyOrigin::Adapter,
        );

        (go_result, body)
    }

    fn plan_adapter_body(
        &mut self,
        method: &AdapterMethod,
        wraps_inner: bool,
        bindings: &AdapterBindings,
    ) -> (Option<GoType>, Vec<Statement>) {
        let AdapterBindings {
            receiver,
            parameters,
        } = bindings;
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

        let go_result = method.target.result_go_type(&self.queries());
        self.push_adapter_result(&mut statements, method, inner_call);
        (go_result, statements)
    }

    fn push_adapter_result(
        &mut self,
        statements: &mut Vec<Statement>,
        method: &AdapterMethod,
        inner_call: GoExpression,
    ) {
        if method.target.return_abi() == CallableReturnAbi::Void {
            statements.push(effect_statement(inner_call));
        } else if method.source.return_abi() == CallableReturnAbi::Void {
            let zero = self.go_zero(method.source.result_layout.logical_type());
            statements.push(effect_statement(inner_call));
            statements.push(plain_return(zero));
        } else if method.source.same_result_representation(&method.target) {
            statements.push(plain_return(inner_call));
        } else {
            self.plan_function_result_layout_change(
                statements,
                inner_call,
                &method.source,
                &method.target,
            );
        }
    }
}

fn adapter_source_owner<'s>(
    source_requirement: Option<&'s InterfaceRequirement>,
    checked: &'s CheckedMethod,
) -> &'s Symbol {
    source_requirement.map_or(&checked.selected_owner, |source| {
        &source.declaring_interface
    })
}

struct AdapterBindings {
    receiver: GoIdentifier,
    parameters: Vec<GoIdentifier>,
}

struct AdapterSource<'s> {
    owner: &'s Symbol,
    go_method: Option<CalleeName>,
    requirement: Option<&'s InterfaceRequirement>,
    return_type: &'s Type,
}

struct ResultFallback<'r> {
    layout: &'r ValueLayout,
    abi: CallableReturnAbi,
    declared: &'r Type,
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

pub(in crate::lowering) fn requirement_go_return_abi(
    facts: &EmitFacts<'_>,
    requirement: &InterfaceRequirement,
    declared_return: &Type,
) -> CallableReturnAbi {
    go_callable_return_abi(
        facts,
        &requirement_callee(requirement),
        declared_return,
        &requirement.method.go_hints,
    )
}

fn requirement_callee(requirement: &InterfaceRequirement) -> CalleeName {
    CalleeName::Method(MethodName::new(
        requirement.declaring_interface.clone(),
        &requirement.method.source_name,
    ))
}

fn emits_lisette_method(facts: &EmitFacts<'_>, owner: &Symbol, member: &str) -> bool {
    !is_go_import(owner) && !is_prelude_symbol(owner) && facts.emits_go_method(owner, member)
}

pub(in crate::lowering) fn tagged_counterpart_body(
    facts: &EmitFacts<'_>,
    owner: &Symbol,
    member: &str,
) -> Option<MethodBody> {
    if !emits_lisette_method(facts, owner, member) {
        None
    } else if facts.method_returns_partial(owner, member) {
        Some(MethodBody::InCounterpart)
    } else if facts.method_uses_tagged_return(member) {
        Some(MethodBody::InPlain)
    } else {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum MethodUse {
    Call,
    Value,
}

fn callee_selects_tagged_counterpart(
    facts: &EmitFacts<'_>,
    callee: &CheckedCallee,
    method_use: MethodUse,
) -> bool {
    if let Some(requirement) = callee.interface_requirement() {
        return facts.requirement_uses_tagged_counterpart(requirement);
    }
    let Some(method) = callee.name.as_ref().and_then(CalleeName::method) else {
        return false;
    };
    match method_use {
        MethodUse::Call => {
            tagged_counterpart_body(facts, &method.owner, &method.name)
                == Some(MethodBody::InCounterpart)
        }
        MethodUse::Value => tagged_counterpart_body(facts, &method.owner, &method.name).is_some(),
    }
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
                && facts.requirement_uses_tagged_counterpart(requirement)
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
    let spelling = if facts.requirement_uses_tagged_counterpart(requirement) {
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
    method
        .source_owner
        .as_ref()
        .map(|owner| facts.known_method(owner, &method.source_name))
        .and_then(|source| source.name_span.as_ref())
        .and_then(|span| facts.source_position(span))
        .or_else(|| {
            facts
                .definition(&plan.interface_id)
                .name_span
                .as_ref()
                .and_then(|span| facts.source_position(span))
        })
}

impl AdapterPlan {
    fn type_name(&self, index: usize) -> String {
        let concrete_name = unqualified_name(&self.concrete_id);
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

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn member_spelling(
        &self,
        expression: &Expression,
        method_use: MethodUse,
    ) -> MemberSpelling {
        let selects_twin = matches!(
            expression,
            Expression::DotAccess {
                resolution: DotAccessResolution::InstanceMethodValue { .. }
                    | DotAccessResolution::InstanceMethod { .. },
                ..
            } | Expression::Identifier { .. }
        ) && self.facts.checked(expression).function_value().is_some_and(
            |callee| callee_selects_tagged_counterpart(self.facts, callee, method_use),
        );

        if selects_twin {
            MemberSpelling::TaggedCounterpart
        } else {
            MemberSpelling::Plain
        }
    }

    pub(in crate::lowering) fn go_bound_requirement(
        &self,
        expression: &Expression,
    ) -> Option<InterfaceRequirement> {
        let CallOperation::BoundMethod { requirement, .. } =
            &self.facts.checked(expression).function_value()?.operation
        else {
            return None;
        };
        is_go_import(&requirement.declaring_interface).then(|| (**requirement).clone())
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
        let Type::Nominal { id: source_id, .. } = &conversion.source_shape else {
            return None;
        };
        let methods = self.adapted_methods(target, conversion.witness.as_ref())?;
        Some(AdapterPlan {
            concrete_id: source_id.clone(),
            interface_id: target_id.clone(),
            interface_ty: target.clone(),
            concrete_ty: source_ty.clone(),
            generic_context: self.adapter_generic_context(source_ty, &methods),
            methods,
        })
    }

    fn adapted_methods(
        &self,
        target: &Type,
        witness: Option<&ConformanceWitness>,
    ) -> Option<Vec<AdapterMethod>> {
        let requirements = interface_requirements(target, self.facts.type_query_lookup());
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
        source_requirement: Option<&InterfaceRequirement>,
        checked: &CheckedMethod,
    ) -> Option<AdapterMethod> {
        let implementation_name = &checked.selected_name;
        let function = checked.declared_signature.as_function_type()?;
        let instantiated = checked.instantiated_declared_signature.as_function_type()?;
        let (_, parameters) = instantiated.params.split_first()?;
        let param_types: Vec<Type> = parameters.iter().map(|param| param.ty.clone()).collect();
        let return_type = instantiated.return_type.as_ref().clone();

        let source_owner = adapter_source_owner(source_requirement, checked);

        let adapter_source = AdapterSource {
            owner: source_owner,
            go_method: is_go_import(source_owner).then(|| {
                CalleeName::Method(MethodName::new(source_owner.clone(), implementation_name))
            }),
            requirement: source_requirement,
            return_type: source_requirement
                .and_then(|source| source.method.ty.as_function_type())
                .map_or(&function.return_type, |source| &source.return_type),
        };
        let user_abi = self.adapter_source_abi(
            &adapter_source,
            requirement,
            implementation_name,
            &function.return_type,
        );
        let user_result = self
            .facts
            .interface_result_layout(adapter_source.go_method.as_ref(), &return_type);
        let source =
            self.adapter_source_layout(&adapter_source, &param_types, &user_result, user_abi);

        let fallback = ResultFallback {
            layout: &user_result,
            abi: source.return_abi(),
            declared: &function.return_type,
        };
        let target =
            self.adapter_target_layout(requirement, &param_types, &return_type, fallback)?;

        Some(AdapterMethod {
            go_name: requirement_go_name(self.facts, requirement),
            source_name: implementation_name.clone(),
            source_owner: Some(checked.selected_owner.clone()),
            source_go_name: adapter_source_go_name(
                self.facts,
                requirement,
                source_requirement,
                Some(source_owner),
                checked,
            ),
            source,
            target,
        })
    }

    fn adapter_source_abi(
        &self,
        source: &AdapterSource<'_>,
        requirement: &InterfaceRequirement,
        implementation_name: &str,
        declared_return: &Type,
    ) -> CallableReturnAbi {
        if let Some(id) = &source.go_method {
            let hints = &self
                .facts
                .known_method(source.owner, implementation_name)
                .go_hints;
            go_callable_return_abi(self.facts, id, source.return_type, hints)
        } else if let Some(source_requirement) = source.requirement {
            interface_method_return_abi(self.facts, &source_requirement.method)
        } else if self.facts.requirement_uses_tagged_counterpart(requirement) {
            CallableReturnAbi::Value
        } else {
            return_abi(self.facts, declared_return, ReturnSlot::Method)
        }
    }

    fn adapter_source_layout(
        &self,
        source: &AdapterSource<'_>,
        param_types: &[Type],
        user_result: &ValueLayout,
        user_abi: CallableReturnAbi,
    ) -> FunctionLayout {
        let producer = if source.go_method.is_none() && source.requirement.is_none() {
            ResultProducer::Lisette
        } else {
            ResultProducer::Go
        };
        FunctionLayout {
            producer,
            ..self.facts.function_layout(
                self.interface_parameter_layouts(
                    source.go_method.as_ref(),
                    param_types,
                    usize::from(source.requirement.is_none()),
                ),
                user_result,
                user_abi,
                Some(source.return_type),
            )
        }
    }

    fn adapter_target_layout(
        &self,
        requirement: &InterfaceRequirement,
        param_types: &[Type],
        return_type: &Type,
        fallback: ResultFallback<'_>,
    ) -> Option<FunctionLayout> {
        let interface_return = &requirement.method.ty.as_function_type()?.return_type;
        let go_method =
            is_go_import(&requirement.declaring_interface).then(|| requirement_callee(requirement));
        let interface_abi = if go_method.is_some() {
            requirement_go_return_abi(self.facts, requirement, interface_return)
        } else {
            interface_method_return_abi(self.facts, &requirement.method)
        };
        let parameters = self.interface_parameter_layouts(go_method.as_ref(), param_types, 0);
        if interface_abi.fits_return_type(self.facts, return_type) {
            let result = self
                .facts
                .interface_result_layout(go_method.as_ref(), return_type);
            return Some(self.facts.function_layout(
                parameters,
                &result,
                interface_abi,
                Some(interface_return),
            ));
        }
        Some(self.facts.function_layout(
            parameters,
            fallback.layout,
            fallback.abi,
            Some(fallback.declared),
        ))
    }

    fn interface_parameter_layouts(
        &self,
        go_method: Option<&CalleeName>,
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
}
