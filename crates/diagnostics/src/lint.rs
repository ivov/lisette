use crate::LisetteDiagnostic;
use syntax::ast::{DeadCodeCause, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    RedundantLetElse,
    RedundantIfLet,
    UnreachableIfLetElse,
    RedundantIfLetElse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnusedExpressionKind {
    Literal,
    Result,
    Option,
    Partial,
    Value,
}

impl UnusedExpressionKind {
    pub fn lint_name(&self) -> &'static str {
        match self {
            Self::Literal => "unused_literal",
            Self::Result => "unused_result",
            Self::Option => "unused_option",
            Self::Partial => "unused_partial",
            Self::Value => "unused_value",
        }
    }
}

pub fn unused_variable(span: &Span, name: &str, is_struct_field: bool) -> LisetteDiagnostic {
    let help = if is_struct_field {
        format!(
            "Use this variable or prefix it with an underscore: `{}: _{}`.",
            name, name
        )
    } else {
        format!(
            "Use this variable or prefix it with an underscore: `_{}`.",
            name
        )
    };
    LisetteDiagnostic::warn("Unused variable")
        .with_lint_code("unused_variable")
        .with_span_label(span, "never used")
        .with_help(help)
}

pub fn unused_parameter(span: &Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused parameter")
        .with_lint_code("unused_param")
        .with_span_label(span, "never used")
        .with_help(format!(
            "Use this parameter or prefix it with an underscore: `_{}`.",
            name
        ))
}

pub fn unused_mut(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused `mut`")
        .with_lint_code("unnecessary_mut")
        .with_span_label(span, "declared as mutable")
        .with_help("Remove `mut` from the declaration if you do not need to mutate the variable")
}

pub fn written_but_not_read(span: &Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Variable assigned but never read")
        .with_lint_code("assigned_but_never_read")
        .with_span_label(span, format!("`{}` is assigned but never read", name))
        .with_help(
            "Read the variable after assigning it, or explicitly discard it with `let _ = ...`",
        )
}

pub fn dead_code(span: &Span, cause: DeadCodeCause) -> LisetteDiagnostic {
    let (code, msg) = match cause {
        DeadCodeCause::Return => ("dead_code_after_return", "Unreachable code after return"),
        DeadCodeCause::Break => ("dead_code_after_break", "Unreachable code after break"),
        DeadCodeCause::Continue => (
            "dead_code_after_continue",
            "Unreachable code after continue",
        ),
        DeadCodeCause::DivergingIf => (
            "dead_code_after_diverging_if",
            "Unreachable code after diverging if/else",
        ),
        DeadCodeCause::DivergingMatch => (
            "dead_code_after_diverging_match",
            "Unreachable code after diverging match",
        ),
        DeadCodeCause::InfiniteLoop => (
            "dead_code_after_infinite_loop",
            "Unreachable code after infinite loop",
        ),
        DeadCodeCause::DivergingCall => (
            "dead_code_after_diverging_call",
            "Unreachable code after diverging function call",
        ),
    };
    LisetteDiagnostic::warn(msg)
        .with_lint_code(code)
        .with_span_label(span, "unreachable from this point onward")
        .with_help("Remove this line and all code after it")
}

pub fn pattern_issue(span: &Span, kind: IssueKind) -> LisetteDiagnostic {
    let (code, message, label, help) = match kind {
        IssueKind::RedundantLetElse => (
            "redundant_let_else",
            "Redundant `else` in `let...else`",
            "always matches",
            "Remove the `else` block since the pattern cannot fail",
        ),
        IssueKind::RedundantIfLet => (
            "redundant_if_let",
            "Redundant `if let` pattern",
            "always matches",
            "Use `let` instead of `if let` since the pattern cannot fail",
        ),
        IssueKind::UnreachableIfLetElse => (
            "unreachable_if_let_else",
            "Unreachable `else` branch",
            "this branch can never execute",
            "Remove the `else` branch since the pattern always matches",
        ),
        IssueKind::RedundantIfLetElse => (
            "redundant_if_let_else",
            "Redundant `else` branch",
            "this branch does nothing",
            "Remove the `else` branch",
        ),
    };

    LisetteDiagnostic::warn(message)
        .with_lint_code(code)
        .with_span_label(span, label)
        .with_help(help)
}

