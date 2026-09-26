use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableReturnAbi};
use crate::abi::layout::SlotOrigin;
use crate::abi::tuple_element_types;
use crate::analyze::component_uses::ComponentDemand;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::calls::go_interop::WrapperTarget;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::Fallible;
use crate::escape_reserved;
use crate::patterns::sites::{AnnotatedPattern, PatternSubject};
use crate::plan::bodies::{
    LoweredBlock, LoweredStatement, define, define_many, expression_statement,
};
use crate::plan::placement::{
    collapse_declared_temp, expression_contains_binding, is_unit_call, is_zero_call,
    rebind_trailing_temp, requires_temp_var,
};
use crate::plan::values::GoExpression;
use crate::state::bindings::{ComponentBinding, ComponentKind, TupleBinding};
use std::mem;
use syntax::ast::{Binding, Expression, LetMode, Pattern};
use syntax::program::NativeTypeKind;
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(crate) struct LetSpec<'a> {
    identifier: &'a str,
    value: &'a Expression,
    binding_ty: &'a Type,
    mutable: bool,
}

fn needs_explicit_type_declaration(
    planner: &Planner,
    value: &Expression,
    binding_ty: &Type,
) -> bool {
    if planner.facts.is_interface_or_unknown(binding_ty) {
        let value_ty = value.get_type();
        if *binding_ty != value_ty {
            return true;
        }
    }
    if planner.is_function_alias(binding_ty) {
        let value_ty = value.get_type();
        if matches!(value_ty.unwrap_forall(), Type::Function(_)) {
            return true;
        }
    }
    false
}

/// Pick the Go type for a `let` binding's `var X T` temp. Diverging values
/// use the binding type so dead `return x` paths still typecheck.
fn resolve_let_temp_declaration_ty(
    planner: &Planner,
    value: &Expression,
    binding_ty: &Type,
) -> Type {
    let value_ty = value.get_type();
    if value_ty.is_unit() || value_ty.is_never() {
        if binding_ty.is_unit() || binding_ty.is_variable() || binding_ty.is_placeholder() {
            return value_ty;
        }
        return binding_ty.clone();
    }
    if planner.facts.is_interface_or_unknown(binding_ty) && *binding_ty != value_ty {
        return binding_ty.clone();
    }
    value_ty
}

