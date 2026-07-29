use rustc_hash::FxHashMap as HashMap;
use std::borrow::Cow;
use syntax::ast::{Expression, Literal, Span};
use syntax::lex::{rune_codepoint, string_bytes};
use syntax::program::{CallKind, NativeTypeKind};

use crate::passes::walk::NodeCtx;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    let Some(entries) = map_from_entries(expression) else {
        return;
    };

    let mut seen: HashMap<KeyValue, Span> = HashMap::default();
    for entry in entries {
        let Some(Expression::Literal { literal, span, .. }) =
            entry_key(entry).map(Expression::unwrap_parens)
        else {
            continue;
        };
        let Some(key) = key_value(literal) else {
            continue;
        };
        let first = *seen.entry(key).or_insert(*span);
        if first != *span {
            ctx.sink
                .push(diagnostics::infer::duplicate_map_keys(first, *span));
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
enum KeyValue<'a> {
    Boolean(bool),
    Integer(u64),
    Rune(u32),
    Text(Cow<'a, [u8]>),
}

fn map_from_entries(expression: &Expression) -> Option<&[Expression]> {
    let Expression::Call {
        expression: callee,
        args,
        spread: None,
        call_kind: CallKind::NativeMethodIdentifier(NativeTypeKind::Map),
        ty,
        ..
    } = expression
    else {
        return None;
    };

    let Expression::Identifier { value, .. } = callee.unwrap_parens() else {
        return None;
    };
    if value != "Map.from" || !ty.is_map() {
        return None;
    }

    let [argument] = args.as_slice() else {
        return None;
    };
    let Expression::Literal {
        literal: Literal::Slice(entries),
        ..
    } = argument.unwrap_parens()
    else {
        return None;
    };

    entries
        .iter()
        .all(|entry| entry_key(entry).is_some())
        .then_some(entries.as_slice())
}

fn entry_key(entry: &Expression) -> Option<&Expression> {
    let Expression::Tuple { elements, .. } = entry.unwrap_parens() else {
        return None;
    };
    let [key, _value] = elements.as_slice() else {
        return None;
    };
    Some(key)
}

fn key_value(literal: &Literal) -> Option<KeyValue<'_>> {
    match literal {
        Literal::Boolean(value) => Some(KeyValue::Boolean(*value)),
        Literal::Integer { value, .. } => Some(KeyValue::Integer(*value)),
        Literal::Char(text) => rune_codepoint(text).map(KeyValue::Rune),
        Literal::String { value, raw } => string_bytes(value, *raw).map(KeyValue::Text),
        _ => None,
    }
}
