use diagnostics::LocalSink;
use syntax::ast::{Expression, StructKind};
use syntax::program::{Definition, DefinitionBody, NativeTypeKind};
use syntax::types::{FunctionType, Symbol, Type, unqualified_name};

use semantics::store::Store;

pub(crate) fn run(typed_ast: &[Expression], module_id: &str, store: &Store, sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, Position::Value, module_id, store, sink);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Value,
    Callee,
    DotAccessBase,
}

fn visit_expression(
    expression: &Expression,
    position: Position,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    if matches!(expression, Expression::Identifier { .. }) && position != Position::Callee {
        check_one(expression, position, module_id, store, sink);
    }

    match expression {
        Expression::Call {
            expression: callee,
            args,
            spread,
            ..
        } => {
            visit_expression(callee, Position::Callee, module_id, store, sink);
            for arg in args {
                visit_expression(arg, Position::Value, module_id, store, sink);
            }
            if let Some(s) = spread.as_ref() {
                visit_expression(s, Position::Value, module_id, store, sink);
            }
        }
        Expression::Paren {
            expression: inner, ..
        } => {
            visit_expression(inner, position, module_id, store, sink);
        }
        Expression::DotAccess {
            expression: inner, ..
        } => {
            visit_expression(inner, Position::DotAccessBase, module_id, store, sink);
        }
        _ => {
            for child in expression.children() {
                visit_expression(child, Position::Value, module_id, store, sink);
            }
        }
    }
}

fn check_one(
    identifier: &Expression,
    position: Position,
    module_id: &str,
    store: &Store,
    sink: &LocalSink,
) {
    let Expression::Identifier {
        value, ty, span, ..
    } = identifier
    else {
        return;
    };
    let value = value.as_str();
    let span = *span;
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
        if position != Position::DotAccessBase
            && resolves_to_struct_kind(&qualified, StructKind::Record, store)
        {
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
        "Slice"
            | "EnumeratedSlice"
            | "Map"
            | "Channel"
            | "Sender"
            | "Receiver"
            | "string"
            | "Array"
    );

    if is_native {
        if NativeTypeKind::is_constructor_method(method_part) {
            sink.push(diagnostics::infer::native_constructor_value(value, span));
        } else {
            sink.push(diagnostics::infer::native_method_value(
                method_part,
                diagnostics::infer::NativeMethodForm::Static,
                span,
            ));
        }
        return;
    }

    if NativeTypeKind::is_constructor_method(method_part)
        && !is_user_type(type_part, module_id, store)
    {
        let ret_ty = fn_signature(ty).map(|f| f.return_type.as_ref());
        if let Some(ret) = ret_ty {
            let is_native_ret = matches!(ret.get_name(), Some("Channel" | "Map" | "Slice"));
            if is_native_ret {
                sink.push(diagnostics::infer::native_constructor_value(value, span));
                return;
            }
        }
    }

    let Some(signature) = fn_signature(ty) else {
        return;
    };
    let Some(first) = signature.params.first() else {
        return;
    };
    let stripped = first.ty.strip_refs();
    let is_self = matches!(&stripped, Type::Nominal { id, .. }
        if unqualified_name(id) == type_part);
    if !is_self {
        return;
    }

    let method_key = format!("{}.{}.{}", module_id, type_part, method_part);
    let is_public = store
        .get_definition(&method_key)
        .map(|d| d.visibility.is_public())
        .unwrap_or(true);

    if !is_public {
        sink.push(diagnostics::infer::private_method_expression(span));
    }
}

fn fn_signature(ty: &Type) -> Option<&FunctionType> {
    match ty {
        Type::Function(f) => Some(f),
        Type::Forall { body, .. } => match body.as_ref() {
            Type::Function(f) => Some(f),
            _ => None,
        },
        _ => None,
    }
}

fn is_user_type(type_part: &str, module_id: &str, store: &Store) -> bool {
    let qualified = if type_part.contains('.') {
        type_part.to_string()
    } else {
        Symbol::from_parts(module_id, type_part).to_string()
    };
    matches!(
        store.get_definition(&qualified).map(|d| &d.body),
        Some(DefinitionBody::Struct { .. } | DefinitionBody::Enum { .. })
    )
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
        }) => store.deep_struct_kind(alias_ty.unwrap_forall()) == Some(kind),
        _ => false,
    }
}
