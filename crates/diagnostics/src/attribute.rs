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

pub fn iterate_non_unit_variant(attribute_span: &Span, variant_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[iterate]` on enum with payload variant")
        .with_attribute_code("iterate_non_unit_variant")
        .with_span_label(attribute_span, "disallowed if a variant has a payload")
        .with_span_label(variant_span, "this variant has a payload")
        .with_help("Remove the payload, or drop `#[iterate]`")
}

pub fn iterate_generic_enum(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[iterate]` on generic enum")
        .with_attribute_code("iterate_generic_enum")
        .with_span_label(attribute_span, "disallowed if enum has generics")
        .with_help("Remove the generic type parameters, or drop `#[iterate]`")
}

pub fn iterate_not_an_enum(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[iterate]` not on enum")
        .with_attribute_code("iterate_not_an_enum")
        .with_span_label(attribute_span, "not on an enum")
        .with_help("Only an enum can be marked `#[iterate]`")
}

pub fn iterate_in_typedef(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[iterate]` in a typedef")
        .with_attribute_code("iterate_in_typedef")
        .with_span_label(attribute_span, "disallowed in a `.d.lis` typedef")
        .with_help("Only enums in `.lis` source can be marked `#[iterate]`")
}

pub fn display_not_a_struct_or_enum(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[display]` not on a struct or enum")
        .with_attribute_code("display_not_a_struct_or_enum")
        .with_span_label(attribute_span, "not on a struct or enum")
        .with_help("Only a struct or enum can be marked `#[display]`")
}

pub fn display_in_typedef(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[display]` in a typedef")
        .with_attribute_code("display_in_typedef")
        .with_span_label(attribute_span, "disallowed in a `.d.lis` typedef")
        .with_help("Only structs or enums in `.lis` source can be marked `#[display]`")
}

pub fn display_with_arguments(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[display]` takes no arguments")
        .with_attribute_code("display_with_arguments")
        .with_span_label(attribute_span, "remove the arguments")
        .with_help("Write `#[display]` with no arguments")
}

pub fn display_on_pointer_newtype(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[display]` on a pointer-backed newtype")
        .with_attribute_code("display_on_pointer_newtype")
        .with_span_label(attribute_span, "a `Ref` has no display form")
        .with_help("Give the type named fields, or drop `#[display]`")
}

pub fn display_specialized_to_string(attribute_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`#[display]` conflicts with your `to_string`")
        .with_attribute_code("display_specialized_to_string")
        .with_span_label(attribute_span, "adds a `to_string` of its own")
        .with_help(
            "`#[display]` needs a plain `fn to_string(self) -> string` on the type. Write `to_string` in that form, or remove `#[display]`.",
        )
}

pub fn iterate_variants_conflict(
    attribute_span: &Span,
    existing_span: Option<&Span>,
) -> LisetteDiagnostic {
    let mut diagnostic =
        LisetteDiagnostic::error("`#[iterate]` conflicts with existing `variants`")
            .with_attribute_code("iterate_variants_conflict")
            .with_span_label(attribute_span, "would synthesize `variants`");
    if let Some(span) = existing_span {
        diagnostic = diagnostic.with_span_label(span, "`variants` already defined here");
    }
    diagnostic.with_help("Rename the existing `variants`, or drop `#[iterate]`")
}
