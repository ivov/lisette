use super::NormalizedPattern;
use super::types::INTERFACE_UNKNOWN_TAG;
use syntax::ast::Literal;

pub fn format_witness(pattern: &NormalizedPattern) -> String {
    match pattern {
        NormalizedPattern::Wildcard => "_".to_string(),

        // Literals are never produced as witnesses by the exhaustiveness algorithm.
        // When patterns contain literals, missing coverage is shown as `_` (wildcard).
        NormalizedPattern::Literal(_) => unreachable!("literals cannot be witnesses"),

        NormalizedPattern::Constructor {
            type_name,
            tag,
            args,
        } => {
            if type_name.starts_with("Slice") {
                return format_slice_pattern(pattern);
            }

            if type_name.starts_with("Tuple") {
                let formatted_args = args
                    .iter()
                    .map(format_witness)
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("({})", formatted_args);
            }

            let display_tag = strip_module_prefix(tag);

            if display_tag == "__value_enum_unknown__" || display_tag == INTERFACE_UNKNOWN_TAG {
                return "_".to_string();
            }

            if args.is_empty() {
                display_tag
            } else {
                let formatted_args = args
                    .iter()
                    .map(format_witness)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", display_tag, formatted_args)
            }
        }
    }
}

/// Formats a normalized pattern for display (e.g., in error messages).
/// Unlike `format_witness()`, this can handle patterns with literal fields,
/// since user-written patterns may contain literals in struct constructors.
pub fn format_pattern(pattern: &NormalizedPattern) -> String {
    match pattern {
        NormalizedPattern::Wildcard => "_".to_string(),

        // User-written patterns can contain literals in their fields
        NormalizedPattern::Literal(lit) => format_literal(lit),

        NormalizedPattern::Constructor {
            type_name,
            tag,
            args,
        } => {
            if type_name.starts_with("Slice") {
                return format_slice_pattern_for_display(pattern);
            }

            if type_name.starts_with("Tuple") {
                let formatted_args = args
                    .iter()
                    .map(format_pattern)
                    .collect::<Vec<_>>()
                    .join(", ");
                return format!("({})", formatted_args);
            }

            let display_tag = strip_module_prefix(tag);

            if display_tag == "__value_enum_unknown__" || display_tag == INTERFACE_UNKNOWN_TAG {
                return "_".to_string();
            }

            if args.is_empty() {
                display_tag
            } else {
                let formatted_args = args
                    .iter()
                    .map(format_pattern)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({})", display_tag, formatted_args)
            }
        }
    }
}

fn format_literal(lit: &Literal) -> String {
    match lit {
        Literal::Integer { text, value } => text.as_ref().unwrap_or(&value.to_string()).clone(),
        Literal::Float { text, value } => text.as_ref().unwrap_or(&value.to_string()).clone(),
        Literal::Imaginary(val) => format!("{}i", val),
        Literal::Boolean(b) => b.to_string(),
        Literal::String(s) => format!("\"{}\"", s),
        Literal::Char(c) => format!("'{}'", c),
        Literal::FormatString(_) => "f\"...\"".to_string(),
        Literal::Slice(_) => "[...]".to_string(),
    }
}

fn strip_module_prefix(tag: &str) -> String {
    let parts: Vec<&str> = tag.split('.').collect();
    if parts.len() >= 2 {
        parts[1..].join(".")
    } else {
        tag.to_string()
    }
}

fn format_slice_pattern(pattern: &NormalizedPattern) -> String {
    let mut elements = Vec::new();
    let mut current = pattern;
    let mut ends_with_rest = false;

    loop {
        match current {
            NormalizedPattern::Constructor { tag, .. } if tag == "EmptySlice" => {
                break;
            }
            NormalizedPattern::Constructor { tag, args, .. } if tag == "NonEmptySlice" => {
                if args.len() >= 2 {
                    elements.push(format_witness(&args[0]));
                    current = &args[1];
                } else {
                    break;
                }
            }
            NormalizedPattern::Wildcard => {
                ends_with_rest = true;
                break;
            }
            _ => {
                elements.push(format_witness(current));
                break;
            }
        }
    }

    if ends_with_rest {
        elements.push("..".to_string());
    }

    format!("[{}]", elements.join(", "))
}

fn format_slice_pattern_for_display(pattern: &NormalizedPattern) -> String {
    let mut elements = Vec::new();
    let mut current = pattern;
    let mut ends_with_rest = false;

    loop {
        match current {
            NormalizedPattern::Constructor { tag, .. } if tag == "EmptySlice" => {
                break;
            }
            NormalizedPattern::Constructor { tag, args, .. } if tag == "NonEmptySlice" => {
                if args.len() >= 2 {
                    elements.push(format_pattern(&args[0]));
                    current = &args[1];
                } else {
                    break;
                }
            }
            NormalizedPattern::Wildcard => {
                ends_with_rest = true;
                break;
            }
            _ => {
                elements.push(format_pattern(current));
                break;
            }
        }
    }

    if ends_with_rest {
        elements.push("..".to_string());
    }

    format!("[{}]", elements.join(", "))
}
