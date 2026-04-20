use syntax::program::NativeTypeKind;
use syntax::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeGoType {
    Slice,
    EnumeratedSlice,
    Map,
    Channel,
    Sender,
    Receiver,
    String,
}

impl NativeGoType {
    pub(crate) fn from_kind(kind: NativeTypeKind) -> Self {
        match kind {
            NativeTypeKind::Slice => Self::Slice,
            NativeTypeKind::EnumeratedSlice => Self::EnumeratedSlice,
            NativeTypeKind::Map => Self::Map,
            NativeTypeKind::Channel => Self::Channel,
            NativeTypeKind::Sender => Self::Sender,
            NativeTypeKind::Receiver => Self::Receiver,
            NativeTypeKind::String => Self::String,
        }
    }

    pub(crate) fn from_type(ty: &Type) -> Option<Self> {
        NativeTypeKind::from_type(ty).map(Self::from_kind)
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        NativeTypeKind::from_name(name).map(Self::from_kind)
    }

    pub(crate) fn has_type_params(&self) -> bool {
        !matches!(self, Self::String)
    }

    pub(crate) fn emit_type_syntax(&self, type_args: &[String]) -> String {
        match self {
            Self::Slice => format!("[]{}", type_args[0]),
            Self::EnumeratedSlice => format!("[]{}", type_args[0]),
            Self::Map => format!("map[{}]{}", type_args[0], type_args[1]),
            Self::Channel => format!("chan {}", type_args[0]),
            Self::Sender => format!("chan<- {}", type_args[0]),
            Self::Receiver => format!("<-chan {}", type_args[0]),
            Self::String => "string".to_string(),
        }
    }

    pub(crate) fn lisette_name(&self) -> &'static str {
        match self {
            Self::Slice => "Slice",
            Self::EnumeratedSlice => "EnumeratedSlice",
            Self::Map => "Map",
            Self::Channel => "Channel",
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::String => "string",
        }
    }

    pub(crate) fn method_prefix(&self) -> &'static str {
        match self {
            Self::Slice => "Slice",
            Self::EnumeratedSlice => "EnumeratedSlice",
            Self::Map => "Map",
            Self::Channel => "Channel",
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
            Self::String => "String",
        }
    }
}
