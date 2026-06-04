use crate::passes::walk::NodeCtx;
use syntax::ast::{Expression, Literal};

const FILE_MODE_ID: &str = "go:io/fs.FileMode";
const PERM_MASK: u64 = 0o777;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::Literal {
        literal: Literal::Integer { value, text: None },
        ty,
        span,
    } = expression
        && *value > PERM_MASK
        && ty.get_qualified_id() == Some(FILE_MODE_ID)
    {
        ctx.sink
            .push(diagnostics::infer::decimal_file_mode(span, *value));
    }
}
