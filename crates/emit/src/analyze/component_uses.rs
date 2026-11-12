use crate::patterns::matching::{PreludeVariant, prelude_variant};
use rustc_hash::FxHashSet as HashSet;
use syntax::ast::{
    BindingId, Expression, IdentifierResolution, MatchArm, Pattern, collect_pattern_bindings,
};

const STATUS_METHODS: &[&str] = &["is_some", "is_none", "is_ok", "is_err"];
const PAYLOAD_METHODS: &[&str] = &["unwrap_or", "map_or"];

#[derive(Clone, Default)]
pub(crate) struct ComponentDemand {
    pub(crate) value: ValueDemand,
    pub(crate) read_indices: HashSet<usize>,
    pub(crate) defaults: usize,
}

/// How much of a fallible local's payload later code reads.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ValueDemand {
    #[default]
    StatusOnly,
    Payload,
    /// The whole value, rebuilt from its components.
    Whole,
}

pub(crate) fn component_demand<'a, I>(region: I, binding_id: BindingId) -> Option<ComponentDemand>
where
    I: IntoIterator<Item = &'a Expression>,
{
    let mut walker = Walker {
        binding_id,
        supported: false,
        demand: ComponentDemand::default(),
    };
    walker.walk_items(region)?;
    walker.supported.then_some(walker.demand)
}

struct Walker {
    binding_id: BindingId,
    supported: bool,
    demand: ComponentDemand,
}

impl Walker {
    fn names_the_local(&self, expression: &Expression) -> bool {
        match expression.unwrap_parens() {
            Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => self.binding_id == *id,
            _ => false,
        }
    }

    fn targets_local(&self, target: &Expression) -> bool {
        target.root_binding_id() == Some(self.binding_id)
    }

    fn walk(&mut self, expression: &Expression) -> Option<()> {
        match expression {
            Expression::Identifier { .. } if self.names_the_local(expression) => {
                self.demand.value = ValueDemand::Whole;
                return Some(());
            }
            Expression::Assignment { target, value, .. } => {
                if self.targets_local(target) {
                    return None;
                }
                self.walk(target)?;
                self.walk(value)?;
                return Some(());
            }
            Expression::Reference { expression, .. } if self.names_the_local(expression) => {
                return None;
            }
            Expression::Match { subject, arms, .. } => {
                if self.names_the_local(subject) {
                    if !arms.iter().all(|arm| is_component_arm(&arm.pattern)) {
                        return None;
                    }
                    self.supported |= !arms.iter().any(MatchArm::has_guard);
                    let value = if arms.iter().any(MatchArm::has_guard) {
                        ValueDemand::Whole
                    } else if arms.iter().any(|arm| binds_payload(&arm.pattern)) {
                        ValueDemand::Payload
                    } else {
                        ValueDemand::StatusOnly
                    };
                    self.demand.value = self.demand.value.max(value);
                } else {
                    self.walk(subject)?;
                }
                self.walk_arms(arms)?;
                return Some(());
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                if self.names_the_local(scrutinee) {
                    if !is_component_arm(pattern) {
                        return None;
                    }
                    self.supported = true;
                    if binds_payload(pattern) {
                        self.demand.value = self.demand.value.max(ValueDemand::Payload);
                    }
                } else {
                    self.walk(scrutinee)?;
                }
                self.walk(consequence)?;
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative)?;
                }
                return Some(());
            }
            Expression::Propagate { expression, .. } if self.names_the_local(expression) => {
                self.demand.value = ValueDemand::Whole;
                return Some(());
            }
            Expression::DotAccess {
                expression: receiver,
                member,
                ..
            } if self.names_the_local(receiver)
                && let Ok(index) = member.parse::<usize>() =>
            {
                self.supported = true;
                self.demand.value = self.demand.value.max(ValueDemand::Payload);
                self.demand.read_indices.insert(index);
                return Some(());
            }
            Expression::Call {
                expression: callee,
                args,
                ..
            } => {
                if let Expression::DotAccess {
                    expression: receiver,
                    member,
                    ..
                } = callee.unwrap_parens()
                    && self.names_the_local(receiver)
                    && (STATUS_METHODS.contains(&member.as_str())
                        || PAYLOAD_METHODS.contains(&member.as_str()))
                {
                    self.supported = true;
                    if PAYLOAD_METHODS.contains(&member.as_str()) {
                        self.demand.value = self.demand.value.max(ValueDemand::Payload);
                    }
                    self.demand.defaults += usize::from(member == "unwrap_or");
                    for argument in args {
                        self.walk(argument)?;
                    }
                    return Some(());
                }
            }
            _ => {}
        }
        self.walk_items(expression.children())
    }

    fn walk_items<'e>(&mut self, items: impl IntoIterator<Item = &'e Expression>) -> Option<()> {
        for item in items {
            self.walk(item)?;
        }
        Some(())
    }

    fn walk_arms(&mut self, arms: &[MatchArm]) -> Option<()> {
        for arm in arms {
            if let Some(guard) = &arm.guard {
                self.walk(guard)?;
            }
            self.walk(&arm.expression)?;
        }
        Some(())
    }
}

fn is_component_arm(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::WildCard { .. } => true,
        _ => matches!(
            prelude_variant(pattern),
            Some((
                PreludeVariant::Some | PreludeVariant::None | PreludeVariant::Ok | PreludeVariant::Err,
                fields,
            )) if fields.len() <= 1
                && fields.iter().all(|field| {
                    matches!(field, Pattern::WildCard { .. } | Pattern::Identifier { .. })
                })
        ),
    }
}

fn binds_payload(pattern: &Pattern) -> bool {
    let is_err = matches!(prelude_variant(pattern), Some((PreludeVariant::Err, _)));
    !is_err && !collect_pattern_bindings(pattern).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syntax::ast::Span;
    use syntax::program::DotAccessResolution;
    use syntax::types::Type;

    fn local(id: u32) -> Expression {
        Expression::Identifier {
            value: "value".into(),
            ty: Type::unit(),
            span: Span::dummy(),
            resolution: IdentifierResolution::Binding(BindingId::new(id)),
        }
    }

    fn component() -> Expression {
        Expression::DotAccess {
            expression: Box::new(local(1)),
            member: "0".into(),
            ty: Type::unit(),
            span: Span::dummy(),
            resolution: DotAccessResolution::Unresolved,
        }
    }

    #[test]
    fn whole_value_demand_survives_component_reads_in_either_order() {
        let whole = local(1);
        let payload = component();
        for region in [[&whole, &payload], [&payload, &whole]] {
            let demand = component_demand(region, BindingId::new(1)).unwrap();
            assert!(demand.value == ValueDemand::Whole);
            assert!(demand.read_indices.contains(&0));
        }
    }

    #[test]
    fn mutation_rejects_component_lowering_in_either_order() {
        let mutation = Expression::Assignment {
            target: Box::new(local(1)),
            value: Box::new(local(2)),
            compound_operator: None,
            span: Span::dummy(),
        };
        let payload = component();
        for region in [[&mutation, &payload], [&payload, &mutation]] {
            assert!(component_demand(region, BindingId::new(1)).is_none());
        }
    }

    #[test]
    fn same_spelled_binding_does_not_increase_payload_demand() {
        let payload = component();
        let other = local(2);
        let demand = component_demand([&payload, &other], BindingId::new(1)).unwrap();
        assert!(demand.value == ValueDemand::Payload);
        assert!(component_demand([&other], BindingId::new(1)).is_none());
    }
}
