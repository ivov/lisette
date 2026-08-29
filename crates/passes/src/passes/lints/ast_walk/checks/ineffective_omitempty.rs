use diagnostics::lint::NeverEmptyType;
use syntax::ast::{Attribute, AttributeArg, Expression, Span, StructFieldDefinition, StructFields};
use syntax::attributes::struct_attribute_forces_field_export;
use syntax::program::DefinitionBody;
use syntax::types::{SimpleKind, Type};

use crate::passes::walk::NodeCtx;

const JSON_KEY: &str = "json";

pub fn check_ineffective_omitempty(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Struct {
        attributes, fields, ..
    } = expression
    else {
        return;
    };

    // Emit writes tags for named record fields only.
    let StructFields::Record(fields) = fields else {
        return;
    };
    let struct_tag = struct_json_tag(attributes);

    for field in fields.iter().filter(|field| !field.is_embedded()) {
        let Some(source) = omitempty_source(field, struct_tag.as_ref()) else {
            continue;
        };
        let Some(never_empty) = never_empty_type(ctx, &field.ty, source.omitzero) else {
            continue;
        };
        ctx.sink.push(diagnostics::lint::ineffective_omitempty(
            source.span,
            never_empty,
            source.set_by,
        ));
    }
}

#[derive(Clone, Copy, Default)]
struct JsonTagSettings {
    omitempty: Option<bool>,
    omitzero: Option<bool>,
}

impl JsonTagSettings {
    fn apply(&mut self, arguments: &[AttributeArg]) {
        if let Some(setting) = flag_setting(arguments, "omitempty") {
            self.omitempty = Some(setting);
        }
        if let Some(setting) = flag_setting(arguments, "omitzero") {
            self.omitzero = Some(setting);
        }
    }

    fn omits_nothing(self) -> bool {
        self.omitempty == Some(true) && self.omitzero != Some(true)
    }
}

struct StructJsonTag<'a> {
    span: &'a Span,
    settings: JsonTagSettings,
}

struct OmitemptySource<'a> {
    span: &'a Span,
    set_by: Option<&'a Span>,
    omitzero: Option<bool>,
}

fn struct_json_tag(attributes: &[Attribute]) -> Option<StructJsonTag<'_>> {
    let mut tag: Option<StructJsonTag> = None;
    for attribute in attributes {
        if !struct_attribute_forces_field_export(attribute) {
            continue;
        }
        let Some(JsonTag::Structured(arguments)) = json_tag(attribute) else {
            continue;
        };
        let tag = tag.get_or_insert(StructJsonTag {
            span: &attribute.span,
            settings: JsonTagSettings::default(),
        });
        tag.settings.apply(arguments);
        if flag_setting(arguments, "omitempty") == Some(true) {
            tag.span = &attribute.span;
        }
    }
    tag
}

fn omitempty_source<'a>(
    field: &'a StructFieldDefinition,
    struct_tag: Option<&StructJsonTag<'a>>,
) -> Option<OmitemptySource<'a>> {
    for attribute in field.attributes() {
        let arguments = match json_tag(attribute) {
            None => continue,
            Some(JsonTag::Opaque) => return None,
            Some(JsonTag::Structured(arguments)) => arguments,
        };
        if attribute.name == JSON_KEY && arguments.iter().any(is_raw) {
            return None;
        }
        if has_flag(arguments, "skip") {
            return None;
        }
        let mut settings = match attribute.name.as_str() {
            "tag" => JsonTagSettings::default(),
            _ => struct_tag.map(|tag| tag.settings).unwrap_or_default(),
        };
        let own_omitempty = flag_setting(arguments, "omitempty") == Some(true);
        settings.apply(arguments);
        if !settings.omits_nothing() {
            return None;
        }
        return Some(if own_omitempty {
            OmitemptySource {
                span: &attribute.span,
                set_by: None,
                omitzero: settings.omitzero,
            }
        } else {
            OmitemptySource {
                span: &field.name_span,
                set_by: struct_tag.map(|tag| tag.span),
                omitzero: settings.omitzero,
            }
        });
    }

    let tag = struct_tag?;
    tag.settings.omits_nothing().then_some(OmitemptySource {
        span: &field.name_span,
        set_by: Some(tag.span),
        omitzero: tag.settings.omitzero,
    })
}

enum JsonTag<'a> {
    /// A raw-mode tag, which emit writes through unread.
    Opaque,
    Structured(&'a [AttributeArg]),
}

fn json_tag(attribute: &Attribute) -> Option<JsonTag<'_>> {
    if attribute.name == JSON_KEY {
        return Some(JsonTag::Structured(&attribute.args));
    }
    if attribute.name != "tag" {
        return None;
    }
    match attribute.args.first()? {
        AttributeArg::String(key) if key == JSON_KEY => {
            Some(JsonTag::Structured(&attribute.args[1..]))
        }
        AttributeArg::Raw(raw) if raw.split(':').next() == Some(JSON_KEY) => Some(JsonTag::Opaque),
        _ => None,
    }
}

fn is_raw(argument: &AttributeArg) -> bool {
    matches!(argument, AttributeArg::Raw(_))
}

fn has_flag(arguments: &[AttributeArg], name: &str) -> bool {
    arguments
        .iter()
        .any(|argument| matches!(argument, AttributeArg::Flag(flag) if flag == name))
}

fn flag_setting(arguments: &[AttributeArg], name: &str) -> Option<bool> {
    arguments.iter().rev().find_map(|argument| match argument {
        AttributeArg::Flag(flag) if flag == name => Some(true),
        AttributeArg::NegatedFlag(flag) if flag == name => Some(false),
        _ => None,
    })
}

fn never_empty_type(ctx: &NodeCtx, ty: &Type, omitzero: Option<bool>) -> Option<NeverEmptyType> {
    // Emit writes `omitzero` in place of `omitempty` for a directly `Option`
    // field, unless the tag turns `omitzero` off.
    if omitzero != Some(false) && ctx.store.peel_alias(ty).is_option() {
        return None;
    }
    match ctx.store.peel_underlying(ty) {
        Type::Array { length, .. } if length > 0 => Some(NeverEmptyType::Array),
        Type::Tuple(_) => Some(NeverEmptyType::Tuple),
        Type::Simple(SimpleKind::Unit) | Type::Never => Some(NeverEmptyType::Struct),
        Type::Nominal { id, .. } => match ctx.store.get_definition(id.as_str())?.body {
            DefinitionBody::Struct { .. } => Some(NeverEmptyType::Struct),
            DefinitionBody::Enum { .. } => Some(NeverEmptyType::Enum),
            _ => None,
        },
        _ => None,
    }
}
