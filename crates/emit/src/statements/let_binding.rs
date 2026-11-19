use crate::OuterBindings;
use crate::Planner;
use crate::abi::callable::LoweredReturnAbi;
use crate::abi::coercion::CoercionPlan;
use crate::abi::wrapping::WrapperTarget;
use crate::analyze::component_uses::ComponentDemand;
use crate::analyze::component_uses::ValueDemand;
use crate::calls::comma_ok::CommaOkValueSlot;
use crate::escape_local;
use crate::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, assign, define, define_many, effect_statement,
};
use crate::ir::local::GoIdentifier;
use crate::ir::values::{ConstantKind, GoExpression};
use crate::names::source_name::SourceName;
use crate::patterns::decision_tree::collect_pattern_info;
use crate::patterns::sites::PatternSubject;
use crate::plan::placement::{
    collapse_declared_temp, expression_contains_binding, is_zero_call, rebind_trailing_temp,
    rebind_updated_temp, requires_temp_var,
};
use crate::state::bindings::{ComponentKind, TupleBinding};
use crate::state::expression::ExpressionContext;
use syntax::EcoString;
use syntax::ast::{Binding, BindingId, Expression, LetMode, Pattern};
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(crate) struct LetSpec<'a> {
    identifier: &'a EcoString,
    binding_id: Option<BindingId>,
    value: &'a Expression,
    binding_ty: &'a Type,
    mutable: bool,
}

fn widens_to_interface(planner: &Planner, value: &Expression, binding_ty: &Type) -> bool {
    planner.facts.is_interface_or_unknown(binding_ty) && *binding_ty != value.get_type()
}

pub(crate) fn needs_explicit_type_declaration(
    planner: &Planner,
    value: &Expression,
    binding_ty: &Type,
) -> bool {
    if widens_to_interface(planner, value, binding_ty) {
        return true;
    }
    if planner.is_function_alias(binding_ty) {
        let value_ty = value.get_type();
        if matches!(value_ty.unwrap_forall(), Type::Function(_)) {
            return true;
        }
    }
    false
}

impl Planner<'_> {
    pub(crate) fn declaration_needs_go_type(
        &mut self,
        value: &Expression,
        constant: Option<ConstantKind>,
        coercion: &CoercionPlan,
        ty: &Type,
    ) -> bool {
        (coercion.is_identity() && self.constant_needs_go_type(constant, ty).is_some())
            || needs_explicit_type_declaration(self, value, ty)
    }
}

fn resolve_let_temp_declaration_ty(
    planner: &Planner,
    value: &Expression,
    binding_ty: &Type,
) -> Type {
    if planner.facts.has_no_go_value(binding_ty) {
        return value.get_type();
    }
    binding_ty.clone()
}