impl Planner<'_> {
    fn choose_let_go_name(
        &mut self,
        identifier: &str,
        raw_go_name: &str,
        force_fresh: bool,
    ) -> String {
        let escaped = escape_reserved(raw_go_name);
        if force_fresh || self.shadows_declaration(&escaped) {
            self.fresh_var(Some(identifier))
        } else {
            escaped.into_owned()
        }
    }

    /// Lower a `let identifier = value` binding to statements; `raw_go_name ==
    /// None` is unused.
    fn lower_let_value(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: Option<&str>,
    ) -> Vec<LoweredStatement> {
        let LetSpec {
            identifier,
            value,
            binding_ty,
            ..
        } = let_spec;
        if is_unit_call(value) {
            return self.lower_let_unit_call(identifier, raw_go_name, value);
        }
        let needs_temp = requires_temp_var(value);
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            return if needs_temp {
                self.lower_let_temp("_", value, binding_ty)
            } else {
                self.lower_discard_value(value)
            };
        };
        let go_identifier = escape_reserved(raw_go_name);
        if !self.shadows_declaration(&go_identifier)
            && !self.scope.is_active_assign_target(&go_identifier)
            && !self.scope.has_binding_for_go_name(&go_identifier)
            && value.get_type().demoted() == binding_ty.demoted()
        {
            if let Some(statements) = self.lower_fused_result_match_into(value, &go_identifier) {
                self.scope.bind(identifier, raw_go_name);
                return statements;
            }
            if let Some(statements) = self.lower_fused_option_match_into(value, &go_identifier) {
                self.scope.bind(identifier, raw_go_name);
                return statements;
            }
            if let Some(statements) = self.lower_defaulted_call_into(value, &go_identifier) {
                self.scope.bind(identifier, raw_go_name);
                return statements;
            }
            if let Some(statements) = self.lower_slice_loop_into(value, &go_identifier) {
                self.scope.bind(identifier, raw_go_name);
                return statements;
            }
            if let Some(statements) =
                self.lower_let_as_components(identifier, value, &go_identifier)
            {
                return statements;
            }
        }
        if needs_temp {
            if self.shadows_declaration(&go_identifier)
                || expression_contains_binding(value, identifier)
            {
                let fresh = self.fresh_var(Some(identifier));
                let statements = self.lower_let_temp(&fresh, value, binding_ty);
                self.scope.bind(identifier, &fresh);
                return statements;
            }
            self.scope.bind(identifier, raw_go_name);
            return self.lower_let_temp(&go_identifier, value, binding_ty);
        }
        self.lower_let_direct(let_spec, raw_go_name)
    }

    fn lower_let_as_components(
        &mut self,
        identifier: &str,
        value: &Expression,
        go_identifier: &str,
    ) -> Option<Vec<LoweredStatement>> {
        let ty = self.facts.peel_alias(&value.get_type());
        let demand = self.component_lets.get(&value.get_span())?.clone();
        if matches!(ty, Type::Tuple(_)) {
            self.lower_tuple_components(identifier, value, &ty, demand)
        } else {
            self.lower_fallible_components(identifier, value, go_identifier, &ty, demand)
        }
    }

    fn lower_fallible_components(
        &mut self,
        identifier: &str,
        value: &Expression,
        go_identifier: &str,
        ty: &Type,
        demand: ComponentDemand,
    ) -> Option<Vec<LoweredStatement>> {
        let kind = if ty.is_option() {
            ComponentKind::Option
        } else if ty.is_result() {
            ComponentKind::Result
        } else {
            return None;
        };
        let (needs_value, needs_whole_value) = (demand.needs_value, demand.needs_whole_value);
        // Only an Option can be rebuilt from components in one expression.
        if needs_whole_value && kind == ComponentKind::Result {
            return None;
        }
        let payload_go_type = self.use_go_type(&ty.ok_type());
        let slot = if needs_value {
            self.declare(go_identifier);
            CommaOkValueSlot::Named(go_identifier.to_string())
        } else {
            CommaOkValueSlot::Discarded
        };
        let mut pair = match kind {
            ComponentKind::Option => {
                let source = self.comma_ok_source(value)?;
                // With a nil guard, `ok` alone is not the success condition.
                if source.has_nil_guard() {
                    return None;
                }
                self.bind_comma_ok_pair(value, source, slot)
            }
            ComponentKind::Result => {
                let fuse = self.result_fuse_plan(value)?;
                if fuse.has_nil_guard() || fuse.wraps_error() || !fuse.carries_payload() {
                    return None;
                }
                fuse.bind(self, slot, None)
            }
        };
        let statements = mem::take(&mut pair.statements);
        let payload = pair.value().unwrap_or("_").to_string();
        let status = pair.status().to_string();
        self.scope.set_component_binding(
            identifier,
            ComponentBinding {
                value: payload,
                status,
                payload_go_type,
                kind,
            },
        );
        Some(statements)
    }

    fn lower_tuple_components(
        &mut self,
        identifier: &str,
        value: &Expression,
        ty: &Type,
        demand: ComponentDemand,
    ) -> Option<Vec<LoweredStatement>> {
        let elements = tuple_element_types(ty);
        if elements.len() < 2 {
            return None;
        }
        let read_indices = demand.read_indices;
        let needs_whole_value = demand.needs_whole_value;
        let plan = self.plan_call(value)?;
        if !matches!(
            plan.resolved.abi.result,
            CallableReturnAbi::Tuple { arity } if arity == elements.len()
        ) {
            return None;
        }
        // Go rejects a name that no later line reads, so an unread element gets `_`.
        let is_read = |index: usize| needs_whole_value || read_indices.contains(&index);
        if !(0..elements.len()).any(is_read) {
            return None;
        }
        let names: Vec<String> = (0..elements.len())
            .map(|index| {
                if !is_read(index) {
                    return "_".to_string();
                }
                let name = self.fresh_var(Some(&format!("{identifier}{index}")));
                self.declare(&name);
                name
            })
            .collect();
        let (setup, call) = self
            .lower_call(value, None, ExpressionContext::value())
            .into_parts();
        let mut statements = setup;
        statements.push(define_many(names.clone(), call));
        self.scope
            .set_tuple_binding(identifier, TupleBinding { names });
        Some(statements)
    }

    /// `let x = expr?`. Adds a leading `var x T` when the binding widens to
    /// an interface.
    fn lower_let_propagate(
        &mut self,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
        binding_ty: &Type,
    ) -> Vec<LoweredStatement> {
        let Expression::Propagate {
            expression: inner, ..
        } = value
        else {
            unreachable!("lower_let_propagate requires a Propagate value");
        };
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind(identifier, "_");
            return self.lower_propagate(inner, Some("_")).0;
        };
        let go_identifier = self.choose_let_go_name(identifier, raw_go_name, false);
        let widens_to_interface =
            self.facts.is_interface_or_unknown(binding_ty) && *binding_ty != value.get_type();
        let mut statements = Vec::new();
        if widens_to_interface {
            let var_ty = self.use_go_type(binding_ty);
            statements.push(LoweredStatement::VarDecl {
                name: go_identifier.clone(),
                go_type: var_ty,
                value: None,
            });
            self.declare(&go_identifier);
        }
        statements.extend(self.lower_propagate(inner, Some(&go_identifier)).0);
        self.scope.bind(identifier, &go_identifier);
        self.try_declare(&go_identifier);
        statements
    }

    /// `let x = foo()` where `foo()` returns unit: run the call as a
    /// statement, then declare the binding as `struct{}{}`.
    fn lower_let_unit_call(
        &mut self,
        identifier: &str,
        raw_go_name: Option<&str>,
        value: &Expression,
    ) -> Vec<LoweredStatement> {
        let (mut statements, value_expression) = self
            .lower_value(value, ExpressionContext::value())
            .into_parts();
        statements.push(expression_statement(value_expression));
        let Some(raw_go_name) = raw_go_name else {
            return statements;
        };
        let unit = || GoExpression::empty_composite("struct{}".to_string());
        let escaped = escape_reserved(raw_go_name);
        if self.shadows_declaration(&escaped) {
            let fresh = self.fresh_var(Some(identifier));
            self.declare(&fresh);
            statements.push(define(fresh.clone(), unit()));
            self.scope.bind(identifier, &fresh);
        } else {
            let go_identifier = self.scope.bind(identifier, raw_go_name);
            self.try_declare(&go_identifier);
            statements.push(define(go_identifier, unit()));
        }
        statements
    }

    fn lower_let_direct(&mut self, let_spec: LetSpec, raw_go_name: &str) -> Vec<LoweredStatement> {
        let LetSpec {
            identifier,
            value,
            binding_ty,
            mutable,
        } = let_spec;
        if !mutable
            && let Some(statements) =
                self.try_lower_let_into_wrapper_slot(identifier, raw_go_name, value, binding_ty)
        {
            return statements;
        }

        let origin = self.function_type_origin(binding_ty, SlotOrigin::Lisette);
        let plan = self.lower_value(
            value,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let constant = plan.expression.constant_kind();
        let mut statements = plan.setup;
        let coercion = self.value_slot_coercion(value, binding_ty);
        let coercion_is_identity = coercion.is_identity();
        let constant_needs_type =
            coercion_is_identity && self.constant_needs_go_type(constant, binding_ty).is_some();
        let (coercion_setup, value_expression) = coercion.lower(self, plan.expression);
        statements.extend(coercion_setup);

        let bound = self.scope.bind(identifier, raw_go_name);
        let is_new = !self.package.is_package_block_name(&bound) && self.try_declare(&bound);
        let go_identifier = if !is_new || self.scope.is_active_assign_target(&bound) {
            let fresh = self.fresh_var(Some(identifier));
            self.scope.bind(identifier, &fresh);
            self.try_declare(&fresh);
            fresh
        } else {
            bound
        };

        // A bare `var x T` only where the slot's zero is the value.
        if is_zero_call(value)
            && statements.is_empty()
            && coercion_is_identity
            && !needs_explicit_type_declaration(self, value, binding_ty)
        {
            let var_ty = self.use_go_type(binding_ty);
            statements.push(LoweredStatement::VarDecl {
                name: go_identifier,
                go_type: var_ty,
                value: None,
            });
            return statements;
        }

        if constant_needs_type || needs_explicit_type_declaration(self, value, binding_ty) {
            let var_ty = self.use_go_type(binding_ty);
            statements.push(LoweredStatement::VarDecl {
                name: go_identifier,
                go_type: var_ty,
                value: Some(value_expression),
            });
            return statements;
        }
        // A temp no source binding answers to has no other reader to break.
        if let Some(name) = value_expression.as_identifier()
            && !self.scope.has_binding_for_go_name(name)
            && rebind_trailing_temp(&mut statements, &go_identifier, name)
        {
            return statements;
        }
        statements.push(define(go_identifier, value_expression));
        statements
    }

    /// Route a slot-style ABI wrapper into the let's Go name, removing the
    /// `name := result_N` alias.
    fn try_lower_let_into_wrapper_slot(
        &mut self,
        identifier: &str,
        raw_go_name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) -> Option<Vec<LoweredStatement>> {
        let go_identifier = escape_reserved(raw_go_name);
        if self.shadows_declaration(&go_identifier)
            || self.scope.is_active_assign_target(&go_identifier)
            || self.scope.has_binding_for_go_name(&go_identifier)
        {
            return None;
        }
        if value.get_type().demoted() != binding_ty.demoted() {
            return None;
        }
        let plan = self.plan_call(value)?;
        if !matches!(plan.result_transition, AbiTransition::WrapToTagged) {
            return None;
        }
        if matches!(plan.resolved.abi.result, CallableReturnAbi::Tuple { .. }) {
            return None;
        }
        if self.call_result_layout_bridge(&plan, binding_ty).is_some() {
            return None;
        }
        let target = WrapperTarget::Slot(&go_identifier);
        let statements =
            self.lower_abi_wrapped_call_to(value, &plan.resolved.abi, binding_ty, target)?;
        // `push_wrapper_slot` / `push_simple_wrapper_value` already declared
        // `go_identifier`; only the binding from the user-name still needs setup.
        self.scope.bind(identifier, go_identifier.as_ref());
        Some(statements)
    }

    fn lower_let_temp(
        &mut self,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        if !self.is_declared(name) {
            if let Some(declaration) = self.let_temp_var_declaration(name, value, binding_ty) {
                statements.push(declaration);
            }
            self.try_declare(name);
        }
        statements.extend(self.lower_assign(
            value,
            &GoExpression::name(name.to_string()),
            Some(binding_ty),
        ));
        collapse_declared_temp(
            &mut statements,
            name,
            self.short_declaration_keeps_type(binding_ty),
        );
        statements
    }

    fn let_temp_var_declaration(
        &mut self,
        name: &str,
        value: &Expression,
        binding_ty: &Type,
    ) -> Option<LoweredStatement> {
        if name == "_" {
            return None;
        }
        let return_ctx = self.return_ctx();
        let resolved_ty = resolve_let_temp_declaration_ty(self, value, binding_ty);
        let peeled_resolved = self.facts.peel_alias(&resolved_ty);
        let peeled_binding = self.facts.peel_alias(binding_ty);
        let needs_context = |ty: &Type| ty.is_variable() || ty.is_placeholder();
        let has_contextual_ok_ty = matches!(
            value,
            Expression::TryBlock { .. } | Expression::RecoverBlock { .. }
        ) && !needs_context(&peeled_resolved)
            && needs_context(&peeled_resolved.ok_type());

        let var_ty = if has_contextual_ok_ty {
            if !needs_context(&peeled_binding) && !needs_context(&peeled_binding.ok_type()) {
                self.use_go_type(binding_ty)
            } else if let Some(ctx_ty) = return_ctx.ty().cloned() {
                if Fallible::from_type(&ctx_ty).is_some() {
                    self.use_go_type(&ctx_ty)
                } else {
                    self.use_go_type(&resolved_ty)
                }
            } else {
                self.use_go_type(&resolved_ty)
            }
        } else {
            self.use_go_type(&resolved_ty)
        };
        Some(LoweredStatement::VarDecl {
            name: name.to_string(),
            go_type: var_ty,
            value: None,
        })
    }
}

