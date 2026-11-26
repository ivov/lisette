use crate::target::names::go_name;
use crate::target::types::display_form::DisplayForm;
use syntax::ast::{EnumVariant, Generic, VariantFields};
use syntax::containment::enum_payload_pointer_wrapped;
use syntax::go_names;
use syntax::program::Definition;
use syntax::types::Type;

#[derive(Debug, Clone)]
pub(crate) struct EnumLayout {
    pub(crate) enum_name: String,
    pub(crate) variants: Vec<VariantLayout>,
    pub(crate) generics: Vec<Generic>,
    pub(crate) default_variant: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct VariantLayout {
    pub(crate) name: String,
    pub(crate) tag_constant: String,
    pub(crate) is_struct_variant: bool,
    pub(crate) fields: Vec<FieldLayout>,
    pub(crate) doc: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonPayload {
    Object,
    Value,
    Array,
}

impl VariantLayout {
    pub(crate) fn json_payload(&self) -> JsonPayload {
        if self.is_struct_variant {
            JsonPayload::Object
        } else if self.fields.len() == 1 {
            JsonPayload::Value
        } else {
            JsonPayload::Array
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FieldLayout {
    pub(crate) source_name: String,
    pub(crate) go_name: String,
    pub(crate) ty: Type,
    pub(crate) kind: FieldKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FieldKind {
    Plain(DisplayForm),
    Recursive,
}

impl FieldLayout {
    pub(crate) fn format(&self) -> DisplayForm {
        match self.kind {
            FieldKind::Plain(format) => format,
            FieldKind::Recursive => DisplayForm::Value,
        }
    }

    pub(crate) fn is_recursive(&self) -> bool {
        matches!(self.kind, FieldKind::Recursive)
    }
}

impl EnumLayout {
    pub(crate) fn new<'d>(
        enum_id: &str,
        generics: &[Generic],
        variants: &[EnumVariant],
        default_variant: Option<usize>,
        definition: impl Fn(&str) -> Option<&'d Definition> + Copy,
    ) -> Self {
        let enum_name = go_name::unqualified_name(enum_id).to_string();

        let slots = go_names::enum_field_slots(&enum_name, variants);
        let variants: Vec<_> = variants
            .iter()
            .enumerate()
            .map(|(variant_index, v)| {
                Self::compute_variant_layout(
                    variant_index,
                    v,
                    enum_id,
                    &slots[variant_index],
                    definition,
                )
            })
            .collect();
        Self {
            enum_name,
            variants,
            generics: generics.to_vec(),
            default_variant,
        }
    }

    pub(crate) fn tag_go_type(&self) -> &'static str {
        if self.variants.len() <= 256 {
            "uint8"
        } else {
            "uint16"
        }
    }

    pub(crate) fn tag_values(&self) -> Vec<usize> {
        let Some(default_index) = self.default_variant.filter(|index| *index != 0) else {
            return (0..self.variants.len()).collect();
        };
        let mut next = 1;
        (0..self.variants.len())
            .map(|index| {
                if index == default_index {
                    return 0;
                }
                let value = next;
                next += 1;
                value
            })
            .collect()
    }

    fn compute_variant_layout<'d>(
        variant_index: usize,
        variant: &EnumVariant,
        enum_id: &str,
        slots: &[String],
        definition: impl Fn(&str) -> Option<&'d Definition>,
    ) -> VariantLayout {
        let enum_name = go_name::unqualified_name(enum_id);
        let tag_constant = go_name::enum_tag_constant(enum_name, &variant.name);

        let is_struct_variant = matches!(variant.fields, VariantFields::Struct(_));

        let fields: Vec<_> = variant
            .fields
            .iter()
            .enumerate()
            .map(|(field_index, field)| {
                let source_name = if is_struct_variant {
                    field.name.to_string()
                } else {
                    field_index.to_string()
                };

                let go_name = slots[field_index].clone();

                let kind = if enum_payload_pointer_wrapped(
                    enum_id,
                    variant_index,
                    field_index,
                    &field.ty,
                    &definition,
                ) {
                    FieldKind::Recursive
                } else {
                    FieldKind::Plain(DisplayForm::of(&field.ty, &definition))
                };

                FieldLayout {
                    source_name,
                    go_name,
                    ty: field.ty.clone(),
                    kind,
                }
            })
            .collect();

        VariantLayout {
            name: variant.name.to_string(),
            tag_constant,
            is_struct_variant,
            fields,
            doc: variant.doc.clone(),
        }
    }

    pub(crate) fn variant(&self, name: &str) -> &VariantLayout {
        self.variants
            .iter()
            .find(|variant| variant.name == name)
            .unwrap_or_else(|| panic!("internal error: the enum layout has no variant `{name}`"))
    }

    pub(crate) fn struct_field_name(&self, variant_name: &str, field_name: &str) -> String {
        self.variant(variant_name)
            .fields
            .iter()
            .find(|field| field.source_name == field_name)
            .map(|field| field.go_name.clone())
            .unwrap_or_else(|| {
                panic!("internal error: the variant `{variant_name}` has no field `{field_name}`")
            })
    }
}
