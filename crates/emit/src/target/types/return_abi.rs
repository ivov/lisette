use crate::target::types::go_type::returns_go_void;
use syntax::program::{Definition, GoHint, GoReturnContract, go_return_contract};
use syntax::types::{self, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadLayout {
    Packed,
    Flattened,
}

impl PayloadLayout {
    pub(crate) fn is_flattened(self) -> bool {
        matches!(self, Self::Flattened)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionReturnAbi {
    CommaOk { payload: PayloadLayout },
    Nullable,
    Sentinel(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallableReturnAbi {
    Void,
    Value,
    Lowered(LoweredReturnAbi),
}

impl CallableReturnAbi {
    pub(crate) fn unlowered(return_type: &Type) -> Self {
        if returns_go_void(return_type) {
            Self::Void
        } else {
            Self::Value
        }
    }

    pub(crate) fn lowered(&self) -> Option<&LoweredReturnAbi> {
        match self {
            Self::Lowered(lowered) => Some(lowered),
            Self::Void | Self::Value => None,
        }
    }

    pub(crate) fn is_lowered(&self) -> bool {
        matches!(self, Self::Lowered(_))
    }

    pub(crate) fn has_flattened_payload(&self) -> bool {
        self.lowered()
            .is_some_and(LoweredReturnAbi::has_flattened_payload)
    }

    pub(crate) fn same_logical_contract(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Lowered(left), Self::Lowered(right)) => left.same_logical_contract(right),
            (Self::Void | Self::Value, Self::Void | Self::Value) => true,
            _ => self == other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoweredReturnAbi {
    BareError,
    Result { payload: PayloadLayout },
    Partial { payload: PayloadLayout },
    Option(OptionReturnAbi),
    Tuple { arity: usize },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LoweredFailure {
    Error { payload: Option<PayloadLayout> },
    Absent(OptionReturnAbi),
    Unfailing,
}

impl LoweredReturnAbi {
    pub(crate) fn failure(&self) -> LoweredFailure {
        match self {
            Self::BareError => LoweredFailure::Error { payload: None },
            Self::Result { payload } | Self::Partial { payload } => LoweredFailure::Error {
                payload: Some(*payload),
            },
            Self::Option(option) => LoweredFailure::Absent(*option),
            Self::Tuple { .. } => LoweredFailure::Unfailing,
        }
    }

    pub(crate) fn payload(&self) -> Option<PayloadLayout> {
        match self {
            Self::Result { payload }
            | Self::Partial { payload }
            | Self::Option(OptionReturnAbi::CommaOk { payload }) => Some(*payload),
            Self::BareError
            | Self::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_))
            | Self::Tuple { .. } => None,
        }
    }

    pub(crate) fn with_payload(self, payload: PayloadLayout) -> Self {
        match self {
            Self::Result { .. } => Self::Result { payload },
            Self::Partial { .. } => Self::Partial { payload },
            Self::Option(OptionReturnAbi::CommaOk { .. }) => {
                Self::Option(OptionReturnAbi::CommaOk { payload })
            }
            other @ Self::BareError | other @ Self::Option(_) | other @ Self::Tuple { .. } => other,
        }
    }

    pub(crate) fn has_flattened_payload(&self) -> bool {
        self.payload().is_some_and(PayloadLayout::is_flattened)
    }

    pub(crate) fn same_logical_contract(&self, other: &Self) -> bool {
        self.clone().with_payload(PayloadLayout::Packed)
            == other.clone().with_payload(PayloadLayout::Packed)
    }
}

pub(crate) fn go_return_abi<'d>(
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

pub(crate) fn go_payload_layout(ok_ty: &Type) -> PayloadLayout {
    if ok_ty.tuple_arity().is_some_and(|arity| arity >= 2) {
        PayloadLayout::Flattened
    } else {
        PayloadLayout::Packed
    }
}
