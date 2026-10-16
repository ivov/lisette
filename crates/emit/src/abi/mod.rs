pub(crate) mod callable;
pub(crate) mod catalog;
pub(crate) mod coercion;
pub(crate) mod layout;
pub(crate) mod transition;

use crate::Planner;
use crate::names::go_name;
use crate::names::go_name::PRELUDE_ERROR_ID;
use crate::types::go_type::GoType;
use callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use coercion::CoercionPlan;
use layout::{SlotOrigin, ValueLayout};
use syntax::ast::Expression;
use syntax::types::Type;

/// Go spells a tuple payload as one result per element.
pub(crate) fn go_payload_layout(return_ty: &Type) -> PayloadLayout {
    if return_ty
        .ok_type()
        .tuple_arity()
        .is_some_and(|arity| arity >= 2)
    {
        PayloadLayout::Flattened
    } else {
        PayloadLayout::Packed
    }
}

impl Planner<'_> {
    /// Natural physical ABI of a Lisette-authored callable return.
    pub(crate) fn callable_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        self.classify_direct_emission(return_ty)
            .map_or(CallableReturnAbi::Value, CallableReturnAbi::Lowered)
    }

    pub(crate) fn slot_return_abi(
        &self,
        return_ty: &Type,
        origin: SlotOrigin,
    ) -> CallableReturnAbi {
        self.classify_slot_emission(return_ty, origin)
            .map_or(CallableReturnAbi::Value, CallableReturnAbi::Lowered)
    }

    /// A Go-named function type renders as its Go name, so a value of that
    /// type has Go's shape in any slot.
    pub(crate) fn function_type_origin(&self, fn_ty: &Type, origin: SlotOrigin) -> SlotOrigin {
        if origin.declared_by_go() || !self.is_go_named_function_type(fn_ty) {
            origin
        } else {
            SlotOrigin::GoParameter
        }
    }

    fn is_go_named_function_type(&self, ty: &Type) -> bool {
        matches!(ty.unwrap_forall(), Type::Nominal { id, .. } if go_name::is_go_import(id))
            && self.facts.resolve_to_function_type(ty).is_some()
    }

    /// A function value's layout as emitted into a slot at `slot_origin`.
    /// A closure literal returns in the slot's ABI but keeps Lisette value slots.
    pub(crate) fn emitted_value_layout(
        &self,
        value: &Expression,
        slot_origin: SlotOrigin,
    ) -> ValueLayout {
        let ty = value.get_type();
        if self.is_go_callable(value) {
            return self.value_layout(&ty, slot_origin);
        }
        let layout = self.value_layout(&ty, SlotOrigin::Lisette);
        if !is_closure_literal(value) {
            return layout;
        }
        let ValueLayout::Function {
            function_type,
            layout,
        } = layout
        else {
            return layout;
        };
        let result = self.value_layout(&layout.result_type, SlotOrigin::Lisette);
        let return_abi = self.slot_return_abi(&layout.result_type, slot_origin);
        ValueLayout::Function {
            function_type,
            layout: self.function_layout(layout.parameters, &result, return_abi, None),
        }
    }

    /// Non-identity for e.g. a Lisette `fn` bound to a Go-named function type.
    pub(crate) fn function_slot_bridge(
        &self,
        value: &Expression,
        target_ty: &Type,
    ) -> CoercionPlan {
        let Some(slot_origin) = self.go_function_slot_origin(&value.get_type(), target_ty) else {
            return CoercionPlan::Identity;
        };
        let source = self.emitted_value_layout(value, slot_origin);
        let target = self.value_layout(target_ty, slot_origin);
        CoercionPlan::bridge(self, &source, &target)
    }

    pub(crate) fn function_type_slot_bridge(
        &self,
        value_ty: &Type,
        target_ty: &Type,
    ) -> CoercionPlan {
        let Some(slot_origin) = self.go_function_slot_origin(value_ty, target_ty) else {
            return CoercionPlan::Identity;
        };
        let source = self.value_layout(value_ty, SlotOrigin::Lisette);
        let target = self.value_layout(target_ty, slot_origin);
        CoercionPlan::bridge(self, &source, &target)
    }

    fn go_function_slot_origin(&self, value_ty: &Type, target_ty: &Type) -> Option<SlotOrigin> {
        let slot_origin = self.function_type_origin(target_ty, SlotOrigin::Lisette);
        (slot_origin.declared_by_go()
            && self.facts.resolve_to_function_type(value_ty).is_some()
            && self.facts.resolve_to_function_type(target_ty).is_some())
        .then_some(slot_origin)
    }

    pub(crate) fn value_slot_coercion(&self, value: &Expression, target_ty: &Type) -> CoercionPlan {
        let bridge = self.function_slot_bridge(value, target_ty);
        if bridge.is_identity() {
            CoercionPlan::internal(self, &value.get_type(), target_ty)
        } else {
            bridge
        }
    }

    /// Lowered shape for the slot at `origin`, or `None` to keep it tagged.
    pub(crate) fn classify_slot_emission(
        &self,
        return_ty: &Type,
        origin: SlotOrigin,
    ) -> Option<LoweredReturnAbi> {
        let abi = self.classify_direct_emission(return_ty)?;
        Some(if origin.declared_by_go() && abi.payload().is_some() {
            abi.with_payload(go_payload_layout(&self.facts.peel_alias(return_ty)))
        } else {
            abi
        })
    }

    /// Lowered shape for a Lisette return type, or `None` to keep it tagged.
    pub(crate) fn classify_direct_emission(&self, return_ty: &Type) -> Option<LoweredReturnAbi> {
        let peeled = self.facts.peel_alias(return_ty);
        if peeled.is_result() && self.err_slot_is_nilable(&peeled) {
            return Some(if peeled.ok_type().is_unit() {
                LoweredReturnAbi::BareError
            } else {
                LoweredReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                }
            });
        }
        if peeled.is_partial() && self.err_slot_is_nilable(&peeled) {
            return Some(LoweredReturnAbi::Partial {
                payload: PayloadLayout::Packed,
            });
        }
        if peeled.is_option() {
            let encoding = if self.facts.is_nullable_option(&peeled) {
                OptionReturnAbi::Nullable
            } else {
                OptionReturnAbi::CommaOk {
                    payload: PayloadLayout::Packed,
                }
            };
            return Some(LoweredReturnAbi::Option(encoding));
        }
        if let Some(arity) = peeled.tuple_arity()
            && arity >= 2
        {
            return Some(LoweredReturnAbi::Tuple { arity });
        }
        None
    }

    /// True when the err slot of a `Result`/`Partial` lowers to a Go
    /// nilable type, so `nil` typechecks as the no-error sentinel.
    fn err_slot_is_nilable(&self, fallible_ty: &Type) -> bool {
        let err = self.facts.peel_alias(&fallible_ty.err_type());
        matches!(&err, Type::Nominal { id, .. } if id.as_str() == PRELUDE_ERROR_ID)
            || self.facts.is_nilable_go_type(&err)
    }

    pub(crate) fn render_callable_return_ty(
        &mut self,
        abi: &CallableReturnAbi,
        return_ty: &Type,
    ) -> String {
        let go_type = self.callable_return_go_type(abi, return_ty);
        self.use_rendered_go_type(go_type)
    }

    /// The Go result of a Lisette-authored callable with this return ABI.
    pub(crate) fn callable_return_go_type(
        &self,
        abi: &CallableReturnAbi,
        return_ty: &Type,
    ) -> GoType {
        let peeled = self.facts.peel_alias(return_ty);
        let result = self.value_layout(&peeled, SlotOrigin::Lisette);
        self.function_layout(Vec::new(), &result, abi.clone(), None)
            .result_go_type(self)
            .unwrap_or_else(|| self.go_type(&peeled))
    }
}

pub(crate) fn go_result_list(slots: &[GoType]) -> GoType {
    let code = format!(
        "({})",
        slots
            .iter()
            .map(|slot| slot.code.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    GoType::with_dependencies(code, slots)
}

pub(crate) fn tuple_element_types(ty: &Type) -> Vec<Type> {
    match ty {
        Type::Tuple(elements) => elements.clone(),
        Type::Nominal { params, .. } => params.clone(),
        _ => Vec::new(),
    }
}

pub(crate) fn is_closure_literal(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::Lambda { .. } | Expression::Function { .. }
    )
}

pub(crate) fn is_prelude_container_type(ty: &Type) -> bool {
    ty.is_option() || ty.is_result() || ty.is_partial()
}
