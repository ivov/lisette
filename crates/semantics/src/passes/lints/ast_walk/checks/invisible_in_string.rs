use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, FormatStringPart, Literal, Pattern};

pub fn check_invisible_in_string_expression(expression: &Expression, ctx: &NodeCtx) {
    let Expression::Literal { literal, span, .. } = expression else {
        return;
    };
    let found = match literal {
        Literal::String { value, .. } => first_invisible(value),
        Literal::FormatString(parts) => parts.iter().find_map(|part| match part {
            FormatStringPart::Text(text) => first_invisible(text),
            FormatStringPart::Expression(_) => None,
        }),
        _ => None,
    };
    if let Some((codepoint, name, is_bidi)) = found {
        ctx.sink.push(diagnostics::lint::invisible_in_string(
            span, codepoint, name, is_bidi,
        ));
    }
}

pub fn check_invisible_in_string_pattern(pattern: &Pattern, ctx: &NodeCtx) {
    let Pattern::Literal {
        literal: Literal::String { value, .. },
        span,
        ..
    } = pattern
    else {
        return;
    };
    if let Some((codepoint, name, is_bidi)) = first_invisible(value) {
        ctx.sink.push(diagnostics::lint::invisible_in_string(
            span, codepoint, name, is_bidi,
        ));
    }
}

fn first_invisible(text: &str) -> Option<(u32, &'static str, bool)> {
    text.chars()
        .find_map(|c| classify_invisible(c).map(|(name, is_bidi)| (c as u32, name, is_bidi)))
}

fn classify_invisible(c: char) -> Option<(&'static str, bool)> {
    match c {
        '\u{00A0}' => Some(("no-break space", false)),
        '\u{200B}' => Some(("zero-width space", false)),
        '\u{200C}' => Some(("zero-width non-joiner", false)),
        '\u{200D}' => Some(("zero-width joiner", false)),
        '\u{202A}' => Some(("left-to-right embedding", true)),
        '\u{202B}' => Some(("right-to-left embedding", true)),
        '\u{202C}' => Some(("pop directional formatting", true)),
        '\u{202D}' => Some(("left-to-right override", true)),
        '\u{202E}' => Some(("right-to-left override", true)),
        '\u{2066}' => Some(("left-to-right isolate", true)),
        '\u{2067}' => Some(("right-to-left isolate", true)),
        '\u{2068}' => Some(("first strong isolate", true)),
        '\u{2069}' => Some(("pop directional isolate", true)),
        '\u{FEFF}' => Some(("zero-width no-break space", false)),
        _ => None,
    }
}
