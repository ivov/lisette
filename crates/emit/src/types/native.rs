use syntax::program::NativeTypeKind;

pub(crate) fn has_type_params(kind: NativeTypeKind) -> bool {
    !matches!(kind, NativeTypeKind::String)
}

pub(crate) fn emit_type_syntax(kind: NativeTypeKind, type_args: &[String]) -> String {
    match kind {
        NativeTypeKind::Slice => format!("[]{}", type_args[0]),
        NativeTypeKind::EnumeratedSlice => format!("[]{}", type_args[0]),
        NativeTypeKind::Map => format!("map[{}]{}", type_args[0], type_args[1]),
        NativeTypeKind::Channel => format!("chan {}", type_args[0]),
        NativeTypeKind::Sender => format!("chan<- {}", type_args[0]),
        NativeTypeKind::Receiver => format!("<-chan {}", type_args[0]),
        NativeTypeKind::String => "string".to_string(),
        NativeTypeKind::Array => unreachable!("Array types are lowered directly in go_type"),
    }
}

pub(crate) fn method_prefix(kind: NativeTypeKind) -> &'static str {
    match kind {
        NativeTypeKind::String => "String",
        _ => kind.name(),
    }
}
