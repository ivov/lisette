use crate::LisetteDiagnostic;
use syntax::ast::Span;

pub fn non_exhaustive(match_span: Span, cases: &[String]) -> LisetteDiagnostic {
    let names: Vec<String> = cases.iter().map(|case| format!("`{}`", case)).collect();
    let noun = if cases.len() == 1 { "case" } else { "cases" };
    let example = cases
        .first()
        .map(|case| format!("`{} => {{ ... }}`", case))
        .unwrap_or_default();
    LisetteDiagnostic::error("`match` is not exhaustive")
        .with_infer_code("non_exhaustive")
        .with_span_label(&match_span, "not all patterns covered")
        .with_help(format!(
            "Handle the missing {} {}, e.g. {}",
            noun,
            join_and(&names),
            example
        ))
}

pub(crate) fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{} and {}", first, second),
        [rest @ .., last] => format!("{}, and {}", rest.join(", "), last),
    }
}

pub fn irrefutable_while_let(pattern_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Pattern always matches")
        .with_infer_code("irrefutable_while_let")
        .with_span_label(&pattern_span, "matches every value, so the loop never ends")
        .with_help("Use `loop` with `let` binding instead")
}

pub fn redundant_arm(
    span: Span,
    label: impl Into<String>,
    help: impl Into<String>,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Unreachable pattern")
        .with_infer_code("redundant_arm")
        .with_span_label(&span, label)
        .with_help(help)
}

#[derive(Debug, Clone, Copy)]
pub enum RefutablePattern<'a> {
    ExactSlice(usize),
    SlicePrefix(usize),
    Some,
    Ok,
    Other(&'a str),
}

pub fn refutable_pattern(pattern_span: Span, pattern: RefutablePattern<'_>) -> LisetteDiagnostic {
    let label = describe_pattern_expectation(pattern);
    let help = build_refutability_help(pattern);

    LisetteDiagnostic::error("Pattern might not match")
        .with_infer_code("refutable_pattern")
        .with_span_label(&pattern_span, label)
        .with_help(help)
}

fn describe_pattern_expectation(pattern: RefutablePattern<'_>) -> String {
    match pattern {
        RefutablePattern::ExactSlice(len) => {
            let word = if len == 1 { "element" } else { "elements" };
            format!("only matches {} {}", len, word)
        }
        RefutablePattern::SlicePrefix(len) => {
            format!("only matches {} or more elements", len)
        }
        RefutablePattern::Some => "only matches `Some`".to_string(),
        RefutablePattern::Ok => "only matches `Ok`".to_string(),
        RefutablePattern::Other(witness) => format!("does not match `{}`", witness),
    }
}

fn build_refutability_help(pattern: RefutablePattern<'_>) -> String {
    match pattern {
        RefutablePattern::ExactSlice(_) | RefutablePattern::SlicePrefix(_) => {
            "Handle slices of any length with `match slice { [a, b] => ..., _ => ... }`".to_string()
        }
        RefutablePattern::Some => {
            "Use `if let Some(x) = opt { ... }` to handle only `Some`, or `match opt { Some(x) => ..., None => ... }` to also handle `None`".to_string()
        }
        RefutablePattern::Ok => {
            "Use `if let Ok(x) = result { ... }` to handle only `Ok`, or `match result { Ok(x) => ..., Err(e) => ... }` to also handle `Err`".to_string()
        }
        RefutablePattern::Other(witness) => format!(
            "Handle all cases with `match value {{ {} => ..., _ => ... }}`",
            witness
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_refutable_patterns_do_not_infer_kind_from_witness_text() {
        let diagnostic =
            refutable_pattern(Span::new(0, 0, 1), RefutablePattern::Other("MyNoneVariant"));

        assert_eq!(
            diagnostic.plain_label(),
            Some("does not match `MyNoneVariant`")
        );
    }
}
