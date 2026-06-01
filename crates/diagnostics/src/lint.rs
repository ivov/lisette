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

    LisetteDiagnostic::info(message)
        .with_lint_code(code)
        .with_span_label(span, label)
        .with_help(help)
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
    LisetteDiagnostic::info("Unnecessary `&`")
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
    LisetteDiagnostic::info("Replaceable with zero-fill spread")
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

pub fn unsigned_comparison(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn(format!("Comparison is always {result}"))
        .with_lint_code("unsigned_comparison")
        .with_span_label(span, format!("always {result}"))
        .with_help(
            "An unsigned integer is never negative, so this comparison always has the same result. Did you mean to compare against a different value?",
        )
}

pub fn verbose_failure_propagation(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Verbose failure propagation")
        .with_lint_code("verbose_failure_propagation")
        .with_span_label(span, "verbose")
        .with_help("Use `?` to propagate the failure concisely")
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
    LisetteDiagnostic::info("Redundant comparison to boolean literal")
        .with_lint_code("bool_literal_comparison")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Simplify to `{replacement}`"))
}

pub fn loop_runs_once(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Loop runs at most once")
        .with_lint_code("loop_runs_once")
        .with_span_label(span, "the body always exits before looping back")
        .with_help(
            "The body always exits on the first iteration, so the loop never repeats. Make the exit conditional, or remove the loop.",
        )
}

pub fn unnecessary_return(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary `return`")
        .with_lint_code("unnecessary_return")
        .with_span_label(span, "redundant in tail position")
        .with_help("The final expression of a function is its return value. Drop `return` and keep the value")
}

pub fn identical_if_branches(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Identical if-else branches")
        .with_lint_code("identical_if_branches")
        .with_span_label(span, "both branches are equivalent")
        .with_help("Remove the `if` and keep a single copy of the branch body")
}

pub fn identical_match_arms(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Identical match arms")
        .with_lint_code("identical_match_arms")
        .with_span_label(span, "every arm is identical")
        .with_help(
            "All `match` arms resolve to the same value. Did you mean for the arms to differ?",
        )
}

pub fn unnecessary_bool(span: &Span, consequence_is_true: bool) -> LisetteDiagnostic {
    let help = if consequence_is_true {
        "Replace this `if... else` with the condition itself"
    } else {
        "Replace this `if... else` with the negated condition"
    };

    LisetteDiagnostic::info("Unnecessary boolean if-else")
        .with_lint_code("unnecessary_bool")
        .with_span_label(span, "can be simpler")
        .with_help(help)
}

pub fn redundant_pattern_matching(span: &Span, predicate: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant pattern matching")
        .with_lint_code("redundant_pattern_matching")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `.{predicate}()`"))
}

pub fn manual_map(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual map")
        .with_lint_code("manual_map")
        .with_span_label(span, "can be simpler")
        .with_help("Replace this `match` with `.map(...)`")
}

pub fn manual_unwrap_or(span: &Span, default_has_effects: bool) -> LisetteDiagnostic {
    let method = if default_has_effects {
        "unwrap_or_else"
    } else {
        "unwrap_or"
    };
    LisetteDiagnostic::info("Manual `unwrap_or`")
        .with_lint_code("manual_unwrap_or")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `.{method}(...)`"))
}

pub fn redundant_closure(span: &Span, callee: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant closure")
        .with_lint_code("redundant_closure")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this closure with `{callee}`"))
}

pub fn empty_match_arm(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Empty match arm")
        .with_lint_code("empty_match_arm")
        .with_span_label(span, "forgotten stub?")
        .with_help("Return `()` to indicate an intentional no-op in a match arm")
}

pub fn unnecessary_parens(span: &Span, keyword: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary parens")
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
    LisetteDiagnostic::info("Ineffective match")
        .with_lint_code("single_arm_match")
        .with_span_label(span, "should be `if let`")
        .with_help(format!(
            "A match with a single meaningful arm is ineffective. Use `if let {} = value {{ ... }}` instead.",
            pattern_suggestion
        ))
}

