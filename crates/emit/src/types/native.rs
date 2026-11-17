use syntax::program::NativeTypeKind;

pub(crate) fn method_prefix(kind: NativeTypeKind) -> &'static str {
    match kind {
        NativeTypeKind::String => "String",
        _ => kind.name(),
    }
}
