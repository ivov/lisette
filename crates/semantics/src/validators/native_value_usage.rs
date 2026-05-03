use diagnostics::LocalSink;
use syntax::ast::{Expression, Span, StructKind};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{Symbol, Type, unqualified_name};

use crate::store::Store;

pub(super) fn run(typed_ast: &[Expression], module_id: &str, store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, false, false, module_id, store, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    is_callee: bool,
    is_dot_access_base: bool,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    if let Expression::Identifier {
        value, ty, span, ..
    } = expression
        && !is_callee
    {
        check_one(value, ty, *span, is_dot_access_base, module_id, store, sink);
    }

    match expression {
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => {
            visit_expression(callee, true, false, module_id, store, sink);
            for arg in args {
                visit_expression(arg, false, false, module_id, store, sink);
            }
            if let Some(s) = spread.as_ref() {
                visit_expression(s, false, false, module_id, store, sink);
            }
        }
        Expression::Paren {
            expression: inner, ..
        } => {
            visit_expression(inner, is_callee, is_dot_access_base, module_id, store, sink);
        }
        Expression::DotAccess {
            expression: inner, ..
        } => {
            visit_expression(inner, false, true, module_id, store, sink);
        }
        _ => {
            for child in expression.children() {
                visit_expression(child, false, false, module_id, store, sink);
            }
        }
    }
}

fn check_one(
    value: &str,
    ty: &Type,
    span: Span,
    is_dot_access_base: bool,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    if matches!(
        value,
        "imaginary" | "assert_type" | "complex" | "real" | "panic"
    ) {
        let qualified = Symbol::from_parts(module_id, value);
        if store.get_definition(&qualified).is_none() {
            sink.push(diagnostics::infer::native_constructor_value(value, span));
            return;
        }
    }

    {
        let qualified = if value.contains('.') {
            value.to_string()
        } else {
            Symbol::from_parts(module_id, value).to_string()
        };
        if resolves_to_struct_kind(&qualified, StructKind::Tuple, store) {
            sink.push(diagnostics::infer::native_constructor_value(value, span));
            return;
        }
        if !is_dot_access_base && resolves_to_struct_kind(&qualified, StructKind::Record, store) {
            sink.push(diagnostics::infer::record_struct_value(value, span));
            return;
        }
    }

    let Some((type_part, method_part)) = value.split_once('.') else {
        return;
    };
    if method_part.contains('.') {
        return;
    }

    let is_native = matches!(
        type_part,
        "Slice" | "EnumeratedSlice" | "Map" | "Channel" | "Sender" | "Receiver" | "string"
    );

    if is_native {
        if matches!(method_part, "new" | "buffered") {
            sink.push(diagnostics::infer::native_constructor_value(value, span));
        } else {
            sink.push(diagnostics::infer::native_method_value(method_part, span));
        }
        return;
    }

    if matches!(method_part, "new" | "buffered") {
        let ret_ty = match ty {
            Type::Function { return_type, .. } => Some(return_type.as_ref()),
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Function { return_type, .. } => Some(return_type.as_ref()),
                _ => None,
            },
            _ => None,
        };
        if let Some(ret) = ret_ty {
            let is_native_ret = matches!(ret.get_name(), Some("Channel" | "Map" | "Slice"));
            if is_native_ret {
                sink.push(diagnostics::infer::native_constructor_value(value, span));
                return;
            }
        }
    }

    let is_fn = matches!(ty, Type::Function { .. } | Type::Forall { .. });
    if !is_fn {
        return;
    }
    let fn_params = match ty {
        Type::Function { params, .. } => params.as_slice(),
        Type::Forall { body, .. } => match body.as_ref() {
            Type::Function { params, .. } => params.as_slice(),
            _ => return,
        },
        _ => return,
    };
    let Some(first) = fn_params.first() else {
        return;
    };
    let stripped = first.strip_refs();
    let is_self = matches!(&stripped, Type::Nominal { id, .. }
        if unqualified_name(id) == type_part);
    if !is_self {
        return;
    }

    let method_key = format!("{}.{}.{}", module_id, type_part, method_part);
    let is_public = store
        .get_definition(&method_key)
        .map(|d| d.visibility().is_public())
        .unwrap_or(true);

    if !is_public {
        sink.push(diagnostics::infer::private_method_expression(span));
    }
}

fn resolves_to_struct_kind(qualified: &str, kind: StructKind, store: &Store) -> bool {
    if let Some(k) = store.struct_kind(qualified) {
        return k == kind;
    }
    match store.get_definition(qualified) {
        Some(Definition {
            ty: alias_ty,
            body: DefinitionBody::TypeAlias { .. },
            ..
        }) => {
            if let Type::Nominal { id, .. } = alias_ty.unwrap_forall() {
                store.struct_kind(id) == Some(kind)
            } else {
                false
            }
        }
        _ => false,
    }
}
