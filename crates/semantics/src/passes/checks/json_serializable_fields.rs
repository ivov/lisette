//! Reject channel- and function-typed fields on a `#[json]` struct or enum.
//! Go's `encoding/json` cannot marshal channels or functions, so such a type
//! fails to serialize at runtime.

use diagnostics::LocalSink;
use syntax::ast::{Annotation, AttributeArg, Expression, StructFieldDefinition};

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        check_item(item, sink);
    }
}

fn check_item(expression: &Expression, sink: &LocalSink) {
    match expression {
        Expression::Struct {
            attributes, fields, ..
        } if attributes.iter().any(|a| a.name == "json") => {
            for field in fields {
                if let Some(kind) = unserializable_kind(&field.annotation)
                    && !is_json_skipped(field)
                {
                    sink.push(diagnostics::infer::json_non_serializable_field(
                        &field.annotation.get_span(),
                        kind,
                        true,
                    ));
                }
            }
        }
        Expression::Enum {
            attributes,
            variants,
            ..
        } if attributes.iter().any(|a| a.name == "json") => {
            for variant in variants {
                for field in &variant.fields {
                    if let Some(kind) = unserializable_kind(&field.annotation) {
                        sink.push(diagnostics::infer::json_non_serializable_field(
                            &field.annotation.get_span(),
                            kind,
                            false,
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

fn unserializable_kind(annotation: &Annotation) -> Option<&'static str> {
    match annotation {
        Annotation::Function { .. } => Some("function"),
        Annotation::Constructor { name, .. }
            if matches!(name.as_str(), "Channel" | "Sender" | "Receiver") =>
        {
            Some("channel")
        }
        _ => None,
    }
}

fn is_json_skipped(field: &StructFieldDefinition) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| match attribute.name.as_str() {
            "json" => attribute.args.iter().any(skips_field),
            "tag" => {
                matches!(attribute.args.first(), Some(AttributeArg::String(key)) if key == "json")
                    && attribute.args.iter().skip(1).any(skips_field)
            }
            _ => false,
        })
}

fn skips_field(arg: &AttributeArg) -> bool {
    match arg {
        AttributeArg::Flag(flag) => flag == "skip",
        AttributeArg::String(value) => value == "-",
        AttributeArg::Raw(raw) => raw.contains("\"-\""),
        AttributeArg::NegatedFlag(_) => false,
    }
}
