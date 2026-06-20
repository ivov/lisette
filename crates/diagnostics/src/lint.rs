use crate::LisetteDiagnostic;
use syntax::ast::{DeadCodeCause, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    RedundantLetElse,
    RedundantLetAssert,
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
        IssueKind::RedundantLetAssert => (
            "redundant_let_assert",
            "Redundant `assert` in `let assert`",
            "always matches",
            "Use a plain `let` since the pattern cannot fail",
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

pub fn negated_equality(span: &Span, is_equal: bool) -> LisetteDiagnostic {
    let (from, to) = if is_equal {
        ("!(a == b)", "a != b")
    } else {
        ("!(a != b)", "a == b")
    };

    LisetteDiagnostic::info("Negated equality comparison")
        .with_lint_code("negated_equality")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Rewrite `{from}` as `{to}`"))
}

pub fn tautological_comparison(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn("Tautological comparison")
        .with_lint_code("self_comparison")
        .with_span_label(span, "comparing to itself")
        .with_help(format!(
            "This condition is always `{}`. Did you mean to compare different values?",
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

pub fn type_limit_comparison(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn(format!("Comparison is always {result}"))
        .with_lint_code("type_limit_comparison")
        .with_span_label(span, format!("always `{result}`"))
        .with_help(format!(
            "This compares against the limit of the value's type, so this comparison is always `{result}`. Did you mean to compare against a different value?"
        ))
}

pub fn redundant_comparison(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant comparison")
        .with_lint_code("redundant_comparison")
        .with_span_label(span, "redundant")
        .with_help(
            "This comparison is already implied by the other, so the expression is equivalent to the other side alone",
        )
}

pub fn double_comparison(span: &Span, combined: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Comparisons can be combined")
        .with_lint_code("double_comparison")
        .with_span_label(span, format!("simplify to `{combined}`"))
        .with_help(format!(
            "These two comparisons cover the same operands, so they are equivalent to a single `{combined}`."
        ))
}

pub fn bad_bit_mask(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let (result, clause) = if always_true {
        ("true", "always satisfy")
    } else {
        ("false", "unable to satisfy")
    };

    LisetteDiagnostic::warn("Incompatible bit mask")
        .with_lint_code("bad_bit_mask")
        .with_span_label(span, format!("always `{result}`"))
        .with_help(format!(
            "The mask makes this value {clause} the comparison, so it is always `{result}`. Check the mask or the constant."
        ))
}

pub fn ineffective_bit_mask(
    span: &Span,
    mask_operator: &str,
    mask: i128,
    constant: i128,
) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Ineffective bit mask")
        .with_lint_code("ineffective_bit_mask")
        .with_span_label(span, "mask has no effect")
        .with_help(format!(
            "`{mask_operator} {mask}` does not change the result of comparing with `{constant}`, so the mask can be removed."
        ))
}

pub fn equal_operands(span: &Span, note: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Equal operands")
        .with_lint_code("equal_operands")
        .with_span_label(span, "identical operands")
        .with_help(format!(
            "Both operands are identical so the result {note}. Did you mean to use different operands?"
        ))
}

pub fn float_cmp(span: &Span, is_equal: bool) -> LisetteDiagnostic {
    let operator = if is_equal { "==" } else { "!=" };

    LisetteDiagnostic::warn("Exact float comparison")
        .with_lint_code("float_cmp")
        .with_span_label(span, format!("floats compared with `{operator}`"))
        .with_help(
            "Floating-point results are rarely bit-exact, so `==` and `!=` may not behave as intended. Compare within a tolerance instead, e.g. `math.Abs(a - b) < c`.",
        )
}

pub fn float_equality_without_abs(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Float equality without `abs`")
        .with_lint_code("float_equality_without_abs")
        .with_span_label(span, "difference is not wrapped in `math.Abs`")
        .with_help(
            "Because `a - b` is signed, this is also `true` whenever `a` is far below `b`, not only when `a` and `b` are close, so it wrongly accepts values that are nowhere near equal. Compare the magnitude instead: `math.Abs(a - b) < c`.",
        )
}

pub fn non_negative_comparison(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn(format!("Comparison is always {result}"))
        .with_lint_code("non_negative_comparison")
        .with_span_label(span, format!("always {result}"))
        .with_help(
            "A length is never negative, so this comparison always has the same result. Did you mean to compare against a different value?",
        )
}

pub fn goos_goarch_comparison(
    span: &Span,
    always_true: bool,
    const_name: &str,
    kind: &str,
    examples: &str,
) -> LisetteDiagnostic {
    let result = if always_true { "true" } else { "false" };

    LisetteDiagnostic::warn(format!("Comparison is always {result}"))
        .with_lint_code("goos_goarch_comparison")
        .with_span_label(span, format!("always {result}"))
        .with_help(format!(
            "`runtime.{const_name}` only ever holds a known {kind}, and this is not one. Did you mean a valid value such as {examples}?"
        ))
}

pub fn redundant_operation(span: &Span, always: Option<&str>) -> LisetteDiagnostic {
    match always {
        Some(value) => LisetteDiagnostic::info(format!("Operation always evaluates to `{value}`"))
            .with_lint_code("redundant_operation")
            .with_span_label(span, format!("always `{value}`"))
            .with_help(format!("Simplify this operation to `{value}`")),
        None => LisetteDiagnostic::info("Operation has no effect")
            .with_lint_code("redundant_operation")
            .with_span_label(span, "has no effect")
            .with_help("Simplify this operation to its other operand"),
    }
}

pub fn unnecessary_min_or_max(span: &Span, op: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info(format!("Unnecessary `{op}` call"))
        .with_lint_code("unnecessary_min_or_max")
        .with_span_label(span, "always returns the same operand")
        .with_help(format!(
            "This `{op}` always evaluates to one of its operands, so it has no effect. Simplify it to that operand."
        ))
}

pub fn integer_division_to_zero(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Integer division is always `0`")
        .with_lint_code("integer_division_to_zero")
        .with_span_label(span, "always `0`")
        .with_help(
            "Dividing these integer literals truncates to `0` because the numerator is smaller in magnitude than the denominator. Did you mean floating-point division?",
        )
}

pub fn verbose_failure_propagation(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Verbose failure propagation")
        .with_lint_code("verbose_failure_propagation")
        .with_span_label(span, "verbose")
        .with_help("Use `?` to propagate the failure concisely")
}

pub fn almost_swapped(span: &Span, first: &str, second: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Variables are not swapped")
        .with_lint_code("almost_swapped")
        .with_span_label(span, "does not swap the values")
        .with_help(format!(
            "`{first} = {second}` overwrites `{first}`, so the following `{second} = {first}` writes `{second}`'s own value back and the original `{first}` is lost. To swap them, save one value in a temporary variable first."
        ))
}

pub fn self_assignment(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Self-assignment")
        .with_lint_code("self_assignment")
        .with_span_label(span, "assigning to itself")
        .with_help("Correct this assignment")
}

pub fn manual_compound_assignment(span: &Span, symbol: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual compound assignment")
        .with_lint_code("manual_compound_assignment")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Use the `{symbol}` compound assignment operator"))
}

pub fn misrefactored_assign_op(
    span: &Span,
    target: &str,
    compound: &str,
    other: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::warn(format!("Compound assignment repeats `{target}`"))
        .with_lint_code("misrefactored_assign_op")
        .with_span_label(span, format!("uses `{target}` twice"))
        .with_help(format!(
            "The `{compound}` operator already includes `{target}`, so naming `{target}` again on the right applies it twice. To apply `{other}` once, write `{target} {compound} {other}`."
        ))
}

pub fn neg_multiply(span: &Span, operand: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Multiplication by `-1`")
        .with_lint_code("neg_multiply")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Use the negation `-{operand}` instead"))
}

pub fn regexp_in_loop(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Regexp recompiled on every iteration")
        .with_lint_code("regexp_in_loop")
        .with_span_label(span, "compiled each time through the loop")
        .with_help(
            "Compile the pattern once outside the loop and reuse it: `regexp.MustCompile` for a known-valid pattern, or `regexp.Compile` to keep handling the error",
        )
}

pub fn manual_is_empty(span: &Span, replacement: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Length comparison can use `is_empty()`")
        .with_lint_code("manual_is_empty")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Simplify to `{replacement}`"))
}

pub fn manual_find(span: &Span, receiver: &str, predicate: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `find`")
        .with_lint_code("manual_find")
        .with_span_label(span, "can use `find`")
        .with_help(format!(
            "`filter(...).get(0)` builds the whole filtered slice. Use `{receiver}.find({predicate})` to return the first match directly"
        ))
}

pub fn manual_contains(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `contains`")
        .with_lint_code("manual_contains")
        .with_span_label(span, "can be simpler")
        .with_help("Replace `.any(|x| x == value)` with `.contains(value)`")
}

pub fn unnecessary_first_then_check(span: &Span, replacement: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("First-element check can use `is_empty()`")
        .with_lint_code("unnecessary_first_then_check")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Simplify to `{replacement}`"))
}

pub fn redundant_slice_bounds(span: &Span, replacement: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant slice bounds")
        .with_lint_code("redundant_slice_bounds")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Simplify to `{replacement}`"))
}

pub fn duplicate_logical_operand(span: &Span, operand_text: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Duplicate logical operand")
        .with_lint_code("duplicate_logical_operand")
        .with_span_label(span, "accidental repetition")
        .with_help(format!("Simplify to `{operand_text}`"))
}

pub fn negated_logical_operand(span: &Span, always_true: bool) -> LisetteDiagnostic {
    let (message, label, result, reason) = if always_true {
        (
            "Tautological logical operands",
            "always `true`",
            "true",
            "an operand or its negation always holds",
        )
    } else {
        (
            "Contradictory logical operands",
            "always `false`",
            "false",
            "an operand and its negation cannot both hold",
        )
    };

    LisetteDiagnostic::warn(message)
        .with_lint_code("negated_logical_operand")
        .with_span_label(span, label)
        .with_help(format!("Replace with `{result}`, since {reason}"))
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

pub fn collapsible_if(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Collapsible `if`")
        .with_lint_code("collapsible_if")
        .with_span_label(span, "can be merged into the outer `if`")
        .with_help("Merge this nested `if` into the outer condition with `&&`")
}

pub fn collapsible_match(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Collapsible `match`")
        .with_lint_code("collapsible_match")
        .with_span_label(span, "can be merged into the outer arm")
        .with_help(
            "Move the inner pattern into the outer arm's pattern to remove a level of nesting",
        )
}

pub fn collapsible_else_if(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Collapsible `else if`")
        .with_lint_code("collapsible_else_if")
        .with_span_label(span, "can join the `else`")
        .with_help("Remove the braces around this `if` and write it as `else if`")
}

pub fn redundant_else(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant `else`")
        .with_lint_code("redundant_else")
        .with_span_label(span, "unnecessary")
        .with_help(
            "The `if` branch always exits, so the `else` only adds nesting. Drop `else` and move its body to follow the `if`",
        )
}

pub fn needless_bool_assign(
    span: &Span,
    target: &str,
    condition: &str,
    negate: bool,
) -> LisetteDiagnostic {
    let help = if negate {
        format!("Replace the `if` with `{target} = !({condition})`")
    } else {
        format!("Replace the `if` with `{target} = {condition}`")
    };
    LisetteDiagnostic::info("Needless boolean assignment")
        .with_lint_code("needless_bool_assign")
        .with_span_label(span, "can be simpler")
        .with_help(help)
}

pub fn redundant_closure_call(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant closure call")
        .with_lint_code("redundant_closure_call")
        .with_span_label(span, "called immediately")
        .with_help("Drop the `(|| ...)()` wrapper and use its body directly")
}

pub fn single_element_loop(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Loop over a single element")
        .with_lint_code("single_element_loop")
        .with_span_label(span, "only one element")
        .with_help("The loop body runs once. Bind the element with `let` and remove the loop")
}

pub fn while_let_loop(span: &Span, pattern: &str, subject: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `while let`")
        .with_lint_code("while_let_loop")
        .with_span_label(span, "can be a `while let`")
        .with_help(format!(
            "Replace the `loop` and `match` with `while let {pattern} = {subject}`"
        ))
}

pub fn identical_match_arms(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Identical match arms")
        .with_lint_code("identical_match_arms")
        .with_span_label(span, "every arm is identical")
        .with_help(
            "All `match` arms resolve to the same value. Did you mean for the arms to differ?",
        )
}

pub fn match_same_arms(span: &Span, earlier: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Duplicate match arm")
        .with_lint_code("match_same_arms")
        .with_span_label(span, "same body as an earlier arm")
        .with_help(format!("Merge this arm into the `{earlier}` arm with `|`"))
}

pub fn redundant_guards(span: &Span, binding: &str, literal: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant guard")
        .with_lint_code("redundant_guards")
        .with_span_label(span, "redundant guard")
        .with_help(format!(
            "Replace `{binding}` with `{literal}` in the pattern and drop the guard"
        ))
}

pub fn wildcard_in_or_patterns(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Wildcard in or-pattern")
        .with_lint_code("wildcard_in_or_patterns")
        .with_span_label(span, "can be just `_`")
        .with_help(
            "The `_` alternative already matches everything. Replace the whole pattern with `_`",
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

pub fn manual_unwrap_or(span: &Span, lazy_default: bool) -> LisetteDiagnostic {
    let method = if lazy_default {
        "unwrap_or_else"
    } else {
        "unwrap_or"
    };
    LisetteDiagnostic::info("Manual `unwrap_or`")
        .with_lint_code("manual_unwrap_or")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `.{method}(...)`"))
}

pub fn manual_map_or(span: &Span, lazy_default: bool) -> LisetteDiagnostic {
    let replacement = if lazy_default {
        ".map_or_else(...)"
    } else {
        ".map_or(...)"
    };
    LisetteDiagnostic::info("Manual `map_or`")
        .with_lint_code("manual_map_or")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `{replacement}`"))
}

pub fn manual_filter(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `filter`")
        .with_lint_code("manual_filter")
        .with_span_label(span, "can be simpler")
        .with_help("Replace this `match` with `.filter(...)`")
}

pub fn manual_ok_or(span: &Span, lazy: bool) -> LisetteDiagnostic {
    let method = if lazy { "ok_or_else" } else { "ok_or" };
    LisetteDiagnostic::info("Manual `ok_or`")
        .with_lint_code("manual_ok_or")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `.{method}(...)`"))
}

pub fn manual_ok_err(span: &Span, method: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info(format!("Manual `{method}`"))
        .with_lint_code("manual_ok_err")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace this `match` with `.{method}()`"))
}

pub fn needless_match(span: &Span, subject: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Needless `match`")
        .with_lint_code("needless_match")
        .with_span_label(span, "unnecessary")
        .with_help(format!(
            "Every arm rebuilds the subject unchanged. Replace this `match` with `{subject}`"
        ))
}

pub fn map_unwrap_or(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `map_or`")
        .with_lint_code("map_unwrap_or")
        .with_span_label(span, "can be simpler")
        .with_help("Replace `.map(f).unwrap_or(d)` with `.map_or(d, f)`")
}

pub fn bind_instead_of_map(span: &Span, wrapper: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `map`")
        .with_lint_code("bind_instead_of_map")
        .with_span_label(span, "can be simpler")
        .with_help(format!(
            "Replace `.and_then(|x| {wrapper}(y))` with `.map(|x| y)`"
        ))
}

pub fn map_flatten(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `and_then`")
        .with_lint_code("map_flatten")
        .with_span_label(span, "can be simpler")
        .with_help("Replace `.map(f).flatten()` with `.and_then(f)`")
}

pub fn map_identity(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant `map`")
        .with_lint_code("map_identity")
        .with_span_label(span, "does nothing")
        .with_help("Remove `.map(|x| x)`, which returns its input unchanged")
}

pub fn unnecessary_map_on_constructor(
    span: &Span,
    variant: &str,
    method: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::info(format!("Unnecessary `{method}`"))
        .with_lint_code("unnecessary_map_on_constructor")
        .with_span_label(span, "can be simpler")
        .with_help(format!(
            "Replace `{variant}(x).{method}(f)` with `{variant}(f(x))`"
        ))
}

pub fn map_or_none(span: &Span, replacement: &str) -> LisetteDiagnostic {
    let help = if replacement == "ok" {
        "Replace `.map_or(None, Some)` with `.ok()`".to_string()
    } else {
        format!("Replace `.map_or(None, f)` with `.{replacement}(f)`")
    };
    LisetteDiagnostic::info(format!("Manual `{replacement}`"))
        .with_lint_code("map_or_none")
        .with_span_label(span, "can be simpler")
        .with_help(help)
}

pub fn manual_option_zip(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `zip`")
        .with_lint_code("manual_option_zip")
        .with_span_label(span, "can be simpler")
        .with_help("Replace `a.and_then(|a| b.map(|b| (a, b)))` with `a.zip(b)`")
}

pub fn unnecessary_lazy_evaluations(span: &Span, lazy: &str, eager: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary lazy evaluation")
        .with_lint_code("unnecessary_lazy_evaluations")
        .with_span_label(span, "can be simpler")
        .with_help(format!(
            "Replace `.{lazy}(...)` with `.{eager}(...)` to pass the value directly"
        ))
}

pub fn or_fn_call(span: &Span, eager: &str, lazy: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Unnecessary eager evaluation")
        .with_lint_code("or_fn_call")
        .with_span_label(span, "always evaluated")
        .with_help(format!(
            "Replace `.{eager}(...)` with `.{lazy}(...)` so the fallback runs only when needed"
        ))
}

pub fn needless_question_mark(span: &Span, wrapper: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info(format!("Needless `?` in `{wrapper}`"))
        .with_lint_code("needless_question_mark")
        .with_span_label(span, "can be simpler")
        .with_help(format!("Replace `{wrapper}(x?)` with `x`"))
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

pub fn match_as_if_let(span: &Span, pattern_suggestion: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("`match` reducible to `if let`")
        .with_lint_code("match_as_if_let")
        .with_span_label(span, "can be simpler")
        .with_help(format!(
            "Replace this `match` with `if let {} = value {{ ... }}`",
            pattern_suggestion
        ))
}

pub fn equatable_if_let(span: &Span, pattern: &str, subject: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("`if let` used as an equality check")
        .with_lint_code("equatable_if_let")
        .with_span_label(span, "can be `==`")
        .with_help(format!(
            "The pattern binds nothing, so replace `if let {pattern} = {subject}` with `{subject} == {pattern}`",
        ))
}

pub fn single_arm_select(span: &Span, receive: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Single-arm `select`")
        .with_lint_code("single_arm_select")
        .with_span_label(span, "waits on a single operation")
        .with_help(format!(
            "A `select` with one arm makes no choice between channel operations. Use `match {receive} {{ ... }}` directly"
        ))
}

pub fn match_on_bool(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Match on boolean")
        .with_lint_code("match_on_bool")
        .with_span_label(span, "should be `if`")
        .with_help("A `match` on a boolean is better written as an `if` expression")
}

pub fn match_single_binding(span: &Span, binding: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Ineffective match")
        .with_lint_code("match_single_binding")
        .with_span_label(span, "should be `let`")
        .with_help(format!(
            "A match with a single binding is ineffective. Use `let {} = value` instead.",
            binding
        ))
}

pub fn let_and_return(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant binding before return")
        .with_lint_code("let_and_return")
        .with_span_label(span, "bound and immediately returned")
        .with_help("Return the value directly instead of binding it first")
}

pub fn redundant_rebinding(span: &Span, name: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant rebinding")
        .with_lint_code("redundant_rebinding")
        .with_span_label(span, format!("rebinds `{name}` to itself"))
        .with_help(format!(
            "Remove `let {name} = {name}`. The existing `{name}` already refers to this value, so rebinding it does nothing."
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

pub fn unknown_attribute(span: &Span, name: &str, known: &[&str]) -> LisetteDiagnostic {
    let known_list = known
        .iter()
        .map(|attribute| format!("`#[{attribute}]`"))
        .collect::<Vec<_>>()
        .join(", ");
    LisetteDiagnostic::warn("Unknown attribute")
        .with_lint_code("unknown_attribute")
        .with_span_label(span, "not recognized")
        .with_help(format!(
            "`{name}` is not a recognized attribute. Known attributes: {known_list}"
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

pub fn manual_equal_fold(
    span: &Span,
    negated: bool,
    namespace: &str,
    left_arg: &str,
    right_arg: &str,
) -> LisetteDiagnostic {
    let prefix = if negated { "!" } else { "" };
    LisetteDiagnostic::info("Inefficient comparison")
        .with_lint_code("manual_equal_fold")
        .with_span_label(span, "can use `strings.EqualFold`")
        .with_help(format!(
            "Use `{prefix}{namespace}.EqualFold({left_arg}, {right_arg})` to compare case-insensitively in one call"
        ))
}

pub fn manual_bytes_equal(
    span: &Span,
    negated: bool,
    namespace: &str,
    left_arg: &str,
    right_arg: &str,
) -> LisetteDiagnostic {
    let prefix = if negated { "!" } else { "" };
    LisetteDiagnostic::info("Manual `bytes.Equal`")
        .with_lint_code("manual_bytes_equal")
        .with_span_label(span, "can use `bytes.Equal`")
        .with_help(format!(
            "Use `{prefix}{namespace}.Equal({left_arg}, {right_arg})` to compare byte slices directly"
        ))
}

pub fn redundant_sprintf(span: &Span, namespace: &str, value: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Redundant `Sprintf`")
        .with_lint_code("redundant_sprintf")
        .with_span_label(span, "returns its argument unchanged")
        .with_help(format!(
            "`{namespace}.Sprintf(\"%s\", {value})` formats a string as itself. Use `{value}` directly"
        ))
}

pub fn manual_replace_all(
    span: &Span,
    namespace: &str,
    s: &str,
    old: &str,
    new: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `strings.ReplaceAll`")
        .with_lint_code("manual_replace_all")
        .with_span_label(span, "can use `strings.ReplaceAll`")
        .with_help(format!(
            "`{namespace}.Replace({s}, {old}, {new}, -1)` replaces every occurrence. Use `{namespace}.ReplaceAll({s}, {old}, {new})`"
        ))
}

pub fn manual_time_since(span: &Span, namespace: &str, arg: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `time.Since`")
        .with_lint_code("manual_time_since")
        .with_span_label(span, "can use `time.Since`")
        .with_help(format!(
            "`{namespace}.Since({arg})` is shorthand for `{namespace}.Now().Sub({arg})`"
        ))
}

pub fn manual_time_until(span: &Span, namespace: &str, receiver: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::info("Manual `time.Until`")
        .with_lint_code("manual_time_until")
        .with_span_label(span, "can use `time.Until`")
        .with_help(format!(
            "`{namespace}.Until({receiver})` is shorthand for `{receiver}.Sub({namespace}.Now())`"
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
            "Prefer `wg.Go(|| ...)`, which counts the task and starts it in one step and runs `Done` for you, or move `Add` before the `task`",
        )
}

pub fn deprecated_api(span: &Span, message: &str) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Use of deprecated API")
        .with_lint_code("deprecated")
        .with_span_label(span, "deprecated")
        .with_help(message)
}

pub fn lost_cancel(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Context leaking")
        .with_lint_code("lost_cancel")
        .with_span_label(span, "never called")
        .with_help(
            "Call this cancel function (usually `defer cancel()`) to release the context, or it leaks until the parent is canceled",
        )
}

pub fn exit_after_defer(span: &Span) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("`os.Exit` skips `defer`")
        .with_lint_code("exit_after_defer")
        .with_span_label(span, "exits before the `defer` above can run")
        .with_help(
            "`os.Exit` will terminate the process without running deferred calls. Run the cleanup before exiting instead of deferring it",
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

pub fn out_of_domain_value(
    span: &Span,
    type_display: &str,
    valid_display: &str,
) -> LisetteDiagnostic {
    LisetteDiagnostic::warn("Out-of-domain value")
        .with_lint_code("out_of_domain_value")
        .with_span_label(span, "out of domain")
        .with_help(format!(
            "`{type_display}` has a closed domain (`{valid_display}`) that excludes this value"
        ))
}
