use crate::output::finish::cleanup::drop_unreachable_statements;
use crate::output::finish::verify::verify_final_function_body;
use crate::target::ir::bodies::LoweredBlock;
use crate::target::ir::go_expression::{FunctionLiteralLayout, GoParameter};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

pub(crate) fn function_literal(
    parameters: Vec<GoParameter>,
    result: impl Into<GoType>,
    mut body: LoweredBlock,
    layout: FunctionLiteralLayout,
) -> GoExpression {
    let result = result.into();
    drop_unreachable_statements(&mut body.statements);
    if !result.is_empty() {
        body.ensure_go_termination();
    }
    if cfg!(debug_assertions) {
        verify_final_function_body(&mut body, !result.is_empty())
            .unwrap_or_else(|error| panic!("{error}"));
    }
    GoExpression::unchecked_function_literal(parameters, result, body, layout)
}

pub(crate) fn immediate_call(
    result: impl Into<GoType>,
    body: LoweredBlock,
    layout: FunctionLiteralLayout,
) -> GoExpression {
    GoExpression::call(
        function_literal(Vec::new(), result, body, layout),
        Vec::new(),
    )
}