struct LetPlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    binding: &'a Binding,
    value: &'a Expression,
    mode: &'a LetMode,
}

impl<'a, 'e> LetPlanner<'a, 'e> {
    fn build(mut self) -> LoweredBlock {
        // Declare the binding so unreachable code still typechecks.
        if self.value.get_type().is_never() {
            let mut statements = Vec::new();
            if let Pattern::Identifier { identifier, .. } = &self.binding.pattern
                && let Some(raw_go_name) = self.planner.go_name_for_binding(&self.binding.pattern)
            {
                let go_identifier = self.planner.scope.bind(identifier, &raw_go_name);
                self.planner.try_declare(&go_identifier);
                let var_ty = self.planner.use_go_type(&self.binding.ty);
                statements.push(LoweredStatement::VarDecl {
                    name: go_identifier,
                    go_type: var_ty,
                    value: None,
                });
            }
            statements.push(self.planner.lower_statement(self.value));
            return LoweredBlock { statements };
        }

        let pattern = AnnotatedPattern {
            pattern: &self.binding.pattern,
        };
        match self.mode {
            LetMode::Plain => {}
            LetMode::Assert | LetMode::InvalidAssertElse { .. } => {
                return LoweredBlock {
                    statements: self.planner.lower_let_assert_pattern_site(
                        pattern,
                        self.value,
                        self.binding.pattern.get_span(),
                    ),
                };
            }
            LetMode::Else { block, .. } => {
                return LoweredBlock {
                    statements: self
                        .planner
                        .lower_let_else_pattern_site(pattern, self.value, block),
                };
            }
        }

        match &self.binding.pattern {
            Pattern::Identifier { identifier, .. } => {
                return self.lower_simple_identifier(identifier);
            }
            Pattern::WildCard { .. } => return self.lower_discard(),
            Pattern::Tuple { elements, .. } => {
                let all_unused = elements.iter().all(|element| match element {
                    Pattern::WildCard { .. } => true,
                    Pattern::Identifier { .. } => self.planner.facts.is_unused_binding(element),
                    _ => false,
                });
                if all_unused {
                    return self.lower_discard();
                }
                if let Some(block) = self.lower_channel_split(elements) {
                    return block;
                }
                if elements.iter().all(|element| {
                    matches!(
                        element,
                        Pattern::Identifier { .. } | Pattern::WildCard { .. }
                    )
                }) && self.can_use_multi_value_optimization()
                {
                    return self.lower_multi_value_call(elements);
                }
            }
            _ => {}
        }
        let value_ty = self.value.get_type();
        LoweredBlock {
            statements: self.planner.lower_irrefutable_pattern_site(
                PatternSubject::expression(self.value, &self.binding.pattern, None),
                &self.binding.pattern,
                &value_ty,
            ),
        }
    }

