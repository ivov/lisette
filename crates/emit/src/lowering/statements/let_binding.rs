use crate::Planner;
use crate::lookup::facts::component_uses::ComponentDemand;
use crate::lookup::facts::component_uses::ValueDemand;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::queries::go_name_for_binding;
use crate::lookup::facts::value_layout::ValueLayout;
use crate::lookup::state::bindings::{ComponentKind, TupleBinding};
use crate::lowering::abi::coercion_plan::CoercionPlan;
use crate::lowering::abi::go_result_wrapping::WrapperTarget;
use crate::lowering::calls::call_plan::CallPlan;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::patterns::binding_declarations::pattern_has_bindings;
use crate::lowering::patterns::sites::PatternSubject;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::statements::placement::is_zero_call;
use crate::lowering::statements::placement::{
    collapse_declared_temp, expression_contains_binding, rebind_trailing_temp, rebind_updated_temp,
    requires_temp_var,
};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::{
    LoweredBlock, LoweredStatement, Statement, assign, declare, define, define_many,
    effect_statement,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::Fallible;
use crate::target::ir::values::{ConstantKind, GoExpression};
use crate::target::names::go_name::escape_local;
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::LoweredReturnAbi;
use syntax::EcoString;
use syntax::ast::{Binding, BindingId, Expression, LetMode, Pattern};
use syntax::program::NativeTypeKind;
use syntax::program::ReceiverCoercion;
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(in crate::lowering) struct LetSpec<'a> {
    identifier: &'a EcoString,
    binding_id: Option<BindingId>,
    value: &'a Expression,
    binding_ty: &'a Type,
    mutable: bool,
}

struct DirectLetValue {
    statements: Vec<Statement>,
    value: GoExpression,
    coercion_is_identity: bool,
    typed: bool,
}

impl<'a> Planner<'a> {
    fn choose_let_go_name(
        &mut self,
        identifier: &EcoString,
        ids: &[BindingId],
        raw_go_name: &GoIdentifier,
    ) -> GoIdentifier {
        let escaped = escape_local(raw_go_name);
        if self.queries().shadows_declaration(&escaped)
            || self.scope.go_name_bound_outside_ids(&escaped, ids)
        {
            self.fresh_local(&SourceName::new(identifier))
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
        if self.queries().is_unit_call(value) {
            return self.lower_let_unit_call(let_spec, raw_go_name);
        }
        let needs_temp = requires_temp_var(value);
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind_new_local(identifier, ids, "_");
            return if needs_temp {
                self.lower_let_temp(&GoIdentifier::blank(), value, binding_ty)
            } else {
                self.lower_discard_value(value)
            };
        };

        let go_identifier = self.scope.new_local_identifier(raw_go_name);
        if let Some(statements) =
            self.try_lower_let_into_name(let_spec, raw_go_name, &go_identifier, demand)
        {
            return statements;
        }
        if needs_temp {
            if self.queries().shadows_declaration(&go_identifier)
                || expression_contains_binding(value, identifier)
                || self.scope.expression_reads_go_name(value, &go_identifier)
            {
                let fresh = self.fresh_local(&SourceName::new(identifier));
                let statements = self.lower_let_temp(&fresh, value, binding_ty);
                self.scope.bind_existing_identifier(identifier, ids, fresh);
                return statements;
            }
            let bound = self.scope.bind_new_local(identifier, ids, raw_go_name);
            return self.lower_let_temp(&bound, value, binding_ty);
        }
        self.lower_let_direct(let_spec, raw_go_name)
    }

