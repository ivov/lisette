use crate::LisetteDiagnostic;
use syntax::ast::Span;

use crate::IssueKind;

#[derive(Debug, Clone)]
pub struct PatternIssue {
    pub span: Span,
    pub kind: IssueKind,
}

pub fn non_exhaustive(match_span: Span, case: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::error("`match` is not exhaustive")
        .with_infer_code("non_exhaustive")
        .with_span_label(&match_span, "not all patterns covered")
        .with_help(format!(
            "Handle the missing case `{}`, e.g. `{} => {{ ... }}`",
            case, case
        ))
}

pub fn irrefutable_while_let(pattern_span: Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Pattern always matches")
        .with_infer_code("irrefutable_while_let")
        .with_span_label(&pattern_span, "always matches")
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

pub fn refutable_pattern(
    pattern_span: Span,
    witness: &str,
    slice_info: Option<(usize, bool)>,
) -> LisetteDiagnostic {
    let label = describe_pattern_expectation(witness, slice_info);
    let help = build_refutability_help(witness);

    LisetteDiagnostic::error("Pattern might not match")
        .with_infer_code("refutable_pattern")
        .with_span_label(&pattern_span, label)
        .with_help(help)
}

fn describe_pattern_expectation(witness: &str, slice_info: Option<(usize, bool)>) -> String {
    if witness.starts_with('[') {
        if let Some((len, has_rest)) = slice_info {
            if has_rest {
                return format!("only matches {} or more elements", len);
            }
            let word = if len == 1 { "element" } else { "elements" };
            return format!("only matches {} {}", len, word);
        }

        return "only matches specific length".to_string();
    }

    if witness.contains("None") {
        return "only matches `Some`".to_string();
    }

    if witness.contains("Err") {
        return "only matches `Ok`".to_string();
    }

    format!("does not match `{}`", witness)
}

fn build_refutability_help(witness: &str) -> String {
    if witness.starts_with('[') {
        return r#"Use `match` to handle slices of any length:
    match slice {
        [a, b] => ...,
        _ => ...,
    }"#
        .to_string();
    }

    if witness.contains("None") {
        return r#"Use `if let` to handle only `Some`:
    if let Some(x) = opt {
        ...
    }
Or use `match` to also handle `None`:
    match opt {
        Some(x) => ...,
        None => ...,
    }"#
        .to_string();
    }

    if witness.contains("Err") {
        return r#"Use `if let` to handle only `Ok`:
    if let Ok(x) = result {
        ...
    }
Or use `match` to also handle `Err`:
    match result {
        Ok(x) => ...,
        Err(e) => ...,
    }"#
        .to_string();
    }

    format!(
        "Use `match` to handle all cases:\n    match value {{\n        {} => ...,\n        _ => ...,\n    }}",
        witness
    )
}
