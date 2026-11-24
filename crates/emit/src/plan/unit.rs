use crate::Planner;
use crate::ir::values::GoExpression;
use syntax::ast::Expression;
use syntax::types::Type;

impl Planner<'_> {
    pub(crate) fn unit_value(
        &self,
        source: Option<&Expression>,
        slot: Option<&Type>,
    ) -> GoExpression {
        if let Some(slot) = slot
            && !self.slot_holds_unit(slot)
        {
            panic!("internal error: a unit value reached a slot of type `{slot}`");
        }
        if let Some(source) = source {
            self.record_converted_value(source);
        }
        GoExpression::unit()
    }

    fn slot_holds_unit(&self, slot: &Type) -> bool {
        self.facts.resolves_to_unit(slot)
            || self.facts.is_interface_or_unknown(slot)
            || self.facts.is_parameter_or_open(slot)
    }
}