    fn try_lower_let_into_name(
        &mut self,
        let_spec: LetSpec,
        raw_go_name: &GoIdentifier,
        go_identifier: &GoIdentifier,
        demand: Option<ComponentDemand>,
    ) -> Option<Vec<Statement>> {
        let LetSpec {
            identifier,
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;
        if self.queries().shadows_declaration(go_identifier)
            || self.scope.is_active_assign_target(go_identifier)
            || self.scope.has_binding_for_go_name(go_identifier)
            || !self
                .facts
                .same_type(&value.get_type().demoted(), &binding_ty.demoted())
        {
            return None;
        }
        let local = go_identifier.clone();
        let into = self
            .try_lower_fused_result_match_into(value, &local)
            .or_else(|| self.try_lower_fused_option_match_into(value, &local))
            .or_else(|| self.try_lower_fused_partial_match_into(value, &local))
            .or_else(|| self.try_lower_defaulted_call_into(value, &local))
            .or_else(|| self.try_lower_slice_loop_into(value, &local));

        if let Some(statements) = into {
            self.scope
                .bind_existing_identifier(identifier, binding_id.as_slice(), local);
            return Some(statements);
        }
        if let Some(demand) = demand
            && let Some(statements) =
                self.try_lower_let_as_components(let_spec, go_identifier, demand)
        {
            return Some(statements);
        }
        if !let_spec.mutable {
            return self.try_lower_let_into_wrapper_slot(let_spec, raw_go_name);
        }
        None
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
            .plan_value(value, ExpressionContext::value())
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
            .bind_existing_identifier(identifier, binding_id.as_slice(), target);
        statements
    }

    fn try_lower_let_as_components(
        &mut self,
        let_spec: LetSpec,
        go_identifier: &GoIdentifier,
        demand: ComponentDemand,
    ) -> Option<Vec<Statement>> {
        let ty = let_spec.value.get_type();
        if self.facts.is_tuple(&ty) {
            self.try_lower_tuple_components(let_spec, &ty, demand)
        } else {
            self.try_lower_fallible_components(let_spec, go_identifier, &ty, demand)
        }
    }

    fn try_lower_fallible_components(
        &mut self,
        let_spec: LetSpec,
        go_identifier: &GoIdentifier,
        ty: &Type,
        demand: ComponentDemand,
    ) -> Option<Vec<Statement>> {
        let value = let_spec.value;
        let fallible = self.facts.fallible(ty)?;
        let kind = match &fallible {
            Fallible::Option { .. } => ComponentKind::Option,
            Fallible::Result { err_ty, .. } if self.facts.is_prelude_error(err_ty) => {
                ComponentKind::ErrorResult
            }
            Fallible::Result { .. } => ComponentKind::OtherResult,
        };

        let source = self.component_producer(value, demand.value, kind)?;
        let slot = if demand.value != ValueDemand::StatusOnly {
            self.scope.declare_go_name(go_identifier);
            CommaOkValueSlot::Named(go_identifier.clone())
        } else {
            CommaOkValueSlot::Discarded
        };

        let (statements, components) = source.components(
            self,
            slot,
            fallible.ok_ty().clone(),
            kind,
            demand.defaults > 1,
        )?;

        self.scope.bind_components_filling_ids(
            let_spec.identifier,
            let_spec.binding_id.as_slice(),
            components,
        );
        Some(statements)
    }

    fn try_lower_tuple_components(
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

        let elements = self.facts.tuple_or_type_arguments(ty);
        if elements.len() < 2 {
            return None;
        }
        let read_indices = demand.read_indices;
        let needs_whole_value = demand.value == ValueDemand::Whole;
        let (arity, plan) = self.queries().tuple_call_plan(value)?;
        if arity != elements.len() {
            return None;
        }
        let is_read = |index: usize| needs_whole_value || read_indices.contains(&index);
        if !(0..elements.len()).any(is_read) {
            return None;
        }
        let names: Vec<GoIdentifier> = (0..elements.len())
            .map(|index| {
                if !is_read(index) {
                    return GoIdentifier::blank();
                }
                self.fresh_declared(&SourceName::new(identifier).component(index))
            })
            .collect();

        let (setup, call) = self
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();
        let mut statements = setup;
        statements.push(define_many(names.clone(), call));
        self.scope.bind_tuple_filling_ids(
            identifier,
            binding_id.as_slice(),
            TupleBinding { names },
        );
        Some(statements)
    }

    fn lower_let_propagate(
        &mut self,
        let_spec: LetSpec,
        inner: &Expression,
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

        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind_new_local(identifier, ids, "_");
            return self.lower_propagate_statement(inner);
        };
        let go_identifier = self.choose_let_go_name(identifier, ids, raw_go_name);
        let local = go_identifier.clone();
        let mut statements = Vec::new();
        if widens_to_interface(&self.facts, value, binding_ty) {
            let var_ty = self.queries().go_type(binding_ty);
            statements.push(declare(local.clone(), var_ty));
            self.scope.declare_go_name(&go_identifier);
        }
        statements.extend(self.lower_propagate_binding(inner, &local));
        self.scope.bind_existing_identifier(identifier, ids, local);
        self.scope.try_declare_go_name(&go_identifier);
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
            .plan_value(value, ExpressionContext::value())
            .into_parts();
        statements.push(effect_statement(value_expression));
        let unit = self.unit_value(Some(value), Some(binding_ty));
        let Some(raw_go_name) = raw_go_name else {
            self.scope.bind_new_local(identifier, ids, "_");
            return statements;
        };
        let escaped = escape_local(raw_go_name);
        let go_identifier = if self.queries().shadows_declaration(&escaped) {
            let fresh = self.fresh_declared(&SourceName::new(identifier));
            self.scope.bind_new_local(identifier, ids, &fresh);
            fresh
        } else {
            let go_identifier = self.scope.bind_new_local(identifier, ids, raw_go_name);
            self.scope.try_declare_go_name(&go_identifier);
            go_identifier
        };

        if needs_explicit_type_declaration(&self.facts, value, binding_ty) {
            let go_type = self.queries().go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDeclaration {
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
        let DirectLetValue {
            mut statements,
            value: value_expression,
            coercion_is_identity,
            typed,
        } = self.plan_direct_let_value(value, binding_ty);

        let go_identifier = self.claim_binding(identifier, ids, raw_go_name.clone());

        if is_zero_call(&self.facts, value)
            && statements.is_empty()
            && coercion_is_identity
            && !needs_explicit_type_declaration(&self.facts, value, binding_ty)
        {
            let var_ty = self.queries().go_type(binding_ty);
            statements.push(declare(go_identifier, var_ty));
            return statements;
        }

        if typed {
            let var_ty = self.queries().go_type(binding_ty);
            statements.push(
                LoweredStatement::VarDeclaration {
                    name: go_identifier,
                    go_type: var_ty,
                    value: Some(value_expression),
                }
                .into(),
            );
            return statements;
        }
        if let Some(name) = value_expression.as_identifier()
            && let Some(id) = name.id()
            && self.scope.lacks_binding_for_local(id)
            && (rebind_trailing_temp(&mut statements, &go_identifier, name)
                || rebind_updated_temp(&mut statements, &go_identifier, name))
        {
            return statements;
        }
        statements.push(define(go_identifier, value_expression));
        statements
    }

    fn plan_direct_let_value(&mut self, value: &Expression, binding_ty: &Type) -> DirectLetValue {
        let origin = self
            .facts
            .function_type_origin(binding_ty, SlotOrigin::Lisette);
        let plan = self.plan_value(
            value,
            ExpressionContext::value().with_function_slot_origin(origin),
        );
        let constant = plan.expression().and_then(GoExpression::constant_kind);
        let (mut statements, plan_value) = plan.into_parts();
        let coercion = self.value_slot_coercion(value, binding_ty);
        let coercion_is_identity = coercion.is_identity();
        let typed = self
            .queries()
            .declaration_needs_go_type(value, constant, &coercion, binding_ty);
        let (coercion_setup, value_expression) = coercion.lower(self, plan_value);
        statements.extend(coercion_setup);
        DirectLetValue {
            statements,
            value: value_expression,
            coercion_is_identity,
            typed,
        }
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
        let plan = self.queries().plan_call(CallSite::of(value)?);
        if !plan.wraps_result() {
            return None;
        }
        if matches!(
            plan.resolved.abi.result.lowered(),
            Some(LoweredReturnAbi::Tuple { .. })
        ) {
            return None;
        }
        if self
            .queries()
            .call_result_layout_change(&plan, binding_ty)
            .is_some()
        {
            return None;
        }
        let local = go_identifier;
        let target = WrapperTarget::Slot(&local);
        let statements = self.try_lower_abi_wrapped_call_to(&plan, binding_ty, target)?;

        self.scope
            .bind_existing_identifier(identifier, binding_id.as_slice(), local);
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
            && !self.scope.is_go_name_declared(name)
        {
            return self.lower_try_block_into(items, ty, binding_ty, identifier);
        }
        let mut statements = Vec::new();
        if !self.scope.is_go_name_declared(name) {
            if let Some(declaration) = self
                .queries()
                .let_temp_var_declaration(identifier, value, binding_ty)
            {
                statements.push(declaration.into());
            }
            self.scope.try_declare_go_name(name);
        }
        statements.extend(self.lower_assign(
            value,
            &GoExpression::identifier(identifier.clone()),
            binding_ty,
        ));
        collapse_declared_temp(
            &mut statements,
            identifier,
            self.facts.short_declaration_keeps_type(binding_ty),
        );
        statements
    }

    pub(in crate::lowering) fn build_let_plan(
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

struct LetPlanner<'a, 'e> {
    planner: &'a mut Planner<'e>,
    binding: &'a Binding,
    value: &'a Expression,
    mode: &'a LetMode,
    demand: Option<ComponentDemand>,
}

impl<'a, 'e> LetPlanner<'a, 'e> {
    #[allow(clippy::wildcard_enum_match_arm)]
    fn build(mut self) -> LoweredBlock {
        if self.planner.facts.is_never(&self.value.get_type()) {
            return LoweredBlock {
                statements: self.planner.lower_never_value(self.value),
            };
        }

        if let Some(block) = self.try_lower_let_mode() {
            return block;
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
            _ if let Some(block) = self.try_lower_converted_destructuring() => return block,
            Pattern::Tuple { elements, .. } => {
                if let Some(block) = self.try_lower_tuple_let(elements) {
                    return block;
                }
            }
            _ => {}
        }
        let value_ty = self.value.get_type();
        LoweredBlock {
            statements: self.planner.lower_irrefutable_pattern_site(
                PatternSubject::expression(self.value, "tmp"),
                &self.binding.pattern,
                &value_ty,
            ),
        }
    }

    fn try_lower_let_mode(&mut self) -> Option<LoweredBlock> {
        let pattern = &self.binding.pattern;
        match self.mode {
            LetMode::Plain => None,
            LetMode::Assert | LetMode::InvalidAssertElse { .. } => Some(LoweredBlock {
                statements: self.planner.lower_let_assert_pattern_site(
                    pattern,
                    self.value,
                    self.binding.pattern.get_span(),
                ),
            }),
            LetMode::Else { block, .. }
                if self.planner.facts.resolves_to_unit(&self.value.get_type()) =>
            {
                self.planner
                    .conversions
                    .record_dropped_code(block.get_span());
                (!pattern_has_bindings(pattern)).then(|| self.lower_discard())
            }
            LetMode::Else { block, .. } => Some(LoweredBlock {
                statements: self
                    .planner
                    .lower_let_else_pattern_site(pattern, self.value, block),
            }),
        }
    }

    fn try_lower_tuple_let(&mut self, elements: &[Pattern]) -> Option<LoweredBlock> {
        let all_unused = elements
            .iter()
            .all(|element| matches!(element, Pattern::WildCard { .. }));
        if all_unused {
            return Some(self.lower_discard());
        }
        if let Some(block) = self.try_lower_channel_split(elements) {
            return Some(block);
        }
        if elements.iter().all(|element| {
            matches!(
                element,
                Pattern::Identifier { .. } | Pattern::WildCard { .. }
            )
        }) && let Some(plan) = self.multi_value_call_plan()
        {
            return Some(self.lower_multi_value_call(elements, plan));
        }
        None
    }

    fn try_lower_converted_destructuring(&mut self) -> Option<LoweredBlock> {
        let coercion = self
            .planner
            .value_slot_coercion(self.value, &self.binding.ty);
        if coercion.is_identity() {
            return None;
        }
        let (mut statements, value) = self
            .planner
            .plan_value(self.value, ExpressionContext::value())
            .into_parts();
        let (coercion_setup, converted) = coercion.lower(self.planner, value);
        statements.extend(coercion_setup);
        let subject = self
            .planner
            .stable_source(&mut statements, "subject", converted);
        statements.extend(self.planner.lower_irrefutable_pattern_site(
            PatternSubject::Existing { var: subject },
            &self.binding.pattern,
            &self.binding.ty,
        ));
        Some(LoweredBlock { statements })
    }

    fn try_lower_channel_split(&mut self, elements: &[Pattern]) -> Option<LoweredBlock> {
        let slot_types = self.channel_split_slot_types(elements)?;
        let call = self.planner.queries().native_method_call(self.value)?;
        if call.native_type != NativeTypeKind::Channel || call.method != "split" {
            return None;
        }
        let (source, _) = call.receiver_and_arguments()?;
        let (mut statements, receiver) = self
            .planner
            .plan_value(source, ExpressionContext::value())
            .into_parts();
        let receiver = if self.planner.facts.checked(source).receiver_change()
            == Some(ReceiverCoercion::AutoDeref)
        {
            GoExpression::dereference(receiver)
        } else {
            receiver
        };

        let receiver = self.planner.stable_source(&mut statements, "ch", receiver);
        let planned: Vec<_> = elements
            .iter()
            .zip(slot_types)
            .filter_map(|(pattern, slot_ty)| self.plan_channel_split_name(pattern, &slot_ty))
            .collect();

        for (identifier, id, go_name, go_type) in planned {
            let local = self
                .planner
                .scope
                .bind_new_local(identifier, id.as_slice(), &go_name);
            statements.push(
                LoweredStatement::VarDeclaration {
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
        let slot_types = self.planner.facts.tuple_or_type_arguments(&self.binding.ty);
        let directional = [NativeTypeKind::Sender, NativeTypeKind::Receiver];
        (slot_types.len() == directional.len()
            && slot_types
                .iter()
                .zip(directional)
                .all(|(slot_ty, kind)| self.planner.facts.native_kind(slot_ty) == Some(kind)))
        .then_some(slot_types)
    }

    fn multi_value_call_plan(&self) -> Option<CallPlan<'e, 'a>> {
        let value_ty = self.value.get_type();
        if self
            .planner
            .collect_pattern_info(&self.binding.pattern, &value_ty)
            .bindings
            .iter()
            .any(|binding| binding.coercion.changes_layout())
        {
            return None;
        }
        let slots_read_in_place = self
            .planner
            .facts
            .lowered_tuple_slot_layouts(&value_ty)
            .iter()
            .all(|layout| !matches!(layout, ValueLayout::NullableOption { .. }));
        if !slots_read_in_place {
            return None;
        }
        self.planner
            .queries()
            .tuple_call_plan(self.value)
            .map(|(_, plan)| plan)
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

        if let Some(target) = self.planner.queries().reassignable_shadow(let_spec) {
            return LoweredBlock {
                statements: self.planner.lower_let_reassign(let_spec, target),
            };
        }
        let mut raw_go_name = go_name_for_binding(&self.binding.pattern).map(GoIdentifier::from);
        if let Some(raw) = &raw_go_name
            && self
                .planner
                .scope
                .go_name_bound_outside_ids(&escape_local(raw), binding_id.as_slice())
        {
            raw_go_name = Some(self.planner.fresh_local(&SourceName::new(identifier)));
        }
        let statements = if let Expression::Propagate {
            expression: inner, ..
        } = self.value
            && self
                .planner
                .value_slot_coercion(self.value, &self.binding.ty)
                .is_identity()
        {
            self.planner
                .lower_let_propagate(let_spec, inner, raw_go_name.as_ref())
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

    fn lower_multi_value_call(
        &mut self,
        elements: &[Pattern],
        plan: CallPlan<'e, 'a>,
    ) -> LoweredBlock {
        let planned: Vec<Option<(&str, Option<BindingId>, GoIdentifier)>> = elements
            .iter()
            .map(|pattern| self.plan_multi_value_name(pattern))
            .collect();

        let (mut statements, call) = self
            .planner
            .plan_call_with_plan(plan, None, ExpressionContext::value())
            .into_parts();

        let go_vars: Vec<GoIdentifier> = planned
            .iter()
            .map(|binding| match binding {
                Some((identifier, id, go_name)) => {
                    self.planner
                        .scope
                        .bind_new_local(*identifier, id.as_slice(), go_name)
                }
                None => GoIdentifier::blank(),
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

    fn plan_multi_value_name<'p>(
        &mut self,
        pattern: &'p Pattern,
    ) -> Option<(&'p str, Option<BindingId>, GoIdentifier)> {
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
        let go_name = go_name_for_binding(pattern)?;
        let preferred = go_name.preferred();
        let name = if self.planner.queries().shadows_declaration(&preferred)
            || self
                .planner
                .scope
                .go_name_bound_outside_ids(&preferred, binding.as_slice())
        {
            self.planner.fresh_local(&go_name)
        } else {
            self.planner.scope.new_local_identifier(preferred)
        };

        self.planner.scope.declare_go_name(&name);
        Some((identifier.as_str(), *binding, name))
    }

    fn plan_channel_split_name<'p>(
        &mut self,
        pattern: &'p Pattern,
        slot_ty: &Type,
    ) -> Option<(&'p EcoString, Option<BindingId>, GoIdentifier, GoType)> {
        let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = pattern
        else {
            return None;
        };

        let raw_go_name = GoIdentifier::from(go_name_for_binding(pattern)?);
        let go_name = if self.planner.scope.has_binding_for_go_name(&raw_go_name) {
            self.planner.fresh_local(&SourceName::new(identifier))
        } else {
            self.planner
                .choose_let_go_name(identifier, binding.as_slice(), &raw_go_name)
        };

        self.planner.scope.declare_go_name(&go_name);
        Some((
            identifier,
            *binding,
            go_name,
            self.planner.queries().go_type(slot_ty),
        ))
    }
}

fn widens_to_interface(facts: &EmitFacts<'_>, value: &Expression, binding_ty: &Type) -> bool {
    facts.is_interface_or_unknown(binding_ty) && !facts.same_type(binding_ty, &value.get_type())
}

fn resolve_let_temp_declaration_ty(
    facts: &EmitFacts<'_>,
    value: &Expression,
    binding_ty: &Type,
) -> Type {
    if facts.has_no_go_value(binding_ty) {
        return value.get_type();
    }
    binding_ty.clone()
}

fn needs_explicit_type_declaration(
    facts: &EmitFacts<'_>,
    value: &Expression,
    binding_ty: &Type,
) -> bool {
    if widens_to_interface(facts, value, binding_ty) {
        return true;
    }
    if facts.is_go_named_function_type(binding_ty) {
        let value_ty = value.get_type();
        if matches!(value_ty.unwrap_forall(), Type::Function(_)) {
            return true;
        }
    }
    false
}

impl<'a> PlannerQueries<'_, 'a> {
    pub(in crate::lowering) fn declaration_needs_go_type(
        &self,
        value: &Expression,
        constant: Option<ConstantKind>,
        coercion: &CoercionPlan,
        ty: &Type,
    ) -> bool {
        (coercion.is_identity() && self.constant_needs_go_type(constant, ty).is_some())
            || needs_explicit_type_declaration(self.facts, value, ty)
    }

    fn reassignable_shadow(&self, let_spec: LetSpec) -> Option<GoIdentifier> {
        let LetSpec {
            binding_id,
            value,
            binding_ty,
            ..
        } = let_spec;

        if self.facts.resolves_to_unit(binding_ty)
            || self.facts.is_option_or_result(binding_ty)
            || self.facts.is_partial(binding_ty)
            || self.facts.is_tuple(binding_ty)
            || matches!(value, Expression::Propagate { .. })
            || requires_temp_var(value)
            || self.facts.is_interface_or_unknown(binding_ty)
            || self.facts.resolve_to_function_type(binding_ty).is_some()
        {
            return None;
        }
        self.scope.reusable_binding(binding_id?)
    }

    fn tuple_call_plan<'v>(&self, value: &'v Expression) -> Option<(usize, CallPlan<'a, 'v>)> {
        let plan = self.plan_call(CallSite::of(value)?);
        let arity = self.kept_layout_tuple_arity(&plan, &value.get_type())?;
        Some((arity, plan))
    }

    fn let_temp_var_declaration(
        &self,
        name: &GoIdentifier,
        value: &Expression,
        binding_ty: &Type,
    ) -> Option<LoweredStatement> {
        if name.spelling() == "_" {
            return None;
        }
        let resolved_ty = resolve_let_temp_declaration_ty(self.facts, value, binding_ty);
        let var_ty = self.go_type(&resolved_ty);
        Some(LoweredStatement::VarDeclaration {
            name: name.clone(),
            go_type: var_ty,
            value: None,
        })
    }
}