pub fn discarded_result_in_tail(span: &Span, return_type: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("`Result` is silently discarded")
        .with_lint_code("unused_result")
        .with_span_label(span, "failure will go unnoticed")
        .with_help(format!(
            "Handle this `Result` with `?` or `match`, explicitly discard it with `let _ = ...`, or return it by adding `-> {}` to the function signature",
            return_type
        ))
}

pub fn discarded_option_in_tail(span: &Span, return_type: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused Option")
        .with_lint_code("unused_option")
        .with_span_label(span, "this `Option` is discarded")
        .with_help(format!(
            "Handle this `Option`, explicitly discard it with `let _ = ...`, or return it by adding `-> {}` to the function signature",
            return_type
        ))
}

pub fn discarded_partial_in_tail(span: &Span, return_type: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("`Partial` is silently discarded")
        .with_lint_code("unused_partial")
        .with_span_label(span, "partial result will go unnoticed")
        .with_help(format!(
            "Handle this `Partial` with `match`, explicitly discard it with `let _ = ...`, or return it by adding `-> {}` to the function signature",
            return_type
        ))
}

pub fn unused_expression(span: &Span, kind: UnusedExpressionKind) -> LisetteDiagnostic {
    let (code, msg, label, help) = match kind {
        UnusedExpressionKind::Literal => (
            "unused_literal",
            "Unused literal",
            "this literal has no effect",
            "Remove this literal",
        ),
        UnusedExpressionKind::Result => (
            "unused_result",
            "`Result` is silently discarded",
            "failure will go unnoticed",
            "Handle this `Result` with `?` or `match`, or explicitly discard it with `let _ = ...`",
        ),
        UnusedExpressionKind::Option => (
            "unused_option",
            "Unused Option",
            "this `Option` is discarded",
            "Handle this `Option`, or explicitly discard it with `let _ = ...`",
        ),
        UnusedExpressionKind::Partial => (
            "unused_partial",
            "`Partial` is silently discarded",
            "partial result will go unnoticed",
            "Handle this `Partial` with `match`, or explicitly discard it with `let _ = ...`",
        ),
        UnusedExpressionKind::Value => (
            "unused_value",
            "Unused expression value",
            "this value is discarded",
            "Use the value, or ignore with `let _ = ...`",
        ),
    };
    LisetteDiagnostic::warn(msg)
        .with_lint_code(code)
        .with_span_label(span, label)
        .with_help(help)
}

pub fn unnecessary_reference(span: &Span, name: Option<&str>) -> LisetteDiagnostic {
    let (label, help) = match name {
        Some(n) => (
            format!("`{}` is already a reference", n),
            format!("Remove the `&` operator from `{}`", n),
        ),
        None => (
            "value is already a reference".to_string(),
            "Remove the `&` operator".to_string(),
        ),
    };
    LisetteDiagnostic::warn("Unnecessary `&`")
        .with_lint_code("unnecessary_reference")
        .with_span_label(span, label)
        .with_help(help)
}

pub fn unused_type_parameter(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused type parameter")
        .with_lint_code("unused_type_param")
        .with_span_label(span, "never used")
        .with_help("Remove the unused type parameter or use it in the signature")
}

pub fn type_param_only_in_bound(span: &Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Type parameter only used in bound")
        .with_lint_code("type_param_only_in_bound")
        .with_span_label(
            span,
            format!("`{}` is only used inside another parameter's bound", name),
        )
        .with_help("Remove it, or use it in a parameter type, return type, or bound left-hand side")
}

pub fn ineffective_try_block(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Ineffective `try` block")
        .with_lint_code("try_block_no_success_path")
        .with_span_label(span, "always propagates")
        .with_help("A `try` block is effective only if the expression may succeed or fail")
}

pub fn replaceable_with_zero_fill(span: &Span, kept: &str, struct_name: &str) -> LisetteDiagnostic {
    let example = if kept.is_empty() {
        format!("`{} {{ .. }}`", struct_name)
    } else {
        format!("`{} {{ {}, .. }}`", struct_name, kept)
    };
    LisetteDiagnostic::warn("Replaceable with zero-fill spread")
        .with_lint_code("replaceable_with_zero_fill")
        .with_span_label(span, "has zero-valued fields")
        .with_help(format!(
            "Replace zero-valued fields with zero-fill spread: {}",
            example
        ))
}

