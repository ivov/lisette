pub(crate) mod bridge;
pub(crate) mod callable;
pub(crate) mod catalog;
pub(crate) mod coercion;
pub(crate) mod fallible_producer;
pub(crate) mod layout;
pub(crate) mod transition;
pub(crate) mod wrapping;

use crate::Planner;
use crate::names::go_name;
use crate::plan::calls::CallableOrigin;
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use coercion::CoercionPlan;
use layout::{FunctionLayout, ValueLayout};
use syntax::ast::Expression;
use syntax::program::{
    Adaptation, Definition, GoHint, GoReturnContract, Method, SlotOrigin, go_return_contract,
};
use syntax::types::{self, Type};

fn go_return_abi<'d>(
    return_ty: &Type,
    hints: &[GoHint],
    lookup: impl Fn(&str) -> Option<&'d Definition> + Copy,
) -> Option<LoweredReturnAbi> {
    let peeled = types::peel_alias(return_ty, lookup);
    let payload = || go_payload_layout(&peeled);
    Some(match go_return_contract(return_ty, hints, lookup) {
        GoReturnContract::Value => return None,
        GoReturnContract::Error => LoweredReturnAbi::BareError,
        GoReturnContract::Result => LoweredReturnAbi::Result { payload: payload() },
        GoReturnContract::Partial => LoweredReturnAbi::Partial { payload: payload() },
        GoReturnContract::CommaOk => {
            LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { payload: payload() })
        }
        GoReturnContract::Nullable => LoweredReturnAbi::Option(OptionReturnAbi::Nullable),
        GoReturnContract::Sentinel(value) => {
            LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(value))
        }
        GoReturnContract::Tuple(arity) => LoweredReturnAbi::Tuple { arity },
    })
}

