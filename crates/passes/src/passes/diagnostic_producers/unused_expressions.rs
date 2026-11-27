use diagnostics::{LisetteDiagnostic, UnusedExpressionKind};
use syntax::ast::{Annotation, Expression, MatchArm, SelectArm, Span, UnaryOperator};
use syntax::program::{CallOperation, NativeTypeKind};
use syntax::types::{CompoundKind, Symbol, Type};

use diagnostics::infer::MismatchedTailKind;
use semantics::facts::Selections;
use semantics::store::Store;

struct TailContext<'a> {
    expected_span: Span,
    expected_ty: &'a Type,
}

/// What every visit in one file reads.
struct FileContext<'a> {
    package_id: &'a str,
    store: &'a Store,
    selections: &'a Selections,
}

pub(crate) fn run(
    typed_ast: &[Expression],
    package_id: &str,
    store: &Store,
    selections: &Selections,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let file = FileContext {
        package_id,
        store,
        selections,
    };
    for item in typed_ast {
        visit_expression(item, TailUse::Kept, &file, facts);
    }
}

/// Who reports a block's tail value.
#[derive(Clone, Copy)]
enum TailUse<'a> {
    /// Measured against a declared return type.
    Declared(&'a TailContext<'a>),
    /// Flows out, so reported only if the block's own type drops it.
    Kept,
    /// Dropped whatever its type, and unreported so far.
    Dropped,
    /// Already reported by an enclosing discarded walk.
    Walked,
}

fn visit_expression(
    expression: &Expression,
    tail_use: TailUse<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Block { items, ty, .. }
        | Expression::TryBlock { items, ty, .. }
        | Expression::RecoverBlock { items, ty, .. } => {
            visit_block_items(items, ty, tail_use, file, facts);
            return;
        }
        Expression::Function {
            body,
            return_type,
            return_annotation,
            ..
        } => {
            let Some(body) = body.definition() else {
                return;
            };
            let ctx = tail_context_for_function(body, return_type, return_annotation);
            visit_expression(body, declared_or_kept(&ctx), file, facts);
            return;
        }
        Expression::Lambda {
            body,
            ty,
            span,
            return_annotation,
            ..
        } => {
            let body_ty = body.get_type();
            let ctx = tail_context_for_lambda(ty, *span, return_annotation, &body_ty);

            let body_use = match &ctx {
                // A block reports its own tail, so descending here would duplicate.
                Some(ctx) if matches!(body.as_ref(), Expression::Block { .. }) => {
                    TailUse::Declared(ctx)
                }
                Some(ctx) => {
                    descend_discarded(body, &DiscardMode::Tail(ctx), file, facts);
                    TailUse::Walked
                }
                None => TailUse::Kept,
            };

            visit_expression(body, body_use, file, facts);
            return;
        }
        Expression::For {
            iterable: pre_child,
            body,
            ..
        }
        | Expression::While {
            condition: pre_child,
            body,
            ..
        }
        | Expression::WhileLet {
            scrutinee: pre_child,
            body,
            ..
        } => {
            visit_expression(pre_child, TailUse::Kept, file, facts);
            visit_loop_body(body, tail_use, file, facts);
            return;
        }
        Expression::Loop { body, .. } => {
            visit_loop_body(body, tail_use, file, facts);
            return;
        }
        // Walked by `visit_break_values` instead, under the loop's use.
        Expression::Break { .. } => return,
        Expression::Defer {
            expression: wrapped,
            ..
        }
        | Expression::Task {
            expression: wrapped,
            ..
        } => {
            visit_discarded_expression(wrapped, file, facts);
            return;
        }
        // `descend_discarded` unwraps parens, so a walked spine passes through.
        Expression::Paren {
            expression: inner, ..
        } => {
            visit_expression(inner, tail_use, file, facts);
            return;
        }
        // Branches carry the value and are what `descend_discarded` walks.
        // Subjects, conditions and guards are inputs nothing has walked.
        Expression::If {
            condition,
            consequence,
            alternative,
            ty,
            ..
        } => {
            visit_expression(condition, TailUse::Kept, file, facts);
            let branch_use = branch_use(tail_use, ty);
            visit_expression(consequence, branch_use, file, facts);
            if let Some(alternative) = alternative {
                visit_expression(alternative, branch_use, file, facts);
            }
            return;
        }
        Expression::IfLet {
            scrutinee,
            consequence,
            alternative,
            ty,
            ..
        } => {
            visit_expression(scrutinee, TailUse::Kept, file, facts);
            let branch_use = branch_use(tail_use, ty);
            visit_expression(consequence, branch_use, file, facts);
            if let Some(alternative) = alternative.expression() {
                visit_expression(alternative, branch_use, file, facts);
            }
            return;
        }
        Expression::Match { subject, arms, .. } => {
            visit_expression(subject, TailUse::Kept, file, facts);
            visit_match_arms(arms, tail_use, file, facts);
            return;
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive {
                        receive_expression,
                        body,
                        ..
                    }
                    | SelectArm::Send {
                        send_expression: receive_expression,
                        body,
                    } => {
                        visit_expression(receive_expression, TailUse::Kept, file, facts);
                        visit_expression(body, tail_use, file, facts);
                    }
                    SelectArm::MatchReceive {
                        receive_expression,
                        arms,
                    } => {
                        visit_expression(receive_expression, TailUse::Kept, file, facts);
                        visit_match_arms(arms, tail_use, file, facts);
                    }
                    SelectArm::WildCard { body, .. } => {
                        visit_expression(body, tail_use, file, facts);
                    }
                }
            }
            return;
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, TailUse::Kept, file, facts);
    }
}

