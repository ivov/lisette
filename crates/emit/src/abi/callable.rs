use syntax::types::Type;

use crate::abi::layout::{FunctionLayout, SlotOrigin, ValueLayout};

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
    CommaOk,
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
    Result {
        bare_error: bool,
        payload: PayloadLayout,
    },
    Partial {
        payload: PayloadLayout,
    },
    Option {
        encoding: OptionReturnAbi,
        payload: PayloadLayout,
    },
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

    pub(crate) fn is_multi_return(&self) -> bool {
        matches!(
            self,
            Self::Result {
                bare_error: false,
                ..
            } | Self::Partial { .. }
                | Self::Option {
                    encoding: OptionReturnAbi::CommaOk,
                    ..
                }
                | Self::Tuple { .. }
        )
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
#[derive(Debug, Clone)]
pub(crate) struct CallableParamAbi {
    pub(crate) instantiated: Type,
    pub(crate) declared: Option<Type>,
    pub(crate) origin: SlotOrigin,
    pub(crate) layout: ValueLayout,
}

/// Complete physical contract consumed by call lowering.
#[derive(Debug, Clone)]
pub(crate) struct CallableAbi {
    pub(crate) params: Vec<CallableParamAbi>,
    pub(crate) result: CallableReturnAbi,
    pub(crate) return_layout: ValueLayout,
    pub(crate) return_payload_layout: Option<ValueLayout>,
}

impl CallableAbi {
    pub(crate) fn param(&self, index: usize) -> Option<&CallableParamAbi> {
        self.params.get(index).or_else(|| {
            self.params
                .last()
                .filter(|param| param.instantiated.get_name() == Some("VarArgs"))
        })
    }

    pub(crate) fn function_layout(&self) -> FunctionLayout {
        FunctionLayout {
            parameters: self
                .params
                .iter()
                .map(|param| param.layout.clone())
                .collect(),
            result: Box::new(self.return_layout.clone()),
            payload: self.return_payload_layout.clone().map(Box::new),
            return_abi: self.result.clone(),
        }
    }
}