    fn lower_channel_split(&mut self, elements: &[Pattern]) -> Option<LoweredBlock> {
        let slot_types = self.channel_split_slot_types(elements)?;
        let call = self.planner.native_method_call(self.value)?;
        if call.kind != NativeTypeKind::Channel || call.method != "split" {
            return None;
        }
        let (mut statements, receiver) = self
            .planner
            .lower_value(call.receiver, ExpressionContext::value())
            .into_parts();
        let receiver = if call.receiver.get_type().is_ref() {
            GoExpression::dereference(receiver)
        } else {
            receiver
        };
        let receiver = self.planner.stable_source(&mut statements, "ch", receiver);
        let planned: Vec<_> = elements
            .iter()
            .zip(slot_types)
            .filter_map(|(pattern, slot_ty)| {
                let Pattern::Identifier { identifier, .. } = pattern else {
                    return None;
                };
                let raw_go_name = self.planner.go_name_for_binding(pattern)?;
                let go_name = self.planner.choose_let_go_name(
                    identifier,
                    &raw_go_name,
                    self.planner.scope.has_binding_for_go_name(&raw_go_name),
                );
                Some((identifier, go_name, self.planner.use_go_type(&slot_ty)))
            })
            .collect();
        for (identifier, go_name, go_type) in planned {
            self.planner.scope.bind(identifier, &go_name);
            self.planner.try_declare(&go_name);
            statements.push(define(
                go_name,
                GoExpression::conversion(go_type, receiver.clone()),
            ));
        }
        Some(LoweredBlock { statements })
    }