fn visit_match_arms(
    arms: &[MatchArm],
    tail_use: TailUse<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    for arm in arms {
        if let Some(guard) = &arm.guard {
            visit_expression(guard, TailUse::Kept, file, facts);
        }
        visit_expression(&arm.expression, tail_use, file, facts);
    }
}

/// An `if` with no `else` yields unit while its branch still carries a value.
fn branch_use<'a>(tail_use: TailUse<'a>, conditional_ty: &Type) -> TailUse<'a> {
    match tail_use {
        TailUse::Kept if discards_value(conditional_ty) => TailUse::Dropped,
        other => other,
    }
}

fn declared_or_kept<'a>(ctx: &'a Option<TailContext<'a>>) -> TailUse<'a> {
    match ctx {
        Some(ctx) => TailUse::Declared(ctx),
        None => TailUse::Kept,
    }
}

fn discards_value(ty: &Type) -> bool {
    ty.is_unit() || ty.is_ignored() || ty.is_never()
}

fn tail_context_for_function<'a>(
    body: &Expression,
    return_type: &'a Type,
    return_annotation: &Annotation,
) -> Option<TailContext<'a>> {
    let is_implicit_return = matches!(return_annotation, Annotation::Unknown);
    let body_ty = body.get_type();
    // Deliberately tests return_type.is_unit(), not body_ty.is_unit(), unlike the lambda case.
    let tail_is_discarded =
        is_implicit_return && (return_type.is_unit() || body_ty.is_ignored() || body_ty.is_never());
    tail_is_discarded.then(|| TailContext {
        expected_span: signature_marker_span(body.get_span()),
        expected_ty: return_type,
    })
}

fn tail_context_for_lambda<'a>(
    ty: &'a Type,
    span: Span,
    return_annotation: &Annotation,
    body_ty: &'a Type,
) -> Option<TailContext<'a>> {
    let is_implicit_return = matches!(return_annotation, Annotation::Unknown);
    let lambda_returns_unit = matches!(ty, Type::Function(f) if f.return_type.is_unit());
    let tail_is_discarded = is_implicit_return && (lambda_returns_unit || discards_value(body_ty));
    let lambda_return_ty: &'a Type = match ty {
        Type::Function(f) => &f.return_type,
        _ => body_ty,
    };
    tail_is_discarded.then(|| TailContext {
        expected_span: signature_marker_span(span),
        expected_ty: lambda_return_ty,
    })
}

