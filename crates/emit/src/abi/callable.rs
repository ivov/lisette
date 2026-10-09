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
    pub(crate) fn transition_to(&self, target: &Self) -> AbiTransition {
        if self == target {
            return AbiTransition::Identity;
        }
        match (self, target) {
            (Self::Tagged, target) if target.is_lowered() => AbiTransition::LowerFromTagged,
            (source, Self::Tagged) if source.is_lowered() => AbiTransition::WrapToTagged,
            (source, target) if source.is_lowered() && target.is_lowered() => {
                AbiTransition::Reencode
            }
            _ => AbiTransition::Incompatible,
        }
    }

    pub(crate) fn is_passthrough(&self) -> bool {
        matches!(self, Self::Tagged | Self::Direct)
    }

    pub(crate) fn is_lowered(&self) -> bool {
        !self.is_passthrough()
    }

    pub(crate) fn payload(&self) -> Option<PayloadLayout> {
        match self {
            Self::Result { payload }
            | Self::Partial { payload }
            | Self::Option(OptionReturnAbi::CommaOk { payload }) => Some(*payload),
            Self::Tagged
            | Self::Direct
            | Self::BareError
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

    pub(crate) fn same_logical_contract(&self, other: &Self) -> bool {
        self.clone().with_payload(PayloadLayout::Packed)
            == other.clone().with_payload(PayloadLayout::Packed)
    }
}

/// Required conversion between two callable result contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbiTransition {
    Identity,
    /// Convert a tagged Lisette result into a lowered Go result.
    LowerFromTagged,
    /// Reconstruct the tagged Lisette result from lowered Go results.
    WrapToTagged,
    /// Convert between two non-tagged physical layouts through the logical value.
    Reencode,
    /// The contracts describe different logical result types.
    Incompatible,
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