    fn channel_split_slot_types(&self, elements: &[Pattern]) -> Option<Vec<Type>> {
        let pair_of_names = elements.len() == 2
            && elements.iter().all(|element| {
                matches!(
                    element,
                    Pattern::Identifier { .. } | Pattern::WildCard { .. }
                )
            });
        if !pair_of_names {
            return None;
        }
        let slot_types = tuple_element_types(&self.planner.facts.peel_alias(&self.binding.ty));
        let directional = [NativeTypeKind::Sender, NativeTypeKind::Receiver];
        (slot_types.len() == directional.len()
            && slot_types.iter().zip(directional).all(|(slot_ty, kind)| {
                NativeTypeKind::from_type(&self.planner.facts.peel_alias(slot_ty)) == Some(kind)
            }))
        .then_some(slot_types)
    }

    fn can_use_multi_value_optimization(&self) -> bool {
        let value_ty = self.value.get_type();
        let slots_read_in_place = tuple_element_types(&self.planner.facts.peel_alias(&value_ty))
            .iter()
            .all(|slot_ty| !self.planner.facts.is_nullable_option(slot_ty));
        slots_read_in_place
            && self.planner.plan_call(self.value).is_some_and(|plan| {
                matches!(plan.resolved.abi.result, CallableReturnAbi::Tuple { .. })
                    && self
                        .planner
                        .go_tuple_result_bridges(&plan.resolved.abi, &value_ty)
                        .is_none()
            })
    }

