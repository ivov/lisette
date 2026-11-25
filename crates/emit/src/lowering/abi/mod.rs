use crate::lowering::calls::call_plan::is_prelude_dispatch;
use crate::lowering::definitions::interface_adapter::requirement_go_return_abi;
use crate::lowering::planner::queries::PlannerQueries;
use crate::target::names::go_name::MemberSpelling;
use crate::target::types::return_abi::{CallableReturnAbi, go_return_abi};
pub(in crate::lowering) mod callable;
pub(in crate::lowering) mod coercion_plan;
pub(in crate::lowering) mod fallible_producer;
pub(in crate::lowering) mod go_result_wrapping;
pub(in crate::lowering) mod go_returns;
pub(in crate::lowering) mod go_types;
pub(in crate::lowering) mod layout_change;
pub(in crate::lowering) mod slot_coercion;

use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::return_abi_rules::{
    function_return_abi, interface_method_return_abi, method_return_abi, slot_return_abi,
};
use crate::lookup::facts::value_layout::{FunctionLayout, ValueLayout};
use crate::lookup::state::enclosing::ReturnContext;
use crate::lowering::calls::call_plan::{
    CallableDeclaration, CallableOrigin, FunctionLayoutChange,
};
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::prelude::prelude_constructor;
use coercion_plan::CoercionPlan;
use syntax::ast::Expression;
use syntax::program::{
    CallOperation, CheckedCallee, Definition, GoHint, Method, NativeTypeKind, SlotOrigin,
    is_go_import,
};
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(in crate::lowering) enum ReturnSlot<'a> {
    Function,
    Method,
    InterfaceMethod { name: &'a str, method: &'a Method },
    FunctionValue(SlotOrigin),
    FunctionExpression(ExpressionContext<'a>),
    Go { hints: &'a [GoHint] },
}

pub(in crate::lowering) struct CalleeSignature<'s, 'a> {
    pub(in crate::lowering) callee: &'s Expression,
    pub(in crate::lowering) origin: &'s CallableOrigin<'a>,
    pub(in crate::lowering) id: Option<&'s str>,
    pub(in crate::lowering) declaration: Option<CallableDeclaration<'a>>,
    pub(in crate::lowering) declared_return: Option<&'s Type>,
    pub(in crate::lowering) return_type: &'s Type,
}

impl Planner<'_> {
    pub(in crate::lowering) fn return_context_for_type(&self, return_ty: Type) -> ReturnContext {
        let abi = return_abi(&self.facts, &return_ty, ReturnSlot::Function);
        Self::return_context_from_abi(return_ty, abi)
    }

    pub(in crate::lowering) fn return_context_for_method(&self, return_ty: Type) -> ReturnContext {
        let abi = return_abi(&self.facts, &return_ty, ReturnSlot::Method);
        Self::return_context_from_abi(return_ty, abi)
    }

    pub(in crate::lowering) fn return_context_from_abi(
        return_ty: Type,
        abi: CallableReturnAbi,
    ) -> ReturnContext {
        match abi {
            CallableReturnAbi::Void => ReturnContext::Void(return_ty),
            CallableReturnAbi::Lowered(shape) => ReturnContext::Lowered { return_ty, shape },
            CallableReturnAbi::Value => ReturnContext::Tagged(return_ty),
        }
    }
}

pub(in crate::lowering) fn is_closure_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Lambda { .. } | Expression::Function { .. }
    )
}

pub(in crate::lowering) fn go_callable_return_abi(
    facts: &EmitFacts<'_>,
    qualified_name: &str,
    declared_return: &Type,
    hints: &[GoHint],
) -> CallableReturnAbi {
    if facts.go_callable_return_slot(qualified_name).is_none() {
        return return_abi(facts, declared_return, ReturnSlot::Go { hints });
    }
    facts
        .go_abi_catalog()
        .callable_return_abi(qualified_name)
        .cloned()
        .map_or_else(
            || CallableReturnAbi::unlowered(declared_return),
            CallableReturnAbi::Lowered,
        )
}