impl Planner<'_> {
    fn choose_let_go_name(
        &mut self,
        identifier: &EcoString,
        ids: &[BindingId],
        raw_go_name: &GoIdentifier,
        force_fresh: bool,
    ) -> GoIdentifier {
        let escaped = escape_local(raw_go_name);
        if force_fresh
            || self.shadows_declaration(&escaped)
            || self.scope.has_other_binding_for_go_name(&escaped, ids)
        {
            self.fresh_source(&SourceName::new(identifier))
        } else {
            self.scope.new_local_identifier(raw_go_name)
        }
    }

    fn lower_let_value(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: Option<&GoIdentifier>,
        demand: Option<ComponentDemand>,
    ) -> Vec<Statement> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let ids = binding_id.as_slice();
        if self.is_unit_call(value) {
            return self.lower_let_unit_call(let_spec, raw_go_name);
        }
        let needs_temp = requires_temp_var(value);
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind_source(identifier, ids, "_");
            return if needs_temp {
                self.lower_let_temp(&GoIdentifier::external("_".to_string()), value, binding_ty)
            } else {
                self.lower_discard_value(value)
            };
        };
        let go_identifier = self.scope.new_local_identifier(raw_go_name);
        if !self.shadows_declaration(&go_identifier)
            && !self.scope.is_active_assign_target(&go_identifier)
            && !self.scope.has_binding_for_go_name(&go_identifier)
            && value.get_type().demoted() == binding_ty.demoted()
        {
            let local = go_identifier.clone();
            let into = self
                .lower_fused_result_match_into(value, &local)
                .or_else(|| self.lower_fused_option_match_into(value, &local))
                .or_else(|| self.lower_fused_partial_match_into(value, &local))
                .or_else(|| self.lower_defaulted_call_into(value, &local))
                .or_else(|| self.lower_slice_loop_into(value, &local));
            if let Some(statements) = into {
                self.scope.bind_identifier(identifier, ids, local);
                return statements;
            }
            if let Some(demand) = demand
                && let Some(statements) =
                    self.lower_let_as_components(let_spec, &go_identifier, demand)
            {
                return statements;
            }
            if !let_spec.mutable
                && let Some(statements) =
                    self.try_lower_let_into_wrapper_slot(let_spec, raw_go_name)
            {
                return statements;
            }
        }
        if needs_temp {
            if self.shadows_declaration(&go_identifier)
                || expression_contains_binding(value, identifier)
                || self.scope.expression_reads_go_name(value, &go_identifier)
            {
                let fresh = self.fresh_source(&SourceName::new(identifier));
                let statements = self.lower_let_temp(&fresh, value, binding_ty);
                self.scope.bind_identifier(identifier, ids, fresh);
                return statements;
            }
            let bound = self.scope.bind_source(identifier, ids, raw_go_name);
            return self.lower_let_temp(&bound, value, binding_ty);
        }
        self.lower_let_direct(let_spec, raw_go_name)
    }

    fn reassignable_shadow(&self, let_spec: LetSpec) -> Option<GoIdentifier> {
        let LetSpec {
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let peeled = self.facts.peel_alias(binding_ty);
        if peeled.is_unit()
            || peeled.is_result()
            || peeled.is_option()
            || peeled.is_partial()
            || peeled.tuple_arity().is_some()
            || matches!(value, Expression::Propagate { .. })
            || requires_temp_var(value)
            || self.facts.is_interface_or_unknown(binding_ty)
            || self.facts.resolve_to_function_type(binding_ty).is_some()
        {
            return None;
        }
        self.scope.reusable_binding(binding_id?)
    }

    fn lower_let_reassign(&mut self, let_spec: LetSpec, target: GoIdentifier) -> Vec<Statement> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let (mut statements, plan_value) = self
            .lower_value(value, ExpressionContext::value())
            .into_parts();
        let (coercion_setup, value_expression) = self
            .value_slot_coercion(value, binding_ty)
            .lower(self, plan_value);
        statements.extend(coercion_setup);
        if value_expression.as_identifier() != Some(&target) {
            statements.push(assign(
                GoExpression::identifier(target.clone()),
                value_expression,
            ));
        }
        self.scope
            .bind_identifier(identifier, binding_id.as_slice(), target);
        statements
    }

    fn lower_let_as_components(
        &mut self,
        let_spec: LetSpec,
        go_identifier: &GoIdentifier,
        demand: ComponentDemand,
    ) -> Option<Vec<Statement>> {
        let ty = self.facts.peel_alias(&let_spec.value.get_type());
        if matches!(ty, Type::Tuple(_)) {
            self.lower_tuple_components(let_spec, &ty, demand)
        } else {
            self.lower_fallible_components(let_spec, go_identifier, &ty, demand)
        }
    }

    fn lower_fallible_components(
        &mut self,
        let_spec: LetSpec,
        go_identifier: &GoIdentifier,
        ty: &Type,
        demand: ComponentDemand,
    ) -> Option<Vec<Statement>> {
        let value = let_spec.value;
        let kind = if ty.is_option() {
            ComponentKind::Option
        } else if !ty.is_result() {
            return None;
        } else if self.facts.is_prelude_error(&ty.err_type()) {
            ComponentKind::ErrorResult
        } else {
            ComponentKind::OtherResult
        };
        let source = self.component_producer(value, demand.value == ValueDemand::Whole, kind)?;
        let slot = if demand.value != ValueDemand::StatusOnly {
            self.declare(go_identifier);
            CommaOkValueSlot::Named(go_identifier.clone())
        } else {
            CommaOkValueSlot::Discarded
        };
        let (statements, components) =
            source.components(self, slot, ty.ok_type(), kind, demand.defaults > 1)?;
        self.scope.set_component_binding(
            let_spec.identifier,
            let_spec.binding_id.as_slice(),
            components,
        );
        Some(statements)
    }

    fn tuple_call_arity(&self, value: &Expression) -> Option<usize> {
        let plan = self.plan_call(value)?;
        self.unbridged_tuple_arity(&plan, &value.get_type())
    }

    fn lower_tuple_components(
        &mut self,
        let_spec: LetSpec,
        ty: &Type,
        demand: ComponentDemand,
    ) -> Option<Vec<Statement>> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            ..
        } = let_spec;
        let elements = self.facts.tuple_element_types(ty);
        if elements.len() < 2 {
            return None;
        }
        let read_indices = demand.read_indices;
        let needs_whole_value = demand.value == ValueDemand::Whole;
        if self.tuple_call_arity(value) != Some(elements.len()) {
            return None;
        }
        let is_read = |index: usize| needs_whole_value || read_indices.contains(&index);
        if !(0..elements.len()).any(is_read) {
            return None;
        }
        let names: Vec<GoIdentifier> = (0..elements.len())
            .map(|index| {
                if !is_read(index) {
                    return GoIdentifier::external("_".to_string());
                }
                self.fresh_declared_source(&SourceName::new(identifier).component(index))
            })
            .collect();
        let (setup, call) = self
            .lower_call(value, None, ExpressionContext::value())
            .into_parts();
        let mut statements = setup;
        statements.push(define_many(names.clone(), call));
        self.scope
            .set_tuple_binding(identifier, binding_id.as_slice(), TupleBinding { names });
        Some(statements)
    }

    fn lower_let_propagate(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: Option<&GoIdentifier>,
    ) -> Vec<Statement> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let ids = binding_id.as_slice();
        let Expression::Propagate {
            expression: inner, ..
        } = value
        else {
            unreachable!("lower_let_propagate requires a Propagate value");
        };
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind_source(identifier, ids, "_");
            return self.lower_propagate_statement(inner);
        };
        let go_identifier = self.choose_let_go_name(identifier, ids, raw_go_name, false);
        let local = go_identifier.clone();
        let mut statements = Vec::new();
        if widens_to_interface(self, value, binding_ty) {
            let var_ty = self.go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDecl {
                    name: local.clone(),
                    go_type: var_ty,
                    value: None,
                }
                .into(),
            );
            self.declare(&go_identifier);
        }
        statements.extend(self.lower_propagate_binding(inner, &local));
        self.scope.bind_identifier(identifier, ids, local);
        self.try_declare(&go_identifier);
        statements
    }

    fn lower_let_unit_call(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: Option<&GoIdentifier>,
    ) -> Vec<Statement> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let ids = binding_id.as_slice();
        let (mut statements, value_expression) = self
            .lower_value(value, ExpressionContext::value())
            .into_parts();
        statements.push(effect_statement(value_expression));
        let Some(raw_go_name) = raw_go_name else {
            return statements;
        };
        let escaped = escape_local(raw_go_name);
        let go_identifier = if self.shadows_declaration(&escaped) {
            let fresh = self.fresh_declared_source(&SourceName::new(identifier));
            self.scope.bind_source(identifier, ids, &fresh);
            fresh
        } else {
            let go_identifier = self.scope.bind_source(identifier, ids, raw_go_name);
            self.try_declare(&go_identifier);
            go_identifier
        };
        let unit = GoExpression::unit();
        if needs_explicit_type_declaration(self, value, binding_ty) {
            let go_type = self.go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDecl {
                    name: go_identifier,
                    go_type,
                    value: Some(unit),
                }
                .into(),
            );
        } else {
            statements.push(define(go_identifier, unit));
        }
        statements
    }

    fn lower_let_direct(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: &GoIdentifier,
    ) -> Vec<Statement> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let ids = binding_id.as_slice();
        let origin = self.function_type_origin(binding_ty, SlotOrigin::Lisette);
        let plan = self.lower_value(
            value,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let constant = plan.expression().constant_kind();
        let (mut statements, plan_value) = plan.into_parts();
        let coercion = self.value_slot_coercion(value, binding_ty);
        let coercion_is_identity = coercion.is_identity();
        let typed = self.declaration_needs_go_type(value, constant, &coercion, binding_ty);
        let (coercion_setup, value_expression) = coercion.lower(self, plan_value);
        statements.extend(coercion_setup);

        let go_identifier = self.claim_block_binding_as(
            identifier,
            ids,
            raw_go_name.clone(),
            &SourceName::new(identifier),
            OuterBindings::MayShadow,
        );

        if is_zero_call(&self.facts, value)
            && statements.is_empty()
            && coercion_is_identity
            && !needs_explicit_type_declaration(self, value, binding_ty)
        {
            let var_ty = self.go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDecl {
                    name: go_identifier,
                    go_type: var_ty,
                    value: None,
                }
                .into(),
            );
            return statements;
        }

        if typed {
            let var_ty = self.go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDecl {
                    name: go_identifier,
                    go_type: var_ty,
                    value: Some(value_expression),
                }
                .into(),
            );
            return statements;
        }
        if let Some(name) = value_expression.as_identifier()
            && !self.scope.has_binding_for_local(name)
            && (rebind_trailing_temp(&mut statements, &go_identifier, name)
                || rebind_updated_temp(&mut statements, &go_identifier, name))
        {
            return statements;
        }
        statements.push(define(go_identifier, value_expression));
        statements
    }

    fn try_lower_let_into_wrapper_slot(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        let go_identifier = self.scope.new_local_identifier(raw_go_name);
        let plan = self.plan_call(value)?;
        if !plan.wraps_result() {
            return None;
        }
        if matches!(
            plan.resolved.abi.result.lowered(),
            Some(LoweredReturnAbi::Tuple { .. })
        ) {
            return None;
        }
        if self.call_result_layout_bridge(&plan, binding_ty).is_some() {
            return None;
        }
        let local = go_identifier.clone();
        let target = WrapperTarget::Slot(&local);
        let statements =
            self.lower_abi_wrapped_call_to(value, &plan.resolved.abi, binding_ty, target)?;
        self.scope
            .bind_identifier(identifier, binding_id.as_slice(), local);
        Some(statements)
    }

    fn lower_let_temp(
        &mut self,
        identifier: &GoIdentifier,
        value: &Expression,
        binding_ty: &Type,
    ) -> Vec<Statement> {
        let name = identifier.spelling();
        if let Expression::TryBlock { items, ty, .. } = value
            && name != "_"
            && !self.is_declared(name)
        {
            return self.lower_try_block_into(items, ty, binding_ty, identifier);
        }
        let mut statements = Vec::new();
        if !self.is_declared(name) {
            if let Some(declaration) = self.let_temp_var_declaration(identifier, value, binding_ty)
            {
                statements.push(declaration.into());
            }
            self.try_declare(name);
        }
        statements.extend(self.lower_assign(
            value,
            &GoExpression::identifier(identifier.clone()),
            Some(binding_ty),
        ));
        collapse_declared_temp(
            &mut statements,
            identifier,
            self.short_declaration_keeps_type(binding_ty),
        );
        statements
    }

    fn let_temp_var_declaration(
        &mut self,
        name: &GoIdentifier,
        value: &Expression,
        binding_ty: &Type,
    ) -> Option<LoweredStatement> {
        if name.spelling() == "_" {
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
                self.go_type(binding_ty)
            } else if let Some(ctx_ty) = return_ctx.ty().cloned() {
                if self.fallible(&ctx_ty).is_some() {
                    self.go_type(&ctx_ty)
                } else {
                    self.go_type(&resolved_ty)
                }
            } else {
                self.go_type(&resolved_ty)
            }
        } else {
            self.go_type(&resolved_ty)
        };
        Some(LoweredStatement::VarDecl {
            name: name.clone(),
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
    demand: Option<ComponentDemand>,
}

impl<'a, 'e> LetPlanner<'a, 'e> {
    fn build(mut self) -> LoweredBlock {
        if self.value.get_type().is_never() {
            let mut statements = Vec::new();
            if let Pattern::Identifier {
                identifier,
                binding,
                ..
            } = &self.binding.pattern
                && let Some(raw_go_name) = self.planner.go_name_for_binding(&self.binding.pattern)
            {
                let go_identifier = self.planner.claim_block_binding(
                    identifier,
                    binding.as_slice(),
                    &raw_go_name,
                    OuterBindings::MayShadow,
                );
                let var_ty = self.planner.go_type(&self.binding.ty);
                statements.push(
                    LoweredStatement::VarDecl {
                        name: go_identifier,
                        go_type: var_ty,
                        value: None,
                    }
                    .into(),
                );
            }
            statements.push(self.planner.lower_statement(self.value));
            return LoweredBlock { statements };
        }

        let pattern = &self.binding.pattern;
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
            Pattern::Identifier {
                identifier,
                binding,
                ..
            } => {
                return self.lower_simple_identifier(identifier, *binding);
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
        if call.native_type != NativeTypeKind::Channel || call.method != "split" {
            return None;
        }
        let (source, _) = call.receiver_and_arguments()?;
        let (mut statements, receiver) = self
            .planner
            .lower_value(source, ExpressionContext::value())
            .into_parts();
        let receiver = if self.planner.facts.is_reference(&source.get_type()) {
            GoExpression::dereference(receiver)
        } else {
            receiver
        };
        let receiver = self.planner.stable_source(&mut statements, "ch", receiver);
        let planned: Vec<_> = elements
            .iter()
            .zip(slot_types)
            .filter_map(|(pattern, slot_ty)| {
                let Pattern::Identifier {
                    identifier,
                    binding,
                    ..
                } = pattern
                else {
                    return None;
                };
                let raw_go_name = GoIdentifier::from(self.planner.go_name_for_binding(pattern)?);
                let go_name = self.planner.choose_let_go_name(
                    identifier,
                    binding.as_slice(),
                    &raw_go_name,
                    self.planner.scope.has_binding_for_go_name(&raw_go_name),
                );
                self.planner.declare(&go_name);
                Some((
                    identifier,
                    *binding,
                    go_name,
                    self.planner.go_type(&slot_ty),
                ))
            })
            .collect();
        for (identifier, id, go_name, go_type) in planned {
            let local = self
                .planner
                .scope
                .bind_source(identifier, id.as_slice(), &go_name);
            statements.push(
                LoweredStatement::VarDecl {
                    name: local,
                    go_type,
                    value: Some(receiver.clone()),
                }
                .into(),
            );
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
        let slot_types = self.planner.facts.tuple_element_types(&self.binding.ty);
        let directional = [NativeTypeKind::Sender, NativeTypeKind::Receiver];
        (slot_types.len() == directional.len()
            && slot_types.iter().zip(directional).all(|(slot_ty, kind)| {
                NativeTypeKind::from_type(&self.planner.facts.peel_alias(slot_ty)) == Some(kind)
            }))
        .then_some(slot_types)
    }

    fn can_use_multi_value_optimization(&self) -> bool {
        let value_ty = self.value.get_type();
        if collect_pattern_info(self.planner, &self.binding.pattern, &value_ty)
            .bindings
            .iter()
            .any(|binding| !binding.coercion.is_identity())
        {
            return false;
        }
        let slots_read_in_place = self
            .planner
            .facts
            .tuple_element_types(&value_ty)
            .iter()
            .all(|slot_ty| !self.planner.facts.is_nullable_option(slot_ty));
        slots_read_in_place && self.planner.tuple_call_arity(self.value).is_some()
    }

    fn lower_simple_identifier(
        &mut self,
        identifier: &EcoString,
        binding_id: Option<BindingId>,
    ) -> LoweredBlock {
        let let_spec = LetSpec {
            identifier,
            binding_id,
            value: self.value,
            binding_ty: &self.binding.ty,
            mutable: self.binding.is_mutable(),
        };
        if !self.planner.facts.is_unused_binding(&self.binding.pattern)
            && let Some(target) = self.planner.reassignable_shadow(let_spec)
        {
            return LoweredBlock {
                statements: self.planner.lower_let_reassign(let_spec, target),
            };
        }
        let mut raw_go_name = self
            .planner
            .go_name_for_binding(&self.binding.pattern)
            .map(GoIdentifier::from);
        if let Some(raw) = &raw_go_name
            && self
                .planner
                .scope
                .has_other_binding_for_go_name(&escape_local(raw), binding_id.as_slice())
        {
            raw_go_name = Some(self.planner.fresh_source(&SourceName::new(identifier)));
        }
        let statements = if matches!(self.value, Expression::Propagate { .. }) {
            self.planner
                .lower_let_propagate(let_spec, raw_go_name.as_ref())
        } else {
            self.planner
                .lower_let_value(let_spec, raw_go_name.as_ref(), self.demand.take())
        };
        LoweredBlock { statements }
    }

    fn lower_discard(&mut self) -> LoweredBlock {
        LoweredBlock {
            statements: self.planner.lower_discard_value(self.value),
        }
    }

    fn lower_multi_value_call(&mut self, elements: &[Pattern]) -> LoweredBlock {
        let planned: Vec<Option<(&str, Option<BindingId>, GoIdentifier)>> = elements
            .iter()
            .map(|pattern| {
                let Pattern::Identifier {
                    identifier,
                    binding,
                    ..
                } = pattern
                else {
                    return None;
                };
                if identifier == "_" {
                    return None;
                }
                let go_name = self.planner.go_name_for_binding(pattern)?;
                let preferred = go_name.preferred();
                let name = if self.planner.shadows_declaration(&preferred)
                    || self
                        .planner
                        .scope
                        .has_other_binding_for_go_name(&preferred, binding.as_slice())
                {
                    self.planner.fresh_source(&go_name)
                } else {
                    self.planner.scope.new_local_identifier(preferred)
                };
                self.planner.declare(&name);
                Some((identifier.as_str(), *binding, name))
            })
            .collect();

        let (mut statements, call) = self
            .planner
            .lower_call(self.value, None, ExpressionContext::value())
            .into_parts();

        let go_vars: Vec<GoIdentifier> = planned
            .iter()
            .map(|binding| match binding {
                Some((identifier, id, go_name)) => {
                    self.planner
                        .scope
                        .bind_source(*identifier, id.as_slice(), go_name)
                }
                None => GoIdentifier::external("_".to_string()),
            })
            .collect();
        statements.push(if planned.iter().any(Option::is_some) {
            define_many(go_vars, call)
        } else {
            LoweredStatement::Discard {
                arity: planned.len(),
                value: call,
            }
            .into()
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
        demand: Option<ComponentDemand>,
    ) -> LoweredBlock {
        LetPlanner {
            planner: self,
            binding,
            value,
            mode,
            demand,
        }
        .build()
    }
}
