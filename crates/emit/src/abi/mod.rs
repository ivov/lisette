pub(crate) mod callable;
pub(crate) mod catalog;
pub(crate) mod coercion;
pub(crate) mod layout;
pub(crate) mod transition;

use crate::Planner;
use crate::context::expression::ExpressionContext;
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
        self.classify_direct_emission(return_ty).map_or_else(
            || CallableReturnAbi::unlowered(return_ty),
            CallableReturnAbi::Lowered,
        )
    }

    /// A method keeps Go's `(T, error)` for `Partial`, so the type itself
    /// satisfies Go interfaces such as `io.Reader`. Go's pair cannot tell
    /// `Err(e)` from `Both(zero, e)`, so a non-nilable payload reads back as `Both`.
    pub(crate) fn method_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        let peeled = self.facts.peel_alias(return_ty);
        if peeled.is_partial() && self.err_slot_is_nilable(&peeled) {
            return CallableReturnAbi::Lowered(LoweredReturnAbi::Partial {
                payload: PayloadLayout::Packed,
            });
        }
        self.callable_return_abi(return_ty)
    }

    pub(crate) fn slot_return_abi(
        &self,
        return_ty: &Type,
        origin: SlotOrigin,
    ) -> CallableReturnAbi {
        // Go code that receives a function as `any` sees its direct Go
        // signature, the same one a Lisette function declaration has.
        if matches!(origin, SlotOrigin::GoAny) {
            return self.classify_direct_emission(return_ty).map_or_else(
                || CallableReturnAbi::unlowered(return_ty),
                CallableReturnAbi::Lowered,
            );
        }
        // Internal callback storage must have the same type before and after
        // generic instantiation, including unit and container results.
        if !origin.declared_by_go() {
            return CallableReturnAbi::Value;
        }
        self.classify_slot_emission(return_ty, origin).map_or_else(
            || CallableReturnAbi::unlowered(return_ty),
            CallableReturnAbi::Lowered,
        )
    }

    pub(crate) fn function_expression_return_abi(
        &self,
        return_ty: &Type,
        ctx: ExpressionContext<'_>,
    ) -> CallableReturnAbi {
        if ctx.forces_tagged_go_function() || ctx.result_fills_type_parameter() {
            CallableReturnAbi::Value
        } else {
            self.slot_return_abi(return_ty, ctx.function_slot_origin())
        }
    }

    /// A Go-named function type renders as its Go name, so a value of that
    /// type has Go's shape in any slot.
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

    /// A composite value's layout after context-directed function emission.
    pub(crate) fn emitted_value_layout(
        &self,
        value: &Expression,
        ctx: ExpressionContext<'_>,
    ) -> ValueLayout {
        let ty = value.get_type();
        let slot_origin = ctx.function_slot_origin();
        let origin = if self.is_go_callable(value) {
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
        let result_type = &layout.result_type;
        let result = self.value_layout(result_type, SlotOrigin::Lisette);
        let return_abi = self.function_expression_return_abi(result_type, ctx);
        ValueLayout::Function {
            function_type,
            layout: self.function_layout(layout.parameters, &result, return_abi, None),
        }
    }

    /// Named function slots retain their declaration's physical signature.
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

    /// The direct Go layout that `any` holds for a function type, with its
    /// Lisette layout, or `None` when both have the same representation.
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
            || self.facts.function_alias_declaration(value_ty).is_some()
            || self.facts.function_alias_declaration(target_ty).is_some())
            && self.facts.resolve_to_function_type(value_ty).is_some()
            && self.facts.resolve_to_function_type(target_ty).is_some())
        .then_some(slot_origin)
    }

    pub(crate) fn value_slot_coercion(&self, value: &Expression, target_ty: &Type) -> CoercionPlan {
        let bridge = self.function_slot_bridge(value, target_ty);
        if bridge.is_identity() {
            let internal = CoercionPlan::internal(self, &value.get_type(), target_ty);
            if !internal.is_identity() {
                return internal;
            }
            let target = self.value_layout(target_ty, SlotOrigin::Lisette);
            if matches!(
                target.underlying(),
                ValueLayout::TaggedOption { .. }
                    | ValueLayout::Fallible { .. }
                    | ValueLayout::Tuple { .. }
                    | ValueLayout::Reference { .. }
            ) {
                let source = self.value_layout(&value.get_type(), SlotOrigin::Lisette);
                let bridge = CoercionPlan::bridge(self, &source, &target);
                // References can change a named function type by conversion,
                // but adapting a copied pointee would lose storage identity.
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

    /// Lowered shape for the slot at `origin`, or `None` to keep it tagged.
    pub(crate) fn classify_slot_emission(
        &self,
        return_ty: &Type,
        origin: SlotOrigin,
    ) -> Option<LoweredReturnAbi> {
        let peeled = self.facts.peel_alias(return_ty);
        if origin.declared_by_go() && peeled.is_partial() && self.err_slot_is_nilable(&peeled) {
            return Some(LoweredReturnAbi::Partial {
                payload: go_payload_layout(&peeled),
            });
        }
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
        // Go's value/error pair cannot distinguish Err from Both(zero, error).
        // Functions keep Partial tagged; methods and Go slots use the pair.
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
