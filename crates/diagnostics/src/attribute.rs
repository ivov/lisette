use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn field_attribute_without_struct_attribute(
    field_span: &Span,
    attribute_name: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Orphan field attribute")
        .with_attribute_code("orphan_field_attribute")
        .with_span_label(field_span, "field has attribute but struct does not")
        .with_help(format!(
            "Add `#[{}]` atop the struct definition to enable field-level attributes",
            attribute_name
        ))
}

pub fn duplicate_tag_key(span: &Span, key: &str, first_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate tag")
        .with_attribute_code("duplicate_tag")
        .with_span_label(span, "duplicate")
        .with_span_label(first_span, "first occurrence")
        .with_help(format!(
            "Remove one of the `{}` attributes - each tag key may appear only once per field",
            key
        ))
}

pub fn conflicting_case_transforms(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Conflicting case transforms")
        .with_attribute_code("conflicting_case_transforms")
        .with_span_label(span, "conflicting")
        .with_help("Choose either `snake_case` or `camel_case`, not both")
}
