pub mod analyze;
pub mod cache;
pub mod call_classification;
pub mod checker;
pub mod context;
pub mod facts;
pub mod loader;
pub mod module_graph;
pub mod prelude;
pub mod store;
pub mod validators;

use syntax::ast::Expression;

pub(crate) fn is_trivial_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Unit { .. } => true,
        Expression::Block { items, .. } => {
            items.is_empty() || (items.len() == 1 && matches!(items[0], Expression::Unit { .. }))
        }
        Expression::Tuple { elements, .. } => elements.is_empty(),
        _ => false,
    }
}
