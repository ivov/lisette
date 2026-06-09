use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn defined_type(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed this type")
        .with_infer_code("embed_defined_type")
        .with_span_label(&span, "not embeddable in this version")
        .with_help(format!(
            "Embedding `{}` is not supported in this version. Embed a record struct, an \
             interface, or a pointer to one.",
            target
        ))
}

pub fn imported_target(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed an imported type yet")
        .with_infer_code("embed_imported_target")
        .with_span_label(&span, "imported embeds are not supported yet")
        .with_help(format!(
            "Embedding the imported Go type `{}` is not supported yet. For now, embed a \
             Lisette struct or interface.",
            target
        ))
}

pub fn no_surface(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Type cannot be embedded")
        .with_infer_code("embed_no_surface")
        .with_span_label(&span, "no methods or fields to promote")
        .with_help(format!(
            "`{}` has no selector surface, so embedding it would promote nothing. \
             Embed a struct, interface, or a named type with methods, or use a named field.",
            target
        ))
}

pub fn pointer_to_interface(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer to an interface")
        .with_infer_code("embed_pointer_to_interface")
        .with_span_label(&span, "pointer to an interface")
        .with_help(format!(
            "`{}` embeds a pointer to an interface, which Go rejects. \
             Embed the interface by value instead.",
            target
        ))
}

pub fn nested_ref(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer to a pointer")
        .with_infer_code("embed_nested_ref")
        .with_span_label(&span, "pointer to a pointer")
        .with_help(format!(
            "`{}` embeds a pointer to a pointer, which Go rejects. \
             Embed `T` or `Ref<T>`.",
            target
        ))
}

pub fn pointer_backed_newtype(target: &str, span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed a pointer-backed type")
        .with_infer_code("embed_pointer_backed_newtype")
        .with_span_label(&span, "underlying type is a pointer")
        .with_help(format!(
            "`{}` is a defined type whose underlying type is a pointer, which Go rejects \
             as an embedded field. Embed `T` or `Ref<T>`.",
            target
        ))
}

pub fn option_target(span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Cannot embed `Option<T>`")
        .with_infer_code("embed_option_target")
        .with_span_label(&span, "embedded type cannot be `Option`")
        .with_help(
            "An embed's nullability is the embedded type's own zero, so `embed` takes no \
             `Option`. Embed `T` (or `Ref<T>` for a pointer) instead.",
        )
}
