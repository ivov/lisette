use diagnostics::{LisetteDiagnostic, UnusedExpressionKind};
use syntax::ast::{Annotation, Expression, SelectArm, Span, UnaryOperator};
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::{Symbol, Type};

use diagnostics::infer::MismatchedTailKind;
use semantics::store::Store;

struct TailContext<'a> {
    expected_span: Span,
    expected_ty: &'a Type,
}

pub(crate) fn run(
    typed_ast: &[Expression],
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    for item in typed_ast {
        visit_expression(item, None, module_id, store, facts);
    }
}

fn visit_expression(
    expression: &Expression,
    tail_ctx: Option<&TailContext<'_>>,
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    match expression {
        Expression::Block { items, ty, .. }
        | Expression::TryBlock { items, ty, .. }
        | Expression::RecoverBlock { items, ty, .. } => {
            let tail_is_discarded = tail_ctx.is_some() || discards_value(ty);
            visit_block_items(items, tail_is_discarded, tail_ctx, module_id, store, facts);
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
            let body_ty = body.get_type();
            let ctx = tail_context_for_lambda(ty, *span, return_annotation, &body_ty);

            if ctx.is_some() && !matches!(body.as_ref(), Expression::Block { .. }) {
                descend_discarded(
                    body,
                    &DiscardMode::Tail(ctx.as_ref().into()),
                    module_id,
                    store,
                    facts,
                );
            }

            visit_expression(body, ctx.as_ref(), module_id, store, facts);
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
            visit_expression(pre_child, None, module_id, store, facts);
            visit_loop_body(body, module_id, store, facts);
            return;
        }
        Expression::Loop { body, .. } => {
            visit_loop_body(body, module_id, store, facts);
            return;
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, None, module_id, store, facts);
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
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let items = match body {
        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => items,
        _ => {
            visit_expression(body, None, module_id, store, facts);
            return;
        }
    };

    for item in items {
        if !is_statement_only(item) {
            descend_discarded(item, &DiscardMode::NonTail, module_id, store, facts);
        }
        visit_expression(item, None, module_id, store, facts);
    }
}

// Anchor inferred return types to the function body or lambda delimiter.
fn signature_marker_span(span: Span) -> Span {
    Span::new(span.file_id, span.byte_offset, 1)
}

fn visit_block_items(
    items: &[Expression],
    tail_is_discarded: bool,
    tail_ctx: Option<&TailContext<'_>>,
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let len = items.len();
    for (i, item) in items.iter().enumerate() {
        let is_last = i == len - 1;
        let is_statement_only = is_statement_only(item);

        if !is_statement_only && !is_last {
            descend_discarded(item, &DiscardMode::NonTail, module_id, store, facts);
        }

        if is_last && !is_statement_only && tail_is_discarded {
            descend_discarded(
                item,
                &DiscardMode::Tail(tail_ctx.into()),
                module_id,
                store,
                facts,
            );
        }
    }
}

enum DiscardMode<'a> {
    Tail(TailExpectation<'a>),
    NonTail,
}

#[derive(Clone, Copy)]
enum TailExpectation<'a> {
    Declared(&'a TailContext<'a>),
    Inferred,
}

impl<'a> From<Option<&'a TailContext<'a>>> for TailExpectation<'a> {
    fn from(context: Option<&'a TailContext<'a>>) -> Self {
        match context {
            Some(context) => Self::Declared(context),
            None => Self::Inferred,
        }
    }
}

fn descend_discarded(
    expression: &Expression,
    mode: &DiscardMode<'_>,
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
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
            if let Some(alternative) = alternative {
                descend_discarded(alternative, mode, module_id, store, facts);
            }
        }
        Expression::IfLet {
            consequence,
            alternative,
            ..
        } => {
            descend_discarded(consequence, mode, module_id, store, facts);
            if let Some(alternative) = alternative.expression() {
                descend_discarded(alternative, mode, module_id, store, facts);
            }
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                descend_discarded(&arm.expression, mode, module_id, store, facts);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive { body, .. }
                    | SelectArm::Send { body, .. }
                    | SelectArm::WildCard { body } => {
                        descend_discarded(body, mode, module_id, store, facts);
                    }
                    SelectArm::MatchReceive {
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
    facts: &mut Vec<LisetteDiagnostic>,
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

fn emit_unused_at_leaf(
    leaf: &Expression,
    module_id: &str,
    store: &Store,
    facts: &mut Vec<LisetteDiagnostic>,
) {
    let span = leaf.get_span();
    let is_literal = is_literal_or_negated_literal(leaf);
    let ty = leaf.get_type();
    let mut allowed_lints = callee_allowed_lints(leaf, module_id, store);
    if is_channel_send(leaf) {
        allowed_lints.push("unused_value".to_string());
    }
    if let Some(kind) = lvalue_slice_growth_kind(leaf) {
        if !allowed_lints.iter().any(|s| s == "unused_value") {
            facts.push(diagnostics::lint::unused_expression(&span, kind));
        }
        return;
    }
    emit_unused_expression(span, &ty, is_literal, &allowed_lints, facts);
}

fn lvalue_slice_growth_kind(expression: &Expression) -> Option<UnusedExpressionKind> {
    let Expression::Call {
        expression: callee,
        call_kind: CallKind::NativeMethod(NativeTypeKind::Slice),
        ..
    } = expression
    else {
        return None;
    };
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
    expectation: TailExpectation<'_>,
    module_id: &str,
    store: &Store,
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

    let allowed_lints = callee_allowed_lints(unwrapped, module_id, store);
    let alias = kind.allow_alias();
    if allowed_lints.iter().any(|s| s == alias) {
        return;
    }

    let (expected_span, expected_ty) = match expectation {
        TailExpectation::Declared(ctx) => (ctx.expected_span, ctx.expected_ty.to_string()),
        TailExpectation::Inferred => (item.get_span(), reported_ty.to_string()),
    };

    facts.push(diagnostics::infer::mismatched_tail_value(
        &item.get_span(),
        &reported_ty.to_string(),
        &expected_span,
        &expected_ty,
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
            | Expression::ModuleImport { .. }
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

fn callee_allowed_lints(expression: &Expression, module_id: &str, store: &Store) -> Vec<String> {
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
