use crate::output::finish::cleanup::drop_unreachable_statements;
use crate::output::finish::verify::{BodyResult, verify_final_function_body};
use crate::target::ir::bodies::LoweredBlock;
use crate::target::ir::go_expression::{FunctionLiteralLayout, GoParameter};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

pub(crate) fn function_literal(
    parameters: Vec<GoParameter>,
    result: Option<GoType>,
    mut body: LoweredBlock,
    layout: FunctionLiteralLayout,
) -> GoExpression {
    drop_unreachable_statements(&mut body.statements);
    if result.is_some() {
        body.ensure_go_termination();
    }
    if cfg!(debug_assertions) {
        let body_result = if result.is_some() {
            BodyResult::Value
        } else {
            BodyResult::Void
        };
        verify_final_function_body(&mut body, body_result)
            .unwrap_or_else(|error| panic!("{error}"));
    }
    GoExpression::unchecked_function_literal(parameters, result, body, layout)
}

pub(crate) fn immediate_call(
    result: Option<GoType>,
    body: LoweredBlock,
    layout: FunctionLiteralLayout,
) -> GoExpression {
    GoExpression::call(
        function_literal(Vec::new(), result, body, layout),
        Vec::new(),
    )
}
