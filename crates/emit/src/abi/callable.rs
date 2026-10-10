use syntax::types::Type;

use crate::Planner;

use crate::abi::layout::{FunctionLayout, SlotOrigin, ValueLayout};
use crate::expressions::staging::VariadicCombine;

/// How a logical tuple payload occupies a callable's physical Go result slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadLayout {
    /// One Go result containing Lisette's generated tuple value.
    Packed,
    /// One Go result per tuple element.
    Flattened,
}

impl PayloadLayout {
    pub(crate) fn is_flattened(self) -> bool {
        matches!(self, Self::Flattened)
    }
}

/// Physical encoding of an `Option<T>` result at a callable boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionReturnAbi {
    CommaOk { payload: PayloadLayout },
    Nullable,
    Sentinel(i64),
}

/// Physical Go result contract for a callable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CallableReturnAbi {
    /// One generated Lisette tagged value (`Result`, `Option`, or tuple).
    Tagged,
    /// No generated tagged/lowered boundary encoding is required.
    Direct,
    Lowered(LoweredReturnAbi),
}

/// A `Result`, `Partial`, `Option`, or tuple spread across Go result slots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoweredReturnAbi {
    /// A `Result<(), E>` represented by its error value alone.
    BareError,
    Result {
        payload: PayloadLayout,
    },
    Partial {
        payload: PayloadLayout,
    },
    Option(OptionReturnAbi),
    Tuple {
        arity: usize,
    },
}

impl CallableReturnAbi {
    pub(crate) fn lowered(&self) -> Option<&LoweredReturnAbi> {
        match self {
            Self::Lowered(lowered) => Some(lowered),
            Self::Tagged | Self::Direct => None,
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
            _ => self == other,
        }
    }
}

impl LoweredReturnAbi {
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
            other => other,
        }
    }

    pub(crate) fn has_flattened_payload(&self) -> bool {
        self.payload().is_some_and(PayloadLayout::is_flattened)
    }

    fn same_logical_contract(&self, other: &Self) -> bool {
        self.clone().with_payload(PayloadLayout::Packed)
            == other.clone().with_payload(PayloadLayout::Packed)
    }
}

/// The instantiated and declaration-level views of one callable parameter.
///
#[derive(Debug, Clone)]
pub(crate) struct CallableParamAbi {
    pub(crate) instantiated: Type,
    pub(crate) declared: Option<Type>,
    pub(crate) origin: SlotOrigin,
    pub(crate) layout: ValueLayout,
    pub(crate) variadic: Option<ValueLayout>,
}

/// Complete physical contract consumed by call lowering.
#[derive(Debug, Clone)]
pub(crate) struct CallableAbi {
    pub(crate) params: Vec<CallableParamAbi>,
    pub(crate) result: CallableReturnAbi,
    pub(crate) return_layout: ValueLayout,
}

impl CallableAbi {
    pub(crate) fn param(&self, index: usize) -> Option<&CallableParamAbi> {
        self.params.get(index).or_else(|| self.variadic_param())
    }

    pub(crate) fn variadic_param(&self) -> Option<&CallableParamAbi> {
        self.params.last().filter(|param| param.variadic.is_some())
    }

    pub(crate) fn variadic_combine(&self, extra_leading: usize) -> Option<VariadicCombine> {
        let variadic = self.variadic_param()?;
        Some(VariadicCombine {
            element_ty: variadic.instantiated.clone(),
            fixed_count: self.params.len() - 1 + extra_leading,
        })
    }

    pub(crate) fn function_layout(&self, planner: &Planner<'_>) -> FunctionLayout {
        planner.function_layout(
            self.params
                .iter()
                .map(|param| param.variadic.as_ref().unwrap_or(&param.layout).clone())
                .collect(),
            &self.return_layout,
            self.result.clone(),
        )
    }
}