fn visit_loop_body(
    body: &Expression,
    loop_use: TailUse<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    visit_break_values(body, loop_use, file, facts);

    match body {
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            visit_discarded_items(items, file, facts);
        }
        _ => visit_expression(body, TailUse::Kept, file, facts),
    }
}

/// A `break` value belongs to the loop, not the body spine that leads to it.
fn visit_break_values(
    expression: &Expression,
    loop_use: TailUse<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Break {
            value: Some(value), ..
        } => {
            visit_expression(value, loop_use, file, facts);
        }
        Expression::Loop { .. }
        | Expression::While { .. }
        | Expression::WhileLet { .. }
        | Expression::For { .. }
        | Expression::Function { .. }
        | Expression::Lambda { .. }
        | Expression::Task { .. }
        | Expression::Defer { .. } => {}
        _ => {
            for child in expression.children() {
                visit_break_values(child, loop_use, file, facts);
            }
        }
    }
}

/// Unlike a loop body, a bare wrapped expression is itself a discarded value.
fn visit_discarded_expression(
    expression: &Expression,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            visit_discarded_items(items, file, facts);
        }
        _ => {
            descend_discarded(expression, &DiscardMode::NonTail, file, facts);
            visit_expression(expression, TailUse::Walked, file, facts);
        }
    }
}

fn visit_discarded_items(
    items: &[Expression],
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    for item in items {
        let walked = if is_statement_only(item) {
            false
        } else {
            descend_discarded(item, &DiscardMode::NonTail, file, facts);
            true
        };

        let item_use = if walked {
            TailUse::Walked
        } else {
            TailUse::Kept
        };
        visit_expression(item, item_use, file, facts);
    }
}

// Anchor inferred return types to the function body or lambda delimiter.
fn signature_marker_span(span: Span) -> Span {
    Span::new(span.file_id, span.byte_offset, 1)
}

fn visit_block_items(
    items: &[Expression],
    block_ty: &Type,
    tail_use: TailUse<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let len = items.len();
    for (i, item) in items.iter().enumerate() {
        let walked = if is_statement_only(item) {
            false
        } else if i != len - 1 {
            descend_discarded(item, &DiscardMode::NonTail, file, facts);
            true
        } else {
            match tail_use {
                TailUse::Declared(ctx) => {
                    descend_discarded(item, &DiscardMode::Tail(ctx), file, facts);
                    true
                }
                TailUse::Dropped => {
                    descend_discarded(item, &DiscardMode::NonTail, file, facts);
                    true
                }
                TailUse::Kept if discards_value(block_ty) => {
                    descend_discarded(item, &DiscardMode::NonTail, file, facts);
                    true
                }
                TailUse::Kept => false,
                TailUse::Walked => true,
            }
        };

        let item_use = if walked {
            TailUse::Walked
        } else {
            TailUse::Kept
        };
        visit_expression(item, item_use, file, facts);
    }
}

enum DiscardMode<'a> {
    Tail(&'a TailContext<'a>),
    NonTail,
}

fn descend_discarded(
    expression: &Expression,
    mode: &DiscardMode<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression.unwrap_parens() {
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            if let Some(last) = items.last()
                && !is_statement_only(last)
            {
                descend_discarded(last, mode, file, facts);
            }
        }
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            descend_discarded(consequence, mode, file, facts);
            if let Some(alternative) = alternative {
                descend_discarded(alternative, mode, file, facts);
            }
        }
        Expression::IfLet {
            consequence,
            alternative,
            ..
        } => {
            descend_discarded(consequence, mode, file, facts);
            if let Some(alternative) = alternative.expression() {
                descend_discarded(alternative, mode, file, facts);
            }
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                descend_discarded(&arm.expression, mode, file, facts);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive { body, .. }
                    | SelectArm::Send { body, .. }
                    | SelectArm::WildCard { body, .. } => {
                        descend_discarded(body, mode, file, facts);
                    }
                    SelectArm::MatchReceive {
                        arms: match_arms, ..
                    } => {
                        for match_arm in match_arms {
                            descend_discarded(&match_arm.expression, mode, file, facts);
                        }
                    }
                }
            }
        }
        Expression::Loop { body, .. } => {
            descend_loop_break_values(body, mode, file, facts);
        }
        Expression::Let { .. }
        | Expression::Const { .. }
        | Expression::Assignment { .. }
        | Expression::Return { .. }
        | Expression::Break { .. }
        | Expression::Continue { .. }
        | Expression::Defer { .. }
        | Expression::Task { .. }
        | Expression::While { .. }
        | Expression::WhileLet { .. }
        | Expression::For { .. } => {}
        unwrapped => match mode {
            DiscardMode::Tail(tail_ctx) => check_discarded_tail(expression, tail_ctx, file, facts),
            DiscardMode::NonTail => emit_unused_at_leaf(unwrapped, file, facts),
        },
    }
}