pub fn double_negation(span: &Span, is_bool: bool) -> LisetteDiagnostic {
    let (code, msg) = if is_bool {
        ("double_bool_negation", "Double boolean negation")
    } else {
        ("double_int_negation", "Double numeric negation")
    };

    LisetteDiagnostic::warn(msg)
        .with_lint_code(code)
        .with_span_label(span, "accidental double negation")
        .with_help("Remove one of the negation operators")
}

pub fn tautological_comparison(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn("Tautological comparison")
        .with_lint_code("self_comparison")
        .with_span_label(span, "comparing to itself")
        .with_help(format!(
            "This condition is always {}. Did you mean to compare different values?",
            result
        ))
}

pub fn self_assignment(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Self-assignment")
        .with_lint_code("self_assignment")
        .with_span_label(span, "assigning to itself")
        .with_help("Correct this assignment")
}

pub fn duplicate_logical_operand(span: &Span, operand_text: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Duplicate logical operand")
        .with_lint_code("duplicate_logical_operand")
        .with_span_label(span, "accidental repetition")
        .with_help(format!("Simplify to `{operand_text}`"))
}

pub fn bool_literal_comparison(span: &Span, replacement: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Redundant comparison to boolean literal")
        .with_lint_code("bool_literal_comparison")
        .with_span_label(span, "needlessly verbose")
        .with_help(format!("Simplify to `{replacement}`"))
}

pub fn identical_if_branches(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Identical if-else branches")
        .with_lint_code("identical_if_branches")
        .with_span_label(span, "both branches are equivalent")
        .with_help("Remove the `if` and keep a single copy of the branch body")
}

pub fn empty_match_arm(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Empty match arm")
        .with_lint_code("empty_match_arm")
        .with_span_label(span, "forgotten stub?")
        .with_help("Return `()` to indicate an intentional no-op in a match arm")
}

pub fn discarded_lambda_value(span: &Span, body_ty: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Discarded lambda value")
        .with_lint_code("discarded_lambda_value")
        .with_span_label(span, format!("value of type `{}` is discarded", body_ty))
        .with_help(
            "The lambda signature requires `()`, which does not match the value it is returning. End the lambda body with `()` or bind the value with `let _ = expr`.",
        )
}

pub fn unnecessary_parens(span: &Span, keyword: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unnecessary parens")
        .with_lint_code("excess_parens_on_condition")
        .with_span_label(span, "remove parens")
        .with_help(format!(
            "Lisette does not require parens around `{}` conditions",
            keyword
        ))
}

pub fn match_on_literal(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Ineffective match")
        .with_lint_code("match_on_literal")
        .with_span_label(span, "already known")
        .with_help(
            "Matching on a literal is ineffective, because this always succeeds. Did you mean to match on a variable?",
        )
}

pub fn single_arm_match(span: &Span, pattern_suggestion: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Ineffective match")
        .with_lint_code("single_arm_match")
        .with_span_label(span, "should be `if let`")
        .with_help(format!(
            "A match with a single meaningful arm is ineffective. Use `if let {} = value {{ ... }}` instead.",
            pattern_suggestion
        ))
}

pub fn uninterpolated_fstring(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Uninterpolated f-string")
        .with_lint_code("uninterpolated_fstring")
        .with_span_label(span, "zero interpolations")
        .with_help("Remove the `f` prefix. A string without interpolations does not need to be a format string")
}

pub fn unnecessary_raw_string(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unnecessary raw string")
        .with_lint_code("unnecessary_raw_string")
        .with_span_label(span, "no backslashes")
        .with_help("Remove the `r` prefix. A string without backslashes does not need to be raw")
}

pub fn expression_only_fstring(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Expression-only f-string")
        .with_lint_code("expression_only_fstring")
        .with_span_label(span, "the entire f-string is an expression")
        .with_help("Use the expression directly. Wrapping it in an f-string adds no value")
}

pub fn rest_only_slice_pattern(span: &Span, help: impl Into<String>) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Ineffective pattern")
        .with_lint_code("rest_only_slice_pattern")
        .with_span_label(span, "always matches")
        .with_help(help)
}

pub fn miscased_pascal(span: &Span, code: &str, suggested_name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Miscased name")
        .with_lint_code(code)
        .with_span_label(span, "expected PascalCase")
        .with_help(format!("Rename to `{}`", suggested_name))
}

