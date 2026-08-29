use crate::passes::comparison::{expressions_equivalent, is_side_effect_free};
use crate::passes::walk::NodeCtx;
use syntax::ast::Expression;

pub(crate) fn check(expression: &Expression, ctx: &NodeCtx) {
    if let Expression::If {
        condition,
        alternative,
        ..
    } = expression
        && let Some(alternative) = alternative.as_deref()
        && let Expression::If {
            condition: next_condition,
            ..
        } = alternative
        && is_side_effect_free(condition)
        && is_side_effect_free(next_condition)
        && expressions_equivalent(condition, next_condition)
    {
        ctx.sink.push(diagnostics::infer::repeated_if_condition(
            &next_condition.get_span(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conditions(expressions: &[Expression]) -> (&Expression, &Expression) {
        let Expression::Function { body, .. } = &expressions[0] else {
            panic!("expected function")
        };
        let Expression::Block { items, .. } = body.definition().expect("expected body") else {
            panic!("expected block")
        };
        let Expression::If {
            condition,
            alternative: Some(alternative),
            ..
        } = &items[0]
        else {
            panic!("expected if expression")
        };
        let Expression::If {
            condition: next_condition,
            ..
        } = alternative.as_ref()
        else {
            panic!("expected else if expression")
        };
        (condition, next_condition)
    }

    #[test]
    fn equivalent_calls_remain_side_effecting() {
        let result = syntax::build_ast(
            "fn flag() -> bool { true }\nfn main() { if flag() {} else if flag() {} }",
            0,
        );
        let (condition, next_condition) = conditions(&result.ast[1..]);

        assert!(
            expressions_equivalent(condition, next_condition)
                && !is_side_effect_free(condition)
                && !is_side_effect_free(next_condition)
        );
    }

    #[test]
    fn equivalent_blocks_remain_side_effecting() {
        let result = syntax::build_ast("fn main() { if { true } {} else if { true } {} }", 0);
        let (condition, next_condition) = conditions(&result.ast);

        assert!(
            expressions_equivalent(condition, next_condition)
                && !is_side_effect_free(condition)
                && !is_side_effect_free(next_condition)
        );
    }
}