fn descend_loop_break_values(
    expression: &Expression,
    mode: &DiscardMode<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Break {
            value: Some(value), ..
        } => {
            descend_discarded(value, mode, file, facts);
        }
        Expression::Loop { .. }
        | Expression::While { .. }
        | Expression::WhileLet { .. }
        | Expression::For { .. }
        | Expression::Function { .. }
        | Expression::Lambda { .. }
        | Expression::Task { .. }
        | Expression::Defer { .. } => {}
        _ => {
            for child in expression.children() {
                descend_loop_break_values(child, mode, file, facts);
            }
        }
    }
}

fn emit_unused_at_leaf(leaf: &Expression, file: &FileContext, facts: &mut Vec<LisetteDiagnostic>) {
    let span = leaf.get_span();
    if let Expression::Lambda { params, .. } = leaf {
        let kind = if params.is_empty() {
            UnusedExpressionKind::ParameterlessLambda
        } else {
            UnusedExpressionKind::Lambda
        };
        facts.push(diagnostics::lint::unused_expression(&span, kind));
        return;
    }
    let is_literal = is_literal_or_negated_literal(leaf);
    let ty = leaf.get_type();
    let mut allowed_lints = callee_allowed_lints(leaf, file.package_id, file.store);
    if is_channel_send(leaf, file.store) {
        allowed_lints.push("unused_value".to_string());
    }
    if let Some(kind) = lvalue_slice_growth_kind(leaf, file.selections) {
        if !allowed_lints.iter().any(|s| s == "unused_value") {
            facts.push(diagnostics::lint::unused_expression(&span, kind));
        }
        return;
    }
    emit_unused_expression(span, &ty, is_literal, &allowed_lints, facts);
}

fn lvalue_slice_growth_kind(
    expression: &Expression,
    selections: &Selections,
) -> Option<UnusedExpressionKind> {
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return None;
    };
    if !matches!(
        selections.call_operation(expression),
        Some(CallOperation::Native {
            kind: NativeTypeKind::Slice,
            ..
        })
    ) {
        return None;
    }
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.as_ref()
    else {
        return None;
    };
    let kind = match member.as_str() {
        "append" => UnusedExpressionKind::SliceGrow,
        "reserve" => UnusedExpressionKind::SliceReserve,
        _ => return None,
    };
    receiver.get_var_name().map(|_| kind)
}

fn check_discarded_tail(
    item: &Expression,
    expected: &TailContext<'_>,
    file: &FileContext,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let unwrapped = item.unwrap_parens();
    let reported_ty = get_call_return_type(unwrapped).unwrap_or_else(|| unwrapped.get_type());

    let kind = if reported_ty.is_result() {
        MismatchedTailKind::Result
    } else if reported_ty.is_option() {
        MismatchedTailKind::Option
    } else if reported_ty.is_partial() {
        MismatchedTailKind::Partial
    } else if reported_ty.is_unit()
        || reported_ty.is_ignored()
        || reported_ty.is_never()
        || reported_ty.is_variable()
        || reported_ty.is_error()
    {
        return;
    } else {
        MismatchedTailKind::Value
    };

    let allowed_lints = callee_allowed_lints(unwrapped, file.package_id, file.store);
    let alias = kind.allow_alias();
    if allowed_lints.iter().any(|s| s == alias) {
        return;
    }

    facts.push(diagnostics::infer::mismatched_tail_value(
        &item.get_span(),
        &reported_ty.to_string(),
        &expected.expected_span,
        &expected.expected_ty.to_string(),
    ));
}

