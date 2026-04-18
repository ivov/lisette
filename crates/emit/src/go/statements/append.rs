use crate::Emitter;
use crate::go::utils::Staged;
use syntax::ast::Expression;

impl Emitter<'_> {
    pub(crate) fn is_slice_append_or_extend(&self, func: &Expression) -> bool {
        if let Expression::DotAccess {
            expression, member, ..
        } = func
            && (member == "append" || member == "extend")
        {
            return expression.get_type().resolve().has_name("Slice");
        }
        false
    }

    pub(crate) fn emit_append_args(
        &mut self,
        output: &mut String,
        args: &[Expression],
        spread: Option<&Expression>,
        is_extend: bool,
    ) -> String {
        let stages: Vec<Staged> = args.iter().map(|a| self.stage_composite(a)).collect();
        let mut emitted_args = self.sequence(output, stages, "_arg");
        self.append_spread_arg(output, &mut emitted_args, spread);
        let args_str = emitted_args.join(", ");
        let suffix = if is_extend { "..." } else { "" };
        format!("{}{}", args_str, suffix)
    }
}
