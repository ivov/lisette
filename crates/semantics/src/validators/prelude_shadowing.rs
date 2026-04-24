use diagnostics::LocalSink;
use syntax::ast::Expression;

use crate::store::Store;

pub(super) fn run(typed_ast: &[Expression], store: &Store, sink: &LocalSink) {
    let Some(prelude_module) = store.get_module("prelude") else {
        return;
    };
    for item in typed_ast {
        visit_expression(item, prelude_module, sink);
    }
}

fn visit_expression(
    expression: &Expression,
    prelude_module: &syntax::program::Module,
    sink: &LocalSink,
) {
    match expression {
        Expression::Enum {
            name, name_span, ..
        }
        | Expression::ValueEnum {
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
            let qualified = format!("prelude.{}", name);
            if prelude_module.definitions.contains_key(qualified.as_str()) {
                sink.push(diagnostics::infer::prelude_type_shadowed(name, *name_span));
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_expression(child, prelude_module, sink);
    }
}