fn emit_unused_expression(
    span: Span,
    ty: &Type,
    is_literal: bool,
    allowed_lints: &[String],
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let kind = if is_literal {
        Some(UnusedExpressionKind::Literal)
    } else if ty.is_result() {
        Some(UnusedExpressionKind::Result)
    } else if ty.is_option() {
        Some(UnusedExpressionKind::Option)
    } else if ty.is_partial() {
        Some(UnusedExpressionKind::Partial)
    } else if !ty.is_unit()
        && !ty.is_variable()
        && !ty.is_placeholder()
        && !ty.is_never()
        && !ty.is_error()
    {
        Some(UnusedExpressionKind::Value)
    } else {
        None
    };

    if let Some(kind) = kind
        && !allowed_lints.iter().any(|s| s == kind.lint_name())
    {
        facts.push(diagnostics::lint::unused_expression(&span, kind));
    }
}

fn is_statement_only(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Let { .. }
            | Expression::Assignment { .. }
            | Expression::Defer { .. }
            | Expression::Task { .. }
            | Expression::Assert { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. }
            | Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::Function { .. }
            | Expression::ImplBlock { .. }
            | Expression::Const { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::PackageImport { .. }
            | Expression::RawGo { .. }
    )
}

fn is_literal_or_negated_literal(expression: &Expression) -> bool {
    match expression {
        Expression::Literal { .. } => true,
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => matches!(expression.as_ref(), Expression::Literal { .. }),
        _ => false,
    }
}

fn callee_allowed_lints(expression: &Expression, package_id: &str, store: &Store) -> Vec<String> {
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return vec![];
    };

    if let Expression::Identifier {
        value, resolution, ..
    } = callee.as_ref()
    {
        if let Some(q) = resolution.definition()
            && let Some(definition) = store.get_definition(q)
        {
            return definition.allowed_lints().to_vec();
        }
        if let Some(method) = resolution.method()
            && let Some(method) = store.get_method(&method.owner, &method.name)
        {
            return method.allowed_lints.clone();
        }
        let qualified_guess = if value.contains('.') {
            value.to_string()
        } else {
            Symbol::from_parts(package_id, value).to_string()
        };
        if let Some(definition) = store.get_definition(&qualified_guess) {
            return definition.allowed_lints().to_vec();
        }
    }

    if let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.as_ref()
    {
        let receiver_ty = receiver.get_type().strip_refs();
        if let Type::Nominal { id, .. } = &receiver_ty
            && let Some(method) = store.get_method(id, member)
        {
            return method.allowed_lints.clone();
        }
        if let Some(package) = receiver.get_type().as_import_namespace() {
            let method_key = Symbol::from_parts(package, member);
            if let Some(definition) = store.get_definition(&method_key) {
                return definition.allowed_lints().to_vec();
            }
        }
    }

    vec![]
}

fn is_channel_send(expression: &Expression, store: &Store) -> bool {
    let Expression::Call {
        expression: callee,
        args,
        ..
    } = expression
    else {
        return false;
    };
    let Expression::DotAccess {
        expression: receiver,
        member,
        ..
    } = callee.as_ref()
    else {
        return false;
    };
    if member != "send" || args.len() != 1 {
        return false;
    }
    let resolved = store.peeled(&receiver.get_type().strip_refs());
    matches!(
        resolved.compound_kind(),
        Some(CompoundKind::Channel | CompoundKind::Sender)
    )
}

fn get_call_return_type(expression: &Expression) -> Option<Type> {
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return None;
    };
    callee
        .get_type()
        .unwrap_forall()
        .get_function_ret()
        .cloned()
}
