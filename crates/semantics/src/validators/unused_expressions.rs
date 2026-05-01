use diagnostics::UnusedExpressionKind;
use syntax::ast::{Annotation, Expression, SelectArmPattern, Span, UnaryOperator};
use syntax::types::{Symbol, Type};

use crate::facts::Facts;
use crate::store::Store;
use diagnostics::infer::MismatchedTailKind;

struct TailContext<'a> {
    expected_span: Span,
    expected_ty: &'a Type,
}

pub(super) fn run(typed_ast: &[Expression], module_id: &str, store: &Store, facts: &mut Facts) {
    for item in typed_ast {
        visit_expression(item, None, module_id, store, facts);
    }
}

fn visit_expression(
    expression: &Expression,
    tail_ctx: Option<&TailContext<'_>>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    match expression {
        Expression::Block { items, ty, .. }
        | Expression::TryBlock { items, ty, .. }
        | Expression::RecoverBlock { items, ty, .. } => {
            let tail_is_discarded =
                tail_ctx.is_some() || ty.is_unit() || ty.is_ignored() || ty.is_never();
            visit_block_items(items, tail_is_discarded, tail_ctx, module_id, store, facts);
        }
        Expression::Function {
            body,
            return_type,
            return_annotation,
            ..
        } => {
            let is_implicit_return = matches!(return_annotation, Annotation::Unknown);
            let body_ty = body.get_type();
            let tail_is_discarded = is_implicit_return
                && (return_type.is_unit() || body_ty.is_ignored() || body_ty.is_never());
            let ctx = tail_is_discarded.then(|| TailContext {
                expected_span: signature_marker_span(body.get_span()),
                expected_ty: return_type,
            });
            visit_expression(body, ctx.as_ref(), module_id, store, facts);
            return;
        }
        Expression::Lambda {
            body,
            ty,
            span,
            return_annotation,
            ..
        } => {
            let is_implicit_return = matches!(return_annotation, Annotation::Unknown);
            let lambda_returns_unit =
                matches!(ty, Type::Function { return_type, .. } if return_type.is_unit());
            let body_ty = body.get_type();
            let tail_is_discarded = is_implicit_return
                && (lambda_returns_unit
                    || body_ty.is_unit()
                    || body_ty.is_ignored()
                    || body_ty.is_never());
            let lambda_return_ty: &Type = match ty {
                Type::Function { return_type, .. } => return_type,
                _ => &body_ty,
            };
            let ctx = tail_is_discarded.then(|| TailContext {
                expected_span: signature_marker_span(*span),
                expected_ty: lambda_return_ty,
            });

            if tail_is_discarded && !matches!(body.as_ref(), Expression::Block { .. }) {
                descend_discarded(
                    body,
                    &DiscardMode::Tail(ctx.as_ref()),
                    module_id,
                    store,
                    facts,
                );
            }

            visit_expression(body, ctx.as_ref(), module_id, store, facts);
            return;
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, None, module_id, store, facts);
    }
}

/// 1-byte span at the start of `span`. Anchors the "expected" label when
/// there is no explicit return-type annotation. Lands on the body's `{` for
/// functions and the leading `|` for lambdas.
fn signature_marker_span(span: Span) -> Span {
    Span::new(span.file_id, span.byte_offset, 1)
}

fn visit_block_items(
    items: &[Expression],
    tail_is_discarded: bool,
    tail_ctx: Option<&TailContext<'_>>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    let len = items.len();
    for (i, item) in items.iter().enumerate() {
        let is_last = i == len - 1;
        let is_statement_only = is_statement_only(item);

        if !is_statement_only && !is_last {
            descend_discarded(item, &DiscardMode::NonTail, module_id, store, facts);
        }

        if is_last && !is_statement_only && tail_is_discarded {
            descend_discarded(item, &DiscardMode::Tail(tail_ctx), module_id, store, facts);
        }
    }
}

/// Whether the descent is checking a tail-position discard (function/lambda
/// return value type-mismatch, hard error) or a non-tail discard (statement-
/// position unused expression, warning). Same structural walk; different
/// fact emitted at value leaves.
enum DiscardMode<'a> {
    Tail(Option<&'a TailContext<'a>>),
    NonTail,
}

fn descend_discarded(
    expression: &Expression,
    mode: &DiscardMode<'_>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    match expression.unwrap_parens() {
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            if let Some(last) = items.last()
                && !is_statement_only(last)
            {
                descend_discarded(last, mode, module_id, store, facts);
            }
        }
        Expression::If {
            consequence,
            alternative,
            ..
        } => {
            descend_discarded(consequence, mode, module_id, store, facts);
            descend_discarded(alternative, mode, module_id, store, facts);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                descend_discarded(&arm.expression, mode, module_id, store, facts);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match &arm.pattern {
                    SelectArmPattern::Receive { body, .. }
                    | SelectArmPattern::Send { body, .. }
                    | SelectArmPattern::WildCard { body } => {
                        descend_discarded(body, mode, module_id, store, facts);
                    }
                    SelectArmPattern::MatchReceive {
                        arms: match_arms, ..
                    } => {
                        for match_arm in match_arms {
                            descend_discarded(&match_arm.expression, mode, module_id, store, facts);
                        }
                    }
                }
            }
        }
        Expression::Loop { body, .. } => {
            descend_loop_break_values(body, mode, module_id, store, facts);
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
            DiscardMode::Tail(tail_ctx) => {
                check_discarded_tail(expression, *tail_ctx, module_id, store, facts)
            }
            DiscardMode::NonTail => emit_unused_at_leaf(unwrapped, module_id, store, facts),
        },
    }
}

