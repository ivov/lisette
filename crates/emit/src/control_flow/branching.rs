use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use syntax::ast::Expression;

impl Planner<'_> {
    pub(crate) fn emit_block(
        &mut self,
        output: &mut String,
        expression: &Expression,
        fx: &mut EmitEffects,
    ) {
        let body = self.lower_block_as_body(expression, fx);
        Renderer.render_lowered_block(output, &body);
    }
}

pub(crate) fn wrap_if_struct_literal(condition: String) -> String {
    if condition.contains('{') {
        format!("({})", condition)
    } else {
        condition
    }
}
