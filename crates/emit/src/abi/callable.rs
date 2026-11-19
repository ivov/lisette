use syntax::ast::Expression;
use syntax::types::Type;

use crate::EmitFacts;

use crate::Planner;
use crate::types::go_type::returns_go_void;

use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::expressions::staging::VariadicCombine;
use syntax::program::SlotOrigin;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoweredReturnAbi {
    BareError,
    Result { payload: PayloadLayout },
    Partial { payload: PayloadLayout },
    Option(OptionReturnAbi),
    Tuple { arity: usize },
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

#[derive(Debug, Clone)]
pub(crate) struct CallableParamAbi {
    pub(crate) instantiated: Type,
    pub(crate) declared: Option<Type>,
    pub(crate) origin: SlotOrigin,
    pub(crate) layout: ValueLayout,
    pub(crate) variadic: Option<ValueLayout>,
}

#[derive(Debug, Clone)]
pub(crate) struct CallableAbi {
    pub(crate) params: Vec<CallableParamAbi>,
    pub(crate) result: CallableReturnAbi,
    pub(crate) return_layout: ValueLayout,
    pub(crate) result_declaration: Option<Type>,
}

impl CallableAbi {
    pub(crate) fn param(&self, index: usize) -> Option<&CallableParamAbi> {
        self.params.get(index).or_else(|| self.variadic_param())
    }

    pub(crate) fn variadic_param(&self) -> Option<&CallableParamAbi> {
        self.params.last().filter(|param| param.variadic.is_some())
    }

    pub(crate) fn spread_wraps_to_any(
        &self,
        facts: &EmitFacts<'_>,
        spread: Option<&Expression>,
    ) -> bool {
        let (Some(spread), Some(variadic)) = (spread, self.variadic_param()) else {
            return false;
        };
        facts.resolves_to_unknown(&variadic.instantiated)
            && spread
                .get_type()
                .inner()
                .is_some_and(|ty| !facts.resolves_to_unknown(&ty))
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
            self.result_declaration.as_ref(),
        )
    }
}