pub(in crate::lowering) fn return_abi(
    facts: &EmitFacts<'_>,
    return_ty: &Type,
    slot: ReturnSlot<'_>,
) -> CallableReturnAbi {
    match slot {
        ReturnSlot::Function => function_return_abi(facts, return_ty),
        ReturnSlot::Method => method_return_abi(facts, return_ty),
        ReturnSlot::InterfaceMethod { name, method } => {
            interface_method_return_abi(facts, return_ty, name, method)
        }
        ReturnSlot::FunctionValue(origin) => slot_return_abi(facts, return_ty, origin),
        ReturnSlot::FunctionExpression(ctx) => {
            if ctx.forces_tagged_go_function()
                || ctx.result_fills_type_parameter()
                || ctx.function_slot_origin() == SlotOrigin::GoAny
            {
                CallableReturnAbi::Value
            } else {
                slot_return_abi(facts, return_ty, ctx.function_slot_origin())
            }
        }
        ReturnSlot::Go { hints } => go_return_abi(return_ty, hints, |id| facts.definition(id))
            .map_or_else(
                || CallableReturnAbi::unlowered(return_ty),
                CallableReturnAbi::Lowered,
            ),
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn callee_return_abi(
        &self,
        signature: CalleeSignature<'_, '_>,
    ) -> CallableReturnAbi {
        let abi = self.callee_return_abi_by_origin(&signature);
        debug_assert!(
            !abi.is_lowered() || self.facts.is_lowered_result_type(signature.return_type),
            "a lowered call result wraps into its tagged Lisette value"
        );
        abi
    }

    fn callee_return_abi_by_origin(
        &self,
        signature: &CalleeSignature<'_, '_>,
    ) -> CallableReturnAbi {
        if let Some(layout) = self.bound_dispatch_layout(signature.callee) {
            return layout.return_abi;
        }
        if let Some(abi) = self.go_callee_return_abi(signature) {
            return abi;
        }
        if matches!(
            signature.origin,
            CallableOrigin::Source(CallOperation::NativeConstructor {
                kind: NativeTypeKind::Array,
                ..
            })
        ) {
            return return_abi(self.facts, signature.return_type, ReturnSlot::Function);
        }
        self.lisette_callee_return_abi(signature)
            .unwrap_or_else(|| {
                CallableReturnAbi::unlowered(
                    signature.declared_return.unwrap_or(signature.return_type),
                )
            })
    }

    fn go_callee_return_abi(
        &self,
        signature: &CalleeSignature<'_, '_>,
    ) -> Option<CallableReturnAbi> {
        if self.bound_dispatch_requirement(signature.callee).is_some() {
            return None;
        }
        if let Some(requirement) = self.go_bound_requirement(signature.callee) {
            let declared_return = requirement.method.ty.get_function_ret()?;
            let abi = requirement_go_return_abi(self.facts, &requirement, declared_return);
            return abi.is_lowered().then_some(abi);
        }
        let id = signature.id.filter(|id| is_go_import(id))?;
        if signature
            .declaration
            .is_some_and(CallableDeclaration::is_type_definition)
        {
            return Some(CallableReturnAbi::Value);
        }
        let hints = self
            .facts
            .definition(id)
            .map(Definition::go_hints)
            .or_else(|| {
                let (owner, name) = id.rsplit_once('.')?;
                self.facts
                    .method(owner, name)
                    .map(|method| method.go_hints.as_slice())
            })
            .unwrap_or_default();

        Some(go_callable_return_abi(
            self.facts,
            id,
            signature.declared_return.unwrap_or(signature.return_type),
            hints,
        ))
    }

    fn lisette_callee_return_abi(
        &self,
        signature: &CalleeSignature<'_, '_>,
    ) -> Option<CallableReturnAbi> {
        let callee = signature.callee;
        let callee_ty = callee.get_type();
        let function = self
            .facts
            .resolve_to_function_type(callee_ty.unwrap_forall())
            .unwrap_or_else(|| callee_ty.unwrap_forall().clone());
        let Type::Function(function) = function else {
            return None;
        };
        if is_prelude_dispatch(signature.id, signature.origin)
            || prelude_constructor(callee).is_some()
        {
            return None;
        }
        if let Some(requirement) = self
            .facts
            .checked(callee)
            .function_value()
            .and_then(CheckedCallee::interface_requirement)
        {
            let method = &requirement.method;
            return Some(return_abi(
                self.facts,
                method.ty.get_function_ret().unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &method.source_name,
                    method,
                },
            ));
        }
        if self.member_spelling(callee) == MemberSpelling::TaggedCounterpart {
            return Some(CallableReturnAbi::Value);
        }
        let return_ty = signature
            .declared_return
            .unwrap_or(function.return_type.as_ref());
        let slot = match signature.declaration {
            Some(CallableDeclaration::Method(_))
                if signature
                    .id
                    .and_then(|id| id.rsplit_once('.'))
                    .is_none_or(|(owner, name)| self.facts.emits_go_method(owner, name)) =>
            {
                ReturnSlot::Method
            }
            Some(declaration) if !declaration.is_type_definition() => ReturnSlot::Function,
            _ => ReturnSlot::FunctionValue(
                self.facts
                    .function_type_origin(&callee_ty, SlotOrigin::Lisette),
            ),
        };

        Some(return_abi(self.facts, return_ty, slot))
    }

    pub(in crate::lowering) fn emitted_value_layout(
        &self,
        value: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValueLayout {
        let value = self.peel_go_slot_alias_cast(value, ctx);
        let ty = value.get_type();
        let slot_origin = ctx.function_slot_origin();
        if let Some(layout) = self.go_slot_declaration_layout(value, ctx) {
            return ValueLayout::Function {
                function_type: ty,
                layout,
            };
        }
        let origin =
            if self.is_go_callable(value) && self.facts.resolve_to_function_type(&ty).is_some() {
                slot_origin
            } else {
                SlotOrigin::Lisette
            };

        let layout = self.facts.value_layout(&ty, origin);
        let closure = is_closure_literal(value);
        if !closure && !ctx.result_fills_type_parameter() {
            return layout;
        }
        let ValueLayout::Function {
            function_type,
            layout,
        } = layout
        else {
            return layout;
        };

        let result_type = layout.result_layout.logical_type();
        let return_abi = return_abi(self.facts, result_type, ReturnSlot::FunctionExpression(ctx));
        ValueLayout::Function {
            function_type,
            layout: layout.with_return_abi(self.facts, return_abi, SlotOrigin::Lisette),
        }
    }

    pub(in crate::lowering) fn go_slot_declaration_layout(
        &self,
        value: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> Option<FunctionLayout> {
        if !ctx.function_slot_origin().declared_by_go()
            || ctx.is_callee()
            || ctx.forces_tagged_go_function()
            || ctx.result_fills_type_parameter()
            || is_closure_literal(value)
            || self.is_go_callable(value)
        {
            return None;
        }
        let callee = self.resolve_callable_value(value)?;
        if callee.is_prelude_dispatch || matches!(callee.origin, CallableOrigin::GoInterop) {
            return None;
        }
        Some(callee.function_layout(self.facts))
    }

    pub(in crate::lowering) fn peel_go_slot_alias_cast<'e>(
        &self,
        value: &'e Expression,
        ctx: ExpressionContext<'_>,
    ) -> &'e Expression {
        let Expression::Cast {
            expression: inner,
            ty,
            ..
        } = value
        else {
            return value;
        };

        if !ctx.function_slot_origin().declared_by_go() || self.facts.is_go_named_function_type(ty)
        {
            return value;
        }
        if self.facts.resolve_to_function_type(ty).is_none()
            || !self
                .function_type_slot_layout_change(&inner.get_type(), ty)
                .is_identity()
        {
            return value;
        }
        self.peel_go_slot_alias_cast(inner, ctx)
    }

    pub(in crate::lowering) fn callable_slot_layout_change(
        &self,
        source: FunctionLayout,
        slot_function: &Type,
        return_abi: Option<CallableReturnAbi>,
        origin: SlotOrigin,
    ) -> Option<FunctionLayoutChange> {
        let slot = self.facts.value_layout(slot_function, origin);
        let ValueLayout::Function { layout, .. } = slot.underlying() else {
            return None;
        };
        let target = match return_abi {
            Some(return_abi) if layout.return_abi != return_abi => {
                layout.with_return_abi(self.facts, return_abi, SlotOrigin::Lisette)
            }
            _ => layout.clone(),
        };

        (!source.same_representation(&target)).then_some(FunctionLayoutChange { source, target })
    }

    pub(in crate::lowering) fn function_slot_layout_change(
        &self,
        value: &Expression,
        target_ty: &Type,
    ) -> CoercionPlan {
        let Some(slot_origin) = self.go_function_slot_origin(&value.get_type(), target_ty) else {
            return CoercionPlan::Identity;
        };
        let source = self.emitted_value_layout(
            value,
            ExpressionContext::value().with_function_slot_origin(slot_origin),
        );
        let target = self.facts.value_layout(target_ty, slot_origin);
        CoercionPlan::change(self, &source, &target)
    }

    pub(in crate::lowering::abi) fn function_type_slot_layout_change(
        &self,
        value_ty: &Type,
        target_ty: &Type,
    ) -> CoercionPlan {
        let Some(slot_origin) = self.go_function_slot_origin(value_ty, target_ty) else {
            return CoercionPlan::Identity;
        };
        let source = self.facts.value_layout(value_ty, SlotOrigin::Lisette);
        let target = self.facts.value_layout(target_ty, slot_origin);
        CoercionPlan::change(self, &source, &target)
    }

    pub(in crate::lowering) fn any_slot_function_layouts(
        &self,
        ty: &Type,
    ) -> Option<(ValueLayout, ValueLayout)> {
        self.facts.resolve_to_function_type(ty)?;
        let stored = self.facts.value_layout(ty, SlotOrigin::GoAny);
        let lisette = self.facts.value_layout(ty, SlotOrigin::Lisette);
        (!stored.same_representation(&lisette)).then_some((stored, lisette))
    }

    fn go_function_slot_origin(&self, value_ty: &Type, target_ty: &Type) -> Option<SlotOrigin> {
        let slot_origin = self
            .facts
            .function_type_origin(target_ty, SlotOrigin::Lisette);
        ((slot_origin.declared_by_go()
            || self.facts.mentions_function_alias(value_ty)
            || self.facts.mentions_function_alias(target_ty))
            && self.facts.resolve_to_function_type(value_ty).is_some()
            && self.facts.resolve_to_function_type(target_ty).is_some())
        .then_some(slot_origin)
    }
}
