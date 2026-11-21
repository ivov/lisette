pub(crate) mod bridge;
pub(crate) mod callable;
pub(crate) mod catalog;
pub(crate) mod coercion;
pub(crate) mod fallible_producer;
#[allow(clippy::disallowed_methods)]
pub(crate) mod layout;
pub(crate) mod slot_conversion;
pub(crate) mod transition;
pub(crate) mod wrapping;

use crate::EmitFacts;
use crate::Planner;
use crate::names::go_name;
use crate::patterns::matching::prelude_constructor;
use crate::plan::calls::{CallableDeclaration, CallableOrigin, FunctionBridge};
use crate::state::expression::ExpressionContext;
use callable::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use coercion::CoercionPlan;
use layout::{FunctionLayout, ValueLayout};
use syntax::ast::Expression;
use syntax::program::{
    CallOperation, CheckedCallee, Definition, GoHint, GoReturnContract, Method, NativeTypeKind,
    SlotOrigin, go_return_contract,
};
use syntax::types::{self, Type};

fn go_return_abi<'d>(
    return_ty: &Type,
    hints: &[GoHint],
    lookup: impl Fn(&str) -> Option<&'d Definition> + Copy,
) -> Option<LoweredReturnAbi> {
    let payload = || go_payload_layout(&types::ok_payload(return_ty, lookup));
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

fn go_payload_layout(ok_ty: &Type) -> PayloadLayout {
    if ok_ty.tuple_arity().is_some_and(|arity| arity >= 2) {
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

pub(crate) struct CalleeSignature<'s, 'a> {
    pub(crate) callee: &'s Expression,
    pub(crate) origin: &'s CallableOrigin<'a>,
    pub(crate) id: Option<&'s str>,
    pub(crate) declaration: Option<CallableDeclaration<'a>>,
    pub(crate) declared_return: Option<&'s Type>,
    pub(crate) return_type: &'s Type,
}

impl Planner<'_> {
    pub(crate) fn callee_return_abi(
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
            return self
                .facts
                .return_abi(signature.return_type, ReturnSlot::Function);
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
            let abi = self.facts.go_callable_return_abi(
                &format!(
                    "{}.{}",
                    requirement.declaring_interface, requirement.method.source_name
                ),
                declared_return,
                &requirement.method.go_hints,
            );
            return abi.is_lowered().then_some(abi);
        }
        let id = signature.id.filter(|id| go_name::is_go_import(id))?;
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
        Some(self.facts.go_callable_return_abi(
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
        if self
            .facts
            .is_prelude_dispatch(signature.id, signature.origin)
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
            return Some(self.facts.return_abi(
                method.ty.get_function_ret().unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &method.source_name,
                    method,
                },
            ));
        }
        if self.selects_tagged_twin(callee) {
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
                self.function_type_origin(&callee_ty, SlotOrigin::Lisette),
            ),
        };
        Some(self.facts.return_abi(return_ty, slot))
    }

    pub(crate) fn function_type_origin(&self, fn_ty: &Type, origin: SlotOrigin) -> SlotOrigin {
        if origin.declared_by_go() {
            origin
        } else if self.facts.is_go_named_function_type(fn_ty) {
            SlotOrigin::GoParameter
        } else if let Some(target) = self.facts.alias_declaration(fn_ty) {
            self.function_type_origin(target, origin)
        } else {
            origin
        }
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
        let return_abi = self
            .facts
            .return_abi(result_type, ReturnSlot::FunctionExpression(ctx));
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
        Some(callee.function_layout(self))
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
                .function_type_slot_bridge(&inner.get_type(), ty)
                .is_identity()
        {
            return value;
        }
        self.peel_go_slot_alias_cast(inner, ctx)
    }

    pub(crate) fn callable_slot_bridge(
        &self,
        source: FunctionLayout,
        slot_function: &Type,
        return_abi: Option<CallableReturnAbi>,
        origin: SlotOrigin,
    ) -> Option<FunctionBridge> {
        let slot = self.value_layout(slot_function, origin);
        let ValueLayout::Function { layout, .. } = slot.underlying() else {
            return None;
        };
        let target = match return_abi {
            Some(return_abi) if layout.return_abi != return_abi => {
                layout.with_return_abi(self, return_abi, SlotOrigin::Lisette)
            }
            _ => layout.clone(),
        };
        (!source.same_representation(&target)).then_some(FunctionBridge { source, target })
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

    pub(in crate::abi) fn function_type_slot_bridge(
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
}

pub(crate) fn is_closure_literal(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Lambda { .. } | Expression::Function { .. }
    )
}

pub(crate) fn is_prelude_container_type(ty: &Type) -> bool {
    ty.is_option() || ty.is_result() || ty.is_partial()
}

impl EmitFacts<'_> {
    pub(crate) fn go_callable_return_abi(
        &self,
        qualified_name: &str,
        declared_return: &Type,
        hints: &[GoHint],
    ) -> CallableReturnAbi {
        if self.go_callable_return_slot(qualified_name).is_none() {
            return self.return_abi(declared_return, ReturnSlot::Go { hints });
        }
        self.go_abi_catalog()
            .callable_return_abi(qualified_name)
            .cloned()
            .map_or_else(
                || CallableReturnAbi::unlowered(declared_return),
                CallableReturnAbi::Lowered,
            )
    }

    pub(crate) fn is_go_named_function_type(&self, ty: &Type) -> bool {
        matches!(ty.unwrap_forall(), Type::Nominal { id, .. } if go_name::is_go_import(id))
            && self.resolve_to_function_type(ty).is_some()
    }

    fn err_slot_is_nilable(&self, fallible_ty: &Type) -> bool {
        let err = self.err_payload(fallible_ty);
        self.is_prelude_error(&err) || self.is_nilable_go_type(&err)
    }

    fn lowered_partial(
        &self,
        return_ty: &Type,
        payload: impl FnOnce(&Type) -> PayloadLayout,
    ) -> Option<LoweredReturnAbi> {
        (self.is_partial(return_ty) && self.err_slot_is_nilable(return_ty)).then(|| {
            LoweredReturnAbi::Partial {
                payload: payload(&self.ok_payload(return_ty)),
            }
        })
    }

    fn classify_direct_emission(&self, return_ty: &Type) -> Option<LoweredReturnAbi> {
        if self.is_result(return_ty) && self.err_slot_is_nilable(return_ty) {
            return Some(if self.ok_payload(return_ty).is_unit() {
                LoweredReturnAbi::BareError
            } else {
                LoweredReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                }
            });
        }
        if self.is_option(return_ty) {
            let encoding = if self.is_nullable_option(return_ty) {
                OptionReturnAbi::Nullable
            } else {
                OptionReturnAbi::CommaOk {
                    payload: PayloadLayout::Packed,
                }
            };
            return Some(LoweredReturnAbi::Option(encoding));
        }
        if let Some(elements) = self.tuple_elements(return_ty)
            && elements.len() >= 2
        {
            return Some(LoweredReturnAbi::Tuple {
                arity: elements.len(),
            });
        }
        None
    }

    fn function_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        self.classify_direct_emission(return_ty).map_or_else(
            || CallableReturnAbi::unlowered(return_ty),
            CallableReturnAbi::Lowered,
        )
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
            abi.with_payload(go_payload_layout(&self.ok_payload(return_ty)))
        } else {
            abi
        })
    }

    fn method_return_abi(&self, return_ty: &Type) -> CallableReturnAbi {
        match self.lowered_partial(return_ty, |_| PayloadLayout::Packed) {
            Some(abi) => CallableReturnAbi::Lowered(abi),
            None => self.function_return_abi(return_ty),
        }
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

    fn interface_method_return_abi(
        &self,
        return_ty: &Type,
        name: &str,
        method: &Method,
    ) -> CallableReturnAbi {
        if self.method_uses_tagged_return(name) {
            return CallableReturnAbi::Value;
        }
        let abi = match self.method_return_abi(return_ty) {
            CallableReturnAbi::Value if self.returns_go_void(return_ty) => CallableReturnAbi::Void,
            abi => abi,
        };
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

    pub(crate) fn return_abi(&self, return_ty: &Type, slot: ReturnSlot<'_>) -> CallableReturnAbi {
        match slot {
            ReturnSlot::Function => self.function_return_abi(return_ty),
            ReturnSlot::Method => self.method_return_abi(return_ty),
            ReturnSlot::InterfaceMethod { name, method } => {
                self.interface_method_return_abi(return_ty, name, method)
            }
            ReturnSlot::FunctionValue(origin) => self.slot_return_abi(return_ty, origin),
            ReturnSlot::FunctionExpression(ctx) => {
                if ctx.forces_tagged_go_function()
                    || ctx.result_fills_type_parameter()
                    || ctx.function_slot_origin() == SlotOrigin::GoAny
                {
                    CallableReturnAbi::Value
                } else {
                    self.slot_return_abi(return_ty, ctx.function_slot_origin())
                }
            }
            ReturnSlot::Go { hints } => go_return_abi(return_ty, hints, |id| self.definition(id))
                .map_or_else(
                    || CallableReturnAbi::unlowered(return_ty),
                    CallableReturnAbi::Lowered,
                ),
        }
    }
}
