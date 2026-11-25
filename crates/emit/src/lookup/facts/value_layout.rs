use crate::lookup::facts::emit_facts::EmitFacts;
use crate::target::types::return_abi::{CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi};
use syntax::program::SlotOrigin;
use syntax::types::Type;

#[derive(Debug, Clone)]
pub(crate) struct FunctionLayout {
    pub(crate) parameters: Vec<ValueLayout>,
    pub(crate) result_layout: Box<ValueLayout>,
    pub(crate) results: Vec<ValueLayout>,
    pub(crate) return_abi: CallableReturnAbi,
    pub(crate) producer: ResultProducer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultProducer {
    Go,
    Lisette,
}

impl FunctionLayout {
    pub(crate) fn single_result_layout(&self) -> Option<ValueLayout> {
        match self.return_abi.lowered() {
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                Some(ValueLayout::NullableOption {
                    option_type: self.result_layout.logical_type().clone(),
                    payload: Box::new(self.results.first()?.clone()),
                })
            }
            _ => None,
        }
    }

    pub(crate) fn with_return_abi(
        &self,
        facts: &EmitFacts<'_>,
        return_abi: CallableReturnAbi,
        origin: SlotOrigin,
    ) -> Self {
        let result = facts.value_layout(self.result_layout.logical_type(), origin);
        FunctionLayout {
            producer: self.producer,
            ..facts.function_layout(self.parameters.clone(), &result, return_abi, None)
        }
    }

    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        self.same_parameter_representation(other) && self.same_result_representation(other)
    }

    pub(crate) fn same_parameter_representation(&self, other: &Self) -> bool {
        layouts_match(&self.parameters, &other.parameters)
    }

    pub(crate) fn same_result_representation(&self, other: &Self) -> bool {
        self.return_abi == other.return_abi && layouts_match(&self.results, &other.results)
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ValueLayout {
    Plain(Type),
    Fallible {
        fallible_type: Type,
        payload: Box<ValueLayout>,
        error: Box<ValueLayout>,
    },
    TaggedOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    NullableOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    PointerOption {
        option_type: Type,
        payload: Box<ValueLayout>,
    },
    Reference {
        reference_type: Type,
        pointee: Box<ValueLayout>,
    },
    Slice {
        collection_type: Type,
        element: Box<ValueLayout>,
    },
    Map {
        collection_type: Type,
        key: Box<ValueLayout>,
        value: Box<ValueLayout>,
    },
    Array {
        array_type: Type,
        length: u64,
        element: Box<ValueLayout>,
    },
    Function {
        function_type: Type,
        layout: FunctionLayout,
    },
    Tuple {
        tuple_type: Type,
        elements: Vec<ValueLayout>,
    },
    Named {
        named_type: Type,
        underlying: Box<ValueLayout>,
        go_alias: bool,
    },
}

impl ValueLayout {
    pub(crate) fn underlying(&self) -> &Self {
        match self {
            Self::Named { underlying, .. } => underlying.underlying(),
            _ => self,
        }
    }

    pub(crate) fn payload(&self) -> Option<&Self> {
        match self.underlying() {
            Self::Fallible { payload, .. }
            | Self::TaggedOption { payload, .. }
            | Self::NullableOption { payload, .. }
            | Self::PointerOption { payload, .. } => Some(payload),
            _ => None,
        }
    }

    pub(crate) fn element(&self) -> Option<&Self> {
        match self.underlying() {
            Self::Slice { element, .. } | Self::Array { element, .. } => Some(element),
            _ => None,
        }
    }

    pub(crate) fn variant_field(&self, variant: &str, index: usize) -> Option<&Self> {
        match self {
            Self::TaggedOption { payload, .. }
            | Self::NullableOption { payload, .. }
            | Self::PointerOption { payload, .. } => Some(payload),
            Self::Fallible { payload, error, .. } => Some(if variant == "Err" || index == 1 {
                error
            } else {
                payload
            }),
            _ => None,
        }
    }

    pub(crate) fn indexed_element(&self) -> Option<&Self> {
        match self.underlying() {
            Self::Map { value, .. } => Some(value),
            _ => self.element(),
        }
    }

    pub(crate) fn tuple_elements(&self) -> Option<&[Self]> {
        match self.underlying() {
            Self::Tuple { elements, .. } => Some(elements),
            _ => None,
        }
    }

    pub(crate) fn same_representation(&self, other: &Self) -> bool {
        if let Self::Named {
            underlying,
            go_alias: true,
            ..
        } = self
        {
            return underlying.same_representation(other);
        }
        if let Self::Named {
            underlying,
            go_alias: true,
            ..
        } = other
        {
            return self.same_representation(underlying);
        }
        if self.function_name() != other.function_name() {
            return false;
        }
        match (self, other) {
            (Self::Plain(_), Self::Plain(_)) => true,
            (
                Self::Fallible {
                    payload: left,
                    error: left_error,
                    ..
                },
                Self::Fallible {
                    payload: right,
                    error: right_error,
                    ..
                },
            ) => left.same_representation(right) && left_error.same_representation(right_error),
            (
                Self::TaggedOption { payload: left, .. },
                Self::TaggedOption { payload: right, .. },
            )
            | (
                Self::NullableOption { payload: left, .. },
                Self::NullableOption { payload: right, .. },
            )
            | (
                Self::PointerOption { payload: left, .. },
                Self::PointerOption { payload: right, .. },
            )
            | (Self::Reference { pointee: left, .. }, Self::Reference { pointee: right, .. })
            | (Self::Slice { element: left, .. }, Self::Slice { element: right, .. })
            | (
                Self::Named {
                    underlying: left, ..
                },
                Self::Named {
                    underlying: right, ..
                },
            ) => left.same_representation(right),
            (
                Self::Array {
                    length: left_length,
                    element: left,
                    ..
                },
                Self::Array {
                    length: right_length,
                    element: right,
                    ..
                },
            ) => left_length == right_length && left.same_representation(right),
            (
                Self::Map {
                    key: left_key,
                    value: left_value,
                    ..
                },
                Self::Map {
                    key: right_key,
                    value: right_value,
                    ..
                },
            ) => {
                left_key.same_representation(right_key)
                    && left_value.same_representation(right_value)
            }
            (Self::Function { layout: left, .. }, Self::Function { layout: right, .. }) => {
                left.same_representation(right)
            }
            (
                Self::Tuple { elements: left, .. },
                Self::Tuple {
                    elements: right, ..
                },
            ) => layouts_match(left, right),
            _ => false,
        }
    }

    pub(crate) fn function_name(&self) -> Option<&Type> {
        match self {
            Self::Named {
                underlying,
                go_alias: true,
                ..
            } => underlying.function_name(),
            Self::Named {
                named_type,
                underlying,
                go_alias: false,
            } if matches!(underlying.underlying(), Self::Function { .. }) => Some(named_type),
            Self::Function { function_type, .. }
                if matches!(function_type.unwrap_forall(), Type::Nominal { .. }) =>
            {
                Some(function_type)
            }
            _ => None,
        }
    }

    pub(crate) fn logical_type(&self) -> &Type {
        match self {
            Self::Plain(ty)
            | Self::Fallible {
                fallible_type: ty, ..
            }
            | Self::TaggedOption {
                option_type: ty, ..
            }
            | Self::NullableOption {
                option_type: ty, ..
            }
            | Self::PointerOption {
                option_type: ty, ..
            }
            | Self::Reference {
                reference_type: ty, ..
            }
            | Self::Slice {
                collection_type: ty,
                ..
            }
            | Self::Map {
                collection_type: ty,
                ..
            }
            | Self::Array { array_type: ty, .. }
            | Self::Function {
                function_type: ty, ..
            }
            | Self::Tuple { tuple_type: ty, .. }
            | Self::Named { named_type: ty, .. } => ty,
        }
    }
}

fn layouts_match(left: &[ValueLayout], right: &[ValueLayout]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.same_representation(right))
}
