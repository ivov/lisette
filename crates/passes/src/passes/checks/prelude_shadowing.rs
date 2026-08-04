use diagnostics::LocalSink;
use syntax::ast::Expression;

use semantics::store::Store;

pub(crate) fn run(typed_ast: &[Expression], store: &Store, sink: &LocalSink) {
    let Some(prelude_package) = store.get_package("prelude") else {
        return;
    };
    for item in typed_ast {
        check_top_level_function(item, prelude_package, sink);
        visit_expression(item, prelude_package, sink);
    }
}

fn check_top_level_function(
    item: &Expression,
    prelude_package: &syntax::program::Package,
    sink: &LocalSink,
) {
    if let Expression::Function {
        name, name_span, ..
    } = item
    {
        let qualified = format!("prelude.{}", name);
        if prelude_package.definitions.contains_key(qualified.as_str()) {
            sink.push(diagnostics::infer::prelude_function_shadowed(
                name, *name_span,
            ));
        }
    }
}

fn visit_expression(
    expression: &Expression,
    prelude_package: &syntax::program::Package,
    sink: &LocalSink,
) {
    match expression {
        Expression::Enum {
            name, name_span, ..
        }
        | Expression::Struct {
            name, name_span, ..
        }
        | Expression::TypeAlias {
            name, name_span, ..
        }
        | Expression::Interface {
            name, name_span, ..
        } => {
            // Prelude-defined types (Slice, Map, …) plus inline builtins like
            // `Array` that have no prelude definition but are still reserved.
            let qualified = format!("prelude.{}", name);
            if prelude_package.definitions.contains_key(qualified.as_str())
                || syntax::program::NativeTypeKind::from_name(name).is_some()
            {
                sink.push(diagnostics::infer::prelude_type_shadowed(name, *name_span));
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, prelude_package, sink);
    }
}
