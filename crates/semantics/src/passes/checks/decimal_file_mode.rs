use diagnostics::LocalSink;
use syntax::ast::{Expression, Literal};

const FILE_MODE_ID: &str = "go:io/fs.FileMode";
const PERM_MASK: u64 = 0o777;

pub(crate) fn run(typed_ast: &[Expression], sink: &LocalSink) {
    for item in typed_ast {
        visit_expression(item, sink);
    }
}

fn visit_expression(expression: &Expression, sink: &LocalSink) {
    if let Expression::Literal {
        literal: Literal::Integer { value, text: None },
        ty,
        span,
    } = expression
        && *value > PERM_MASK
        && ty.get_qualified_id() == Some(FILE_MODE_ID)
    {
        sink.push(diagnostics::infer::decimal_file_mode(span, *value));
    }

    for child in expression.children() {
        visit_expression(child, sink);
    }
}
