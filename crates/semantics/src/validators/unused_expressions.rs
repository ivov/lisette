use diagnostics::UnusedExpressionKind;
use syntax::ast::{Expression, Span, UnaryOperator};
use syntax::types::{Symbol, Type};

use crate::facts::{DiscardedTailKind, Facts};
use crate::store::Store;

pub(super) fn run(typed_ast: &[Expression], module_id: &str, store: &Store, facts: &mut Facts) {
    for item in typed_ast {
        visit_expression(item, None, module_id, store, facts);
    }
}

fn visit_expression(
    expression: &Expression,
    // `Some(ty)` when `expression` is a block whose last-item result is
    // passed through as the block's value and subsequently discarded —
    // e.g. a function body whose return type is ignored/unit/never, or a
    // block in statement position. `None` means the block's result is
    // consumed as a value, so the tail is not a discarded-tail candidate.
    discarded_block_ty: Option<&syntax::types::Type>,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    match expression {
        Expression::Block { items, ty, .. }
        | Expression::TryBlock { items, ty, .. }
        | Expression::RecoverBlock { items, ty, .. } => {
            let tail_is_discarded =
                discarded_block_ty.is_some() || ty.is_unit() || ty.is_ignored() || ty.is_never();
            visit_block_items(items, tail_is_discarded, module_id, store, facts);
        }
        Expression::Function {
            body, return_type, ..
        } => {
            let body_ty = body.get_type();
            let tail_is_discarded =
                return_type.is_unit() || body_ty.is_ignored() || body_ty.is_never();
            let discard = if tail_is_discarded {
                Some(return_type)
            } else {
                None
            };
            visit_expression(body, discard, module_id, store, facts);
            return;
        }
        Expression::Lambda { body, .. } => {
            let body_ty = body.get_type();
            let tail_is_discarded = body_ty.is_unit() || body_ty.is_ignored() || body_ty.is_never();
            let discard_anchor = body.get_type();
            let discard = if tail_is_discarded {
                Some(&discard_anchor)
            } else {
                None
            };
            visit_expression(body, discard, module_id, store, facts);
            return;
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, None, module_id, store, facts);
    }
}

fn visit_block_items(
    items: &[Expression],
    tail_is_discarded: bool,
    module_id: &str,
    store: &Store,
    facts: &mut Facts,
) {
    let len = items.len();
    for (i, item) in items.iter().enumerate() {
        let is_last = i == len - 1;
        let is_statement_only = is_statement_only(item);
        let suppress_unused_check = item.is_control_flow();

        if !is_statement_only && !suppress_unused_check && !is_last {
            let item_span = item.get_span();
            let is_literal = is_literal_or_negated_literal(item);
            let ty = item.get_type();

            let mut allowed_lints = callee_allowed_lints(item, module_id, store);
            if is_channel_send(item) {
                allowed_lints.push("unused_value".to_string());
            }

            emit_unused_expression(item_span, &ty, is_literal, &allowed_lints, facts);
        }

        if is_last
            && !is_statement_only
            && !suppress_unused_check
            && tail_is_discarded
            && let Some(call_return) = get_call_return_type(item)
        {
            let classification = if call_return.is_result() {
                Some(("unused_result", DiscardedTailKind::Result))
            } else if call_return.is_option() {
                Some(("unused_option", DiscardedTailKind::Option))
            } else if call_return.is_partial() {
                Some(("unused_partial", DiscardedTailKind::Partial))
            } else {
                None
            };

            if let Some((lint_name, kind)) = classification {
                let allowed_lints = callee_allowed_lints(item, module_id, store);
                if !allowed_lints.contains(&lint_name.to_string()) {
                    facts.add_discarded_tail(item.get_span(), kind, call_return.to_string());
                }
            }
        }
    }
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
        && !allowed_lints.contains(&kind.lint_name().to_string())
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
    match callee.get_type() {
        Type::Function { return_type, .. } => Some((*return_type).clone()),
        Type::Forall { body, .. } => match body.as_ref() {
            Type::Function { return_type, .. } => Some((**return_type).clone()),
            _ => None,
        },
        _ => None,
    }
}
