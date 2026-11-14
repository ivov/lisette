use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn defined_type(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed this type")
        .with_infer_code("embed_defined_type")
        .with_span_label(&span, "not embeddable yet")
        .with_help(format!(
            "Embed a record struct, an interface, or a pointer to one. Embedding `{}` is \
             not supported yet.",
            target
        ))
}

pub fn imported_target(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed an imported type yet")
        .with_infer_code("embed_imported_target")
        .with_span_label(&span, "imported embeds are not supported yet")
        .with_help(format!(
            "Embed a Lisette struct or interface for now. Embedding the imported Go type \
             `{}` is not supported yet.",
            target
        ))
}

pub fn comma_ok_abi_mismatch(interface: &str, method: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error(format!(
        "`{method}` returns its optional value in a way `{interface}` does not accept"
    ))
    .with_infer_code("comma_ok_abi_mismatch")
    .with_span_label(&span, format!("cannot be used as `{interface}` here"))
    .with_help(format!(
        "In Go, `{interface}.{method}` and the matching method on this value return their \
         optional result differently: one as a `(value, ok)` pair, the other as a single \
         value. Both look like `Option` in Lisette, but Go treats them as different method \
         signatures, so this value does not satisfy `{interface}`."
    ))
}

pub fn no_surface(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type cannot be embedded")
        .with_infer_code("embed_no_surface")
        .with_span_label(&span, "no methods or fields to promote")
        .with_help(format!(
            "Embed a struct, interface, or a named type with methods, or use a named field. \
             `{}` has no fields or methods, so embedding it would promote nothing.",
            target
        ))
}

pub fn pointer_to_interface(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer to an interface")
        .with_infer_code("embed_pointer_to_interface")
        .with_span_label(&span, "pointer to an interface")
        .with_help(format!(
            "Embed the interface by value. \
             `{}` embeds a pointer to an interface, which Go rejects.",
            target
        ))
}

pub fn nested_ref(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer to a pointer")
        .with_infer_code("embed_nested_ref")
        .with_span_label(&span, "pointer to a pointer")
        .with_help(format!(
            "Embed `T` or `Ref<T>`. \
             `{}` embeds a pointer to a pointer, which Go rejects.",
            target
        ))
}

pub fn pointer_backed_newtype(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer-backed type")
        .with_infer_code("embed_pointer_backed_newtype")
        .with_span_label(&span, "underlying type is a pointer")
        .with_help(format!(
            "Embed `T` or `Ref<T>`. `{}` is a defined type whose underlying type is a \
             pointer, which Go rejects as an embedded field.",
            target
        ))
}

pub fn non_interface_parent(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed this type in an interface")
        .with_infer_code("embed_non_interface")
        .with_span_label(&span, "not an interface")
        .with_help(format!(
            "Embed an interface, or declare the methods you need directly. \
             An interface can only embed another interface, but `{}` is not one.",
            target
        ))
}

pub fn option_target(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed `Option<T>`")
        .with_infer_code("embed_option_target")
        .with_span_label(&span, "embedded type cannot be `Option`")
        .with_help(
            "Embed `T` (or `Ref<T>` for a pointer). The zero value of the embedded type \
             decides whether an embed can be empty, so `embed` takes no `Option`.",
        )
}