fn descend_loop_break_values(
    expression: &Expression,
    mode: &DiscardMode<'_>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    match expression {
        Expression::Break {
            value: Some(value), ..
        } => {
            descend_discarded(value, mode, module_id, store, facts);
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
                descend_loop_break_values(child, mode, module_id, store, facts);
            }
        }
    }
}

fn emit_unused_at_leaf(leaf: &Expression, module_id: &str, store: &Store, facts: &mut Facts) {
    let span = leaf.get_span();
    let is_literal = is_literal_or_negated_literal(leaf);
    let ty = leaf.get_type();
    let mut allowed_lints = callee_allowed_lints(leaf, module_id, store);
    if is_channel_send(leaf) {
        allowed_lints.push("unused_value".to_string());
    }
    emit_unused_expression(span, &ty, is_literal, &allowed_lints, facts);
}

fn check_discarded_tail(
    item: &Expression,
    tail_ctx: Option<&TailContext<'_>>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
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
    {
        return;
    } else {
        MismatchedTailKind::Value
    };

    let allowed_lints = callee_allowed_lints(unwrapped, module_id, store);
    let alias = kind.allow_alias();
    if allowed_lints.iter().any(|s| s == alias) {
        return;
    }

    let (expected_span, expected_ty) = match tail_ctx {
        Some(ctx) => (ctx.expected_span, ctx.expected_ty.to_string()),
        None => (item.get_span(), reported_ty.to_string()),
    };

    facts.add_discarded_tail(
        item.get_span(),
        reported_ty.to_string(),
        expected_span,
        expected_ty,
    );
}

fn emit_unused_expression(
    span: Span,
    ty: &Type,
    is_literal: bool,
    allowed_lints: &[String],
    facts: &mut Facts,
) {
    let kind = if is_literal {
        Some(UnusedExpressionKind::Literal)
    } else if ty.is_result() {
        Some(UnusedExpressionKind::Result)
    } else if ty.is_option() {
        Some(UnusedExpressionKind::Option)
    } else if ty.is_partial() {
        Some(UnusedExpressionKind::Partial)
    } else if !ty.is_unit() && !ty.is_variable() && !ty.is_never() {
        Some(UnusedExpressionKind::Value)
    } else {
        None
    };

    if let Some(kind) = kind
        && !allowed_lints.iter().any(|s| s == kind.lint_name())
    {
        facts.add_unused_expression(span, kind);
    }
}

fn is_statement_only(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Let { .. }
            | Expression::Assignment { .. }
            | Expression::Defer { .. }
            | Expression::Task { .. }
            | Expression::While { .. }
            | Expression::WhileLet { .. }
            | Expression::For { .. }
            | Expression::Struct { .. }
            | Expression::Enum { .. }
            | Expression::ValueEnum { .. }
            | Expression::TypeAlias { .. }
            | Expression::Interface { .. }
            | Expression::Function { .. }
            | Expression::ImplBlock { .. }
            | Expression::Const { .. }
            | Expression::VariableDeclaration { .. }
            | Expression::ModuleImport { .. }
            | Expression::RawGo { .. }
            | Expression::NoOp
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

fn callee_allowed_lints(expression: &Expression, module_id: &str, store: &Store) -> Vec<String> {
    let Expression::Call {
        expression: callee, ..
    } = expression
    else {
        return vec![];
    };

    if let Expression::Identifier {
        value, qualified, ..
    } = callee.as_ref()
    {
        if let Some(q) = qualified
            && let Some(definition) = store.get_definition(q.as_str())
        {
            return definition.allowed_lints().to_vec();
        }
        let qualified_guess = if value.contains('.') {
            value.to_string()
        } else {
            Symbol::from_parts(module_id, value).to_string()
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
        if let Type::Nominal { id, .. } = &receiver_ty {
            let method_key = id.with_segment(member);
            if let Some(definition) = store.get_definition(&method_key) {
                return definition.allowed_lints().to_vec();
            }
        }
        if let Some(module) = receiver.get_type().as_import_namespace() {
            let method_key = Symbol::from_parts(module, member);
            if let Some(definition) = store.get_definition(&method_key) {
                return definition.allowed_lints().to_vec();
            }
        }
    }

    vec![]
}

fn is_channel_send(expression: &Expression) -> bool {
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
    let resolved = receiver.get_type().strip_refs();
    matches!(resolved.get_name(), Some("Channel" | "Sender"))
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