fn go_payload_layout(return_ty: &Type) -> PayloadLayout {
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

#[derive(Clone, Copy)]
pub(crate) enum ReturnSlot<'a> {
    Function,
    Method,
    InterfaceMethod { name: &'a str, method: &'a Method },
    FunctionValue(SlotOrigin),
    FunctionExpression(ExpressionContext<'a>),
    Go { hints: &'a [GoHint] },
}

impl Planner<'_> {
    pub(crate) fn return_abi(&self, return_ty: &Type, slot: ReturnSlot<'_>) -> CallableReturnAbi {
        match slot {
            ReturnSlot::Function => self.function_return_abi(return_ty),
            ReturnSlot::Method => self.method_return_abi(return_ty),
            ReturnSlot::InterfaceMethod { name, method } => {
                self.interface_method_return_abi(return_ty, name, method)
            }
            ReturnSlot::FunctionValue(origin) => self.slot_return_abi(return_ty, origin),
            ReturnSlot::FunctionExpression(ctx) => {
                if ctx.forces_tagged_go_function() || ctx.result_fills_type_parameter() {
                    CallableReturnAbi::Value
                } else {
                    self.slot_return_abi(return_ty, ctx.function_slot_origin())
                }
            }
            ReturnSlot::Go { hints } => {
                go_return_abi(return_ty, hints, |id| self.facts.definition(id)).map_or_else(
                    || CallableReturnAbi::unlowered(return_ty),
                    CallableReturnAbi::Lowered,
                )
            }
        }
    }

    fn function_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        self.classify_direct_emission(return_ty).map_or_else(
            || CallableReturnAbi::unlowered(return_ty),
            CallableReturnAbi::Lowered,
        )
    }

    fn method_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        match self.lowered_partial(return_ty, |_| PayloadLayout::Packed) {
            Some(abi) => CallableReturnAbi::Lowered(abi),
            None => self.function_return_abi(return_ty),
        }
    }

    fn interface_method_return_abi(
        &self,
        return_ty: &Type,
        name: &str,
        method: &Method,
    ) -> CallableReturnAbi {
        if self.facts.method_uses_tagged_return(name) {
            return CallableReturnAbi::Value;
        }
        let abi = self.method_return_abi(&self.facts.peel_alias(return_ty));
        if abi == CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
            && method.go_hints.contains(&GoHint::CommaOk)
        {
            return CallableReturnAbi::Lowered(LoweredReturnAbi::Option(
                OptionReturnAbi::CommaOk {
                    payload: PayloadLayout::Packed,
                },
            ));
        }
        abi
    }

    fn slot_return_abi(&self, return_ty: &Type, origin: SlotOrigin) -> CallableReturnAbi {
        if matches!(origin, SlotOrigin::GoAny) {
            return self.function_return_abi(return_ty);
        }
        if !origin.declared_by_go() {
            return CallableReturnAbi::Value;
        }
        self.classify_slot_emission(return_ty, origin).map_or_else(
            || CallableReturnAbi::unlowered(return_ty),
            CallableReturnAbi::Lowered,
        )
    }

    pub(crate) fn function_type_origin(&self, fn_ty: &Type, origin: SlotOrigin) -> SlotOrigin {
        if origin.declared_by_go() {
            origin
        } else if self.is_go_named_function_type(fn_ty) {
            SlotOrigin::GoParameter
        } else if let Some(target) = self.facts.alias_declaration(fn_ty) {
            self.function_type_origin(target, origin)
        } else {
            origin
        }
    }

    pub(crate) fn is_go_named_function_type(&self, ty: &Type) -> bool {
        matches!(ty.unwrap_forall(), Type::Nominal { id, .. } if go_name::is_go_import(id))
            && self.facts.resolve_to_function_type(ty).is_some()
    }

    pub(crate) fn emitted_value_layout(
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
        let layout = self.value_layout(&ty, origin);
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
        let return_abi = self.return_abi(result_type, ReturnSlot::FunctionExpression(ctx));
        ValueLayout::Function {
            function_type,
            layout: layout.with_return_abi(self, return_abi, SlotOrigin::Lisette),
        }
    }

    pub(crate) fn go_slot_declaration_layout(
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
        Some(callee.abi.function_layout(self))
    }

    pub(crate) fn peel_go_slot_alias_cast<'e>(
        &self,
        value: &'e Expression,
        ctx: ExpressionContext<'_>,
    ) -> &'e Expression {
        let Expression::Cast {
            expression: inner,
            ty,
            ..
        } = value.unwrap_parens()
        else {
            return value;
        };
        if !ctx.function_slot_origin().declared_by_go() || self.is_go_named_function_type(ty) {
            return value;
        }
        let Some(target) = self.facts.resolve_to_function_type(ty) else {
            return value;
        };
        let inner_ty = inner.get_type();
        match self.facts.resolve_to_function_type(&inner_ty) {
            Some(source) if source == target && !self.is_go_named_function_type(&inner_ty) => {
                self.peel_go_slot_alias_cast(inner, ctx)
            }
            _ => value,
        }
    }

    pub(crate) fn function_slot_bridge(
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

    pub(crate) fn any_slot_function_layouts(
        &self,
        ty: &Type,
    ) -> Option<(ValueLayout, ValueLayout)> {
        self.facts.resolve_to_function_type(ty)?;
        let stored = self.value_layout(ty, SlotOrigin::GoAny);
        let lisette = self.value_layout(ty, SlotOrigin::Lisette);
        (!stored.same_representation(&lisette)).then_some((stored, lisette))
    }

    fn go_function_slot_origin(&self, value_ty: &Type, target_ty: &Type) -> Option<SlotOrigin> {
        let slot_origin = self.function_type_origin(target_ty, SlotOrigin::Lisette);
        ((slot_origin.declared_by_go()
            || self.facts.mentions_function_alias(value_ty)
            || self.facts.mentions_function_alias(target_ty))
            && self.facts.resolve_to_function_type(value_ty).is_some()
            && self.facts.resolve_to_function_type(target_ty).is_some())
        .then_some(slot_origin)
    }

    pub(crate) fn value_slot_coercion(&self, value: &Expression, layout: &Type) -> CoercionPlan {
        let value_use = self.facts.checked(value).value_use();
        self.adapted_slot_coercion(value, &value_use.adaptation, &value_use.slot, layout)
    }

    pub(crate) fn adapted_slot_coercion(
        &self,
        value: &Expression,
        adaptation: &Adaptation,
        slot: &Type,
        layout: &Type,
    ) -> CoercionPlan {
        let bridge = self.function_slot_bridge(value, layout);
        if bridge.is_identity() {
            let internal = CoercionPlan::adapted(self, adaptation, &value.get_type(), slot, None);
            if !internal.is_identity() {
                return internal;
            }
            let target = self.value_layout(layout, SlotOrigin::Lisette);
            if matches!(
                target.underlying(),
                ValueLayout::TaggedOption { .. }
                    | ValueLayout::Fallible { .. }
                    | ValueLayout::Tuple { .. }
                    | ValueLayout::Reference { .. }
            ) {
                let source = self.value_layout(&value.get_type(), SlotOrigin::Lisette);
                let bridge = CoercionPlan::bridge(self, &source, &target);
                let reference = matches!(target.underlying(), ValueLayout::Reference { .. });
                if !bridge.is_identity()
                    && (!reference
                        || matches!(
                            bridge,
                            CoercionPlan::Layout(coercion::LayoutBridge::Conversion { .. })
                        ))
                {
                    return bridge;
                }
            }
            internal
        } else {
            bridge
        }
    }

    fn lowered_partial(
        &self,
        return_ty: &Type,
        payload: impl FnOnce(&Type) -> PayloadLayout,
    ) -> Option<LoweredReturnAbi> {
        let peeled = self.facts.peel_alias(return_ty);
        (peeled.is_partial() && self.err_slot_is_nilable(&peeled)).then(|| {
            LoweredReturnAbi::Partial {
                payload: payload(&peeled),
            }
        })
    }

    fn classify_slot_emission(
        &self,
        return_ty: &Type,
        origin: SlotOrigin,
    ) -> Option<LoweredReturnAbi> {
        if origin.declared_by_go()
            && let Some(abi) = self.lowered_partial(return_ty, go_payload_layout)
        {
            return Some(abi);
        }
        let abi = self.classify_direct_emission(return_ty)?;
        Some(if origin.declared_by_go() && abi.payload().is_some() {
            abi.with_payload(go_payload_layout(&self.facts.peel_alias(return_ty)))
        } else {
            abi
        })
    }

    fn classify_direct_emission(&self, return_ty: &Type) -> Option<LoweredReturnAbi> {
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

    fn err_slot_is_nilable(&self, fallible_ty: &Type) -> bool {
        let err = fallible_ty.err_type();
        self.facts.is_prelude_error(&err) || self.facts.is_nilable_go_type(&err)
    }

    pub(crate) fn return_go_type(&self, abi: &CallableReturnAbi, return_ty: &Type) -> GoType {
        match abi {
            CallableReturnAbi::Void => return GoType::default(),
            CallableReturnAbi::Value => return self.go_type(return_ty),
            CallableReturnAbi::Lowered(_) => {}
        }
        let peeled = self.facts.peel_alias(return_ty);
        let result = self.value_layout(&peeled, SlotOrigin::Lisette);
        self.function_layout(Vec::new(), &result, abi.clone(), None)
            .result_go_type(self)
            .unwrap_or_else(|| self.go_type(&peeled))
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