    fn lower_simple_identifier(&mut self, identifier: &str) -> LoweredBlock {
        let raw_go_name = self.planner.go_name_for_binding(&self.binding.pattern);
        if matches!(self.value, Expression::Propagate { .. }) {
            let statements = self.planner.lower_let_propagate(
                identifier,
                raw_go_name.as_deref(),
                self.value,
                &self.binding.ty,
            );
            return LoweredBlock { statements };
        }
        let statements = self.planner.lower_let_value(
            LetSpec {
                identifier,
                value: self.value,
                binding_ty: &self.binding.ty,
                mutable: self.binding.is_mutable(),
            },
            raw_go_name.as_deref(),
        );
        LoweredBlock { statements }
    }

    fn lower_discard(&mut self) -> LoweredBlock {
        LoweredBlock {
            statements: self.planner.lower_discard_value(self.value),
        }
    }

    fn lower_multi_value_call(&mut self, elements: &[Pattern]) -> LoweredBlock {
        // Bind after lowering the initializer so it sees the previous bindings.
        let planned: Vec<Option<(&str, String)>> = elements
            .iter()
            .map(|pattern| {
                let Pattern::Identifier { identifier, .. } = pattern else {
                    return None;
                };
                if identifier == "_" {
                    return None;
                }
                let go_name = self.planner.go_name_for_binding(pattern)?;
                let escaped = escape_reserved(&go_name).into_owned();
                let name = if self.planner.shadows_declaration(&escaped) {
                    self.planner.fresh_var(Some(identifier))
                } else {
                    escaped
                };
                Some((identifier.as_str(), name))
            })
            .collect();

        let (mut statements, call) = self
            .planner
            .lower_call(self.value, None, ExpressionContext::value())
            .into_parts();

        for (identifier, go_name) in planned.iter().flatten() {
            self.planner.scope.bind(*identifier, go_name);
            self.planner.try_declare(go_name);
        }

        let go_vars = planned
            .iter()
            .map(|binding| binding.as_ref().map_or("_", |(_, name)| name))
            .map(str::to_string)
            .collect();
        statements.push(if planned.iter().any(Option::is_some) {
            define_many(go_vars, call)
        } else {
            LoweredStatement::AssignMany {
                targets: go_vars.into_iter().map(GoExpression::name).collect(),
                value: call,
            }
        });
        LoweredBlock { statements }
    }
}

impl Planner<'_> {
    pub(crate) fn build_let_plan(
        &mut self,
        binding: &Binding,
        value: &Expression,
        mode: &LetMode,
    ) -> LoweredBlock {
        LetPlanner {
            planner: self,
            binding,
            value,
            mode,
        }
        .build()
    }
}