pub fn miscased_snake(span: &Span, code: &str, suggested_name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Miscased name")
        .with_lint_code(code)
        .with_span_label(span, "expected snake_case")
        .with_help(format!("Rename to `{}`", suggested_name))
}

pub fn miscased_screaming_snake(span: &Span, suggested_name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Miscased name")
        .with_lint_code("constant_not_screaming_snake_case")
        .with_span_label(span, "expected SCREAMING_SNAKE_CASE")
        .with_help(format!("Rename to `{}`", suggested_name))
}

pub fn unused_field(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused field")
        .with_lint_code("unused_struct_field")
        .with_span_label(span, "never read")
        .with_help("Use or remove this field")
}

pub fn unused_variant(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused variant")
        .with_lint_code("unused_enum_variant")
        .with_span_label(span, "never constructed or matched")
        .with_help("Use or remove this enum variant")
}

pub fn unused_import(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused import")
        .with_lint_code("unused_import")
        .with_span_label(span, "never used")
        .with_help("Use or remove this import")
}

pub fn unused_type(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused type")
        .with_lint_code("unused_type")
        .with_span_label(span, "never used")
        .with_help("Use or remove this type")
}

pub fn unused_function(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused function")
        .with_lint_code("unused_function")
        .with_span_label(span, "never called")
        .with_help("Call or remove this function")
}

pub fn unused_constant(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unused constant")
        .with_lint_code("unused_constant")
        .with_span_label(span, "never used")
        .with_help("Use or remove this constant")
}

pub fn private_type_in_public_api(
    span: Option<&Span>,
    private_type: &str,
    public_definition: &str,
) -> LisetteDiagnostic {
    let mut diagnostic = LisetteDiagnostic::warn(format!(
        "Private type `{}` in public API",
        private_type
    ))
    .with_lint_code("internal_type_leak")
    .with_help(format!(
        "`{}` is private but exposed by `{}`, which is public. Add `pub` to the private type or remove it from the public API",
        private_type, public_definition
    ));

    if let Some(s) = span {
        diagnostic = diagnostic.with_span_label(s, "private");
    }

    diagnostic
}

pub fn unknown_attribute(span: &Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unknown attribute")
        .with_lint_code("unknown_attribute")
        .with_span_label(span, "not recognized")
        .with_help(format!(
            "`{}` is not a recognized attribute. Known attributes: `#[json]`, `#[xml]`, `#[yaml]`, `#[toml]`, `#[db]`, `#[bson]`, `#[msgpack]`, `#[mapstructure]`, `#[tag]`",
            name
        ))
}

pub fn field_attribute_without_struct_attribute(
    field_span: &Span,
    attribute_name: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Orphan field attribute")
        .with_lint_code("orphan_field_attribute")
        .with_span_label(field_span, "field has attribute but struct does not")
        .with_help(format!(
            "Add `#[{}]` atop the struct definition to enable field-level attributes",
            attribute_name
        ))
}

pub fn duplicate_tag_key(span: &Span, key: &str, first_span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Duplicate tag")
        .with_lint_code("duplicate_tag")
        .with_span_label(span, "duplicate")
        .with_span_label(first_span, "first occurrence")
        .with_help(format!(
            "Remove one of the `{}` attributes - each tag key may appear only once per field",
            key
        ))
}

pub fn conflicting_case_transforms(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::error("Conflicting case transforms")
        .with_lint_code("conflicting_case_transforms")
        .with_span_label(span, "conflicting")
        .with_help("Choose either `snake_case` or `camel_case`, not both")
}

pub fn tag_has_alias(span: &Span, key: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Prefer predefined tag alias")
        .with_lint_code("tag_has_alias")
        .with_span_label(span, "use alias instead")
        .with_help(format!(
            "Use `#[{}(...)]` instead of `#[tag(...)]` for better validation",
            key
        ))
}

pub fn unknown_tag_option(span: &Span, option: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Unknown tag option")
        .with_lint_code("unknown_tag_option")
        .with_span_label(span, "not recognized")
        .with_help(format!(
            "`{}` is not a recognized tag option. Known options: `snake_case`, `camel_case`, `omitempty`, `!omitempty`, `skip`, `string`",
            option
        ))
}