pub fn uninterpolated_fstring(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Uninterpolated f-string")
        .with_lint_code("uninterpolated_fstring")
        .with_span_label(span, "zero interpolations")
        .with_help("Remove the `f` prefix. A string without interpolations does not need to be a format string")
}

pub fn unnecessary_raw_string(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary raw string")
        .with_lint_code("unnecessary_raw_string")
        .with_span_label(span, "no backslashes")
        .with_help("Remove the `r` prefix. A string without backslashes does not need to be raw")
}

pub fn invisible_in_string(
    span: &Span,
    codepoint: u32,
    name: &str,
    is_bidi: bool,
) -> LisetteDiagnostic {
    let (title, code, help) = if is_bidi {
        (
            "Bidirectional character in string",
            "bidi_in_string",
            "Bidirectional control characters can reorder surrounding text and enable source-spoofing attacks. If intentional, write it as a `\\u` escape so it is visible in source; otherwise remove it.",
        )
    } else {
        (
            "Invisible character in string",
            "invisible_in_string",
            "Invisible characters in strings can hide bugs and silently shift meaning. Remove the character, or replace it with the visible character you meant.",
        )
    };
    LisetteDiagnostic::warn(title)
        .with_lint_code(code)
        .with_span_label(span, format!("contains U+{codepoint:04X} ({name})"))
        .with_help(help)
}

pub fn expression_only_fstring(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Expression-only f-string")
        .with_lint_code("expression_only_fstring")
        .with_span_label(span, "the entire f-string is an expression")
        .with_help("Use the expression directly. Wrapping it in an f-string adds no value")
}

pub fn rest_only_slice_pattern(span: &Span, help: impl Into<String>) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Ineffective pattern")
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
    LisetteDiagnostic::error("Miscased name")
        .with_infer_code("constant_not_screaming_snake_case")
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

pub fn tag_has_alias(span: &Span, key: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Prefer predefined tag alias")
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

pub fn trim_charset_misuse(span: &Span, function: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn(format!("Misuse of `{function}`"))
        .with_lint_code("trim_charset_misuse")
        .with_span_label(span, "treated as charset")
        .with_help(format!(
            "`strings.{function}` removes a set of characters, not a substring. Did you mean `strings.TrimPrefix` or `strings.TrimSuffix`?"
        ))
}

pub fn duplicate_arguments(span: &Span, module: &str, function: &str) -> LisetteDiagnostic {
    let display_module = module.strip_prefix("go:").unwrap_or(module);
    LisetteDiagnostic::warn("Duplicate arguments")
        .with_lint_code("duplicate_arguments")
        .with_span_label(span, "identical arguments")
        .with_help(format!(
            "Passing the same value twice to `{display_module}.{function}` makes this call a no-op. Did you mean to pass different values?"
        ))
}

pub fn lost_query_mutation(span: &Span, method: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Lost query mutation")
        .with_lint_code("lost_query_mutation")
        .with_span_label(span, "mutates a discarded copy")
        .with_help(format!(
            "`URL.Query` returns a fresh copy, so this `{method}` has no effect. Bind the copy returned by `Query()` to an identifier, mutate it, then assign `values.Encode()` back to the URL's `RawQuery` field."
        ))
}

pub fn waitgroup_add_in_task(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("`WaitGroup.Add` inside a `task`")
        .with_lint_code("waitgroup_add_in_task")
        .with_span_label(span, "may run after `Wait`")
        .with_help(
            "If `Wait` runs before this `Add`, it sees a zero counter and returns immediately. Call `Add` before the `task`",
        )
}

pub fn unnecessary_range_loop(span: &Span, collection: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary range loop")
        .with_lint_code("unnecessary_range_loop")
        .with_span_label(span, "can be simpler")
        .with_help(format!(
            "This loop exposes the index only to access elements of `{collection}`. Iterate directly over the elements with `for value in {collection}`"
        ))
}
