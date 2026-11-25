use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::types::return_abi::{
    CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout, go_payload_layout,
};
use syntax::program::{GoHint, Method, SlotOrigin};
use syntax::types::Type;

fn err_slot_is_nilable(facts: &EmitFacts<'_>, fallible_ty: &Type) -> bool {
    let err = facts.err_payload(fallible_ty);
    facts.is_prelude_error(&err) || facts.is_nilable_go_type(&err)
}

fn lowered_partial(
    facts: &EmitFacts<'_>,
    return_ty: &Type,
    payload: impl FnOnce(&Type) -> PayloadLayout,
) -> Option<LoweredReturnAbi> {
    (facts.is_partial(return_ty) && err_slot_is_nilable(facts, return_ty)).then(|| {
        LoweredReturnAbi::Partial {
            payload: payload(&facts.ok_payload(return_ty)),
        }
    })
}

fn classify_direct_emission(facts: &EmitFacts<'_>, return_ty: &Type) -> Option<LoweredReturnAbi> {
    if facts.is_result(return_ty) && err_slot_is_nilable(facts, return_ty) {
        return Some(if facts.ok_payload(return_ty).is_unit() {
            LoweredReturnAbi::BareError
        } else {
            LoweredReturnAbi::Result {
                payload: PayloadLayout::Packed,
            }
        });
    }
    if facts.is_option(return_ty) {
        let encoding = if facts.is_nullable_option(return_ty) {
            OptionReturnAbi::Nullable
        } else {
            OptionReturnAbi::CommaOk {
                payload: PayloadLayout::Packed,
            }
        };

        return Some(LoweredReturnAbi::Option(encoding));
    }
    if let Some(elements) = facts.tuple_elements(return_ty)
        && elements.len() >= 2
    {
        return Some(LoweredReturnAbi::Tuple {
            arity: elements.len(),
        });
    }
    None
}

pub(crate) fn function_return_abi(facts: &EmitFacts<'_>, return_ty: &Type) -> CallableReturnAbi {
    classify_direct_emission(facts, return_ty).map_or_else(
        || CallableReturnAbi::unlowered(return_ty),
        CallableReturnAbi::Lowered,
    )
}

fn classify_slot_emission(
    facts: &EmitFacts<'_>,
    return_ty: &Type,
    origin: SlotOrigin,
) -> Option<LoweredReturnAbi> {
    if origin.declared_by_go()
        && let Some(abi) = lowered_partial(facts, return_ty, go_payload_layout)
    {
        return Some(abi);
    }
    let abi = classify_direct_emission(facts, return_ty)?;
    Some(if origin.declared_by_go() && abi.payload().is_some() {
        abi.with_payload(go_payload_layout(&facts.ok_payload(return_ty)))
    } else {
        abi
    })
}

pub(crate) fn method_return_abi(facts: &EmitFacts<'_>, return_ty: &Type) -> CallableReturnAbi {
    match lowered_partial(facts, return_ty, |_| PayloadLayout::Packed) {
        Some(abi) => CallableReturnAbi::Lowered(abi),
        None => function_return_abi(facts, return_ty),
    }
}

pub(crate) fn slot_return_abi(
    facts: &EmitFacts<'_>,
    return_ty: &Type,
    origin: SlotOrigin,
) -> CallableReturnAbi {
    if matches!(origin, SlotOrigin::GoAny) {
        return function_return_abi(facts, return_ty);
    }
    if !origin.declared_by_go() {
        return CallableReturnAbi::Value;
    }
    classify_slot_emission(facts, return_ty, origin).map_or_else(
        || CallableReturnAbi::unlowered(return_ty),
        CallableReturnAbi::Lowered,
    )
}

pub(crate) fn interface_method_return_abi(
    facts: &EmitFacts<'_>,
    return_ty: &Type,
    name: &str,
    method: &Method,
) -> CallableReturnAbi {
    if facts.method_uses_tagged_return(name) {
        return CallableReturnAbi::Value;
    }
    let abi = match method_return_abi(facts, return_ty) {
        CallableReturnAbi::Value if facts.returns_go_void(return_ty) => CallableReturnAbi::Void,
        abi => abi,
    };
    if abi == CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        && method.go_hints.contains(&GoHint::CommaOk)
    {
        return CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        }));
    }
    abi
}
