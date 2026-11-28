use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::types::return_abi::{
    CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout, go_payload_layout,
};
use syntax::program::{GoHint, GoReturnForm, Method, SlotOrigin};
use syntax::types::Type;

fn direct_return_abi(form: GoReturnForm, partial: Option<PayloadLayout>) -> CallableReturnAbi {
    let lowered = match (form, partial) {
        (GoReturnForm::BareError, _) => LoweredReturnAbi::BareError,
        (GoReturnForm::Result, _) => LoweredReturnAbi::Result {
            payload: PayloadLayout::Packed,
        },
        (GoReturnForm::NullableOption, _) => LoweredReturnAbi::Option(OptionReturnAbi::Nullable),
        (GoReturnForm::CommaOkOption, _) => LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        }),
        (GoReturnForm::Tuple(arity), _) => LoweredReturnAbi::Tuple { arity },
        (GoReturnForm::Partial, Some(payload)) => LoweredReturnAbi::Partial { payload },
        (GoReturnForm::Void, _) => return CallableReturnAbi::Void,
        (GoReturnForm::Partial, None) | (GoReturnForm::Value, _) => {
            return CallableReturnAbi::Value;
        }
    };
    CallableReturnAbi::Lowered(lowered)
}

pub(crate) fn function_return_abi(facts: &EmitFacts<'_>, return_ty: &Type) -> CallableReturnAbi {
    direct_return_abi(facts.go_return_form(return_ty), None)
}

pub(crate) fn method_return_abi(facts: &EmitFacts<'_>, return_ty: &Type) -> CallableReturnAbi {
    direct_return_abi(facts.go_return_form(return_ty), Some(PayloadLayout::Packed))
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
    match direct_return_abi(facts.go_return_form(return_ty), Some(PayloadLayout::Packed)) {
        CallableReturnAbi::Lowered(abi) if abi.payload().is_some() => CallableReturnAbi::Lowered(
            abi.with_payload(go_payload_layout(&facts.ok_payload(return_ty))),
        ),
        abi @ (CallableReturnAbi::Lowered(_)
        | CallableReturnAbi::Void
        | CallableReturnAbi::Value) => abi,
    }
}

pub(crate) fn interface_method_return_abi(
    facts: &EmitFacts<'_>,
    method: &Method,
) -> CallableReturnAbi {
    let return_ty = &method
        .ty
        .as_function_type()
        .expect("an interface method must have a function type")
        .return_type;
    if facts.interface_method_is_tagged(&method.ty) {
        return CallableReturnAbi::Value;
    }
    let abi = method_return_abi(facts, return_ty);
    if abi == CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        && method.go_hints.contains(&GoHint::CommaOk)
    {
        return CallableReturnAbi::Lowered(LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
            payload: PayloadLayout::Packed,
        }));
    }
    abi
}
