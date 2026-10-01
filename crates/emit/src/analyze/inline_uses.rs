use syntax::ast::{
    BindingId, Expression, FormatStringPart, IdentifierResolution, Literal, SelectArm,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineDecision {
    Inline,
    Unused,
    Keep,
}

pub(crate) fn analyze_inline_candidate(
    binding_id: BindingId,
    consumers: &[&Expression],
) -> InlineDecision {
    let mut walker = Walker::new(binding_id);
    for consumer in consumers {
        walker.walk(consumer, AccessRole::Read);
    }
    walker.decide()
}

pub(crate) fn analyze_inline_candidate_ids(
    binding_ids: &[BindingId],
    consumers: &[&Expression],
) -> InlineDecision {
    if binding_ids.is_empty() {
        return InlineDecision::Keep;
    }
    if binding_ids.len() <= 1 {
        return analyze_inline_candidate(binding_ids[0], consumers);
    }
    let mut walker = Walker::new(binding_ids[0]);
    walker.alternative_ids.extend_from_slice(&binding_ids[1..]);
    for consumer in consumers {
        walker.walk(consumer, AccessRole::Read);
    }
    walker.decide()
}

pub(crate) fn region_blocks_inline<'a, I>(trees: I, binding_id: Option<BindingId>) -> bool
where
    I: IntoIterator<Item = &'a Expression>,
{
    let Some(binding_id) = binding_id else {
        return true;
    };
    let mut walker = Walker::new(binding_id);
    for tree in trees {
        walker.walk(tree, AccessRole::Read);
    }
    walker.any_use_or_opacity()
}

struct Walker {
    binding_id: BindingId,
    alternative_ids: Vec<BindingId>,
    crossed_barrier: bool,
    uses: Vec<Access>,
    opaque_raw_go_in_region: bool,
}

#[derive(Clone, Copy)]
struct Access {
    role: AccessRole,
    after_barrier: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AccessRole {
    #[default]
    Read,
    Write,
    Address,
    Capture,
}

impl Walker {
    fn new(binding_id: BindingId) -> Self {
        Self {
            binding_id,
            alternative_ids: Vec::new(),
            crossed_barrier: false,
            uses: Vec::new(),
            opaque_raw_go_in_region: false,
        }
    }

    fn any_use_or_opacity(&self) -> bool {
        !self.uses.is_empty() || self.opaque_raw_go_in_region
    }

    fn decide(self) -> InlineDecision {
        match (self.opaque_raw_go_in_region, self.uses.as_slice()) {
            (false, []) => InlineDecision::Unused,
            (
                false,
                [
                    Access {
                        role: AccessRole::Read,
                        after_barrier: false,
                    },
                ],
            ) => InlineDecision::Inline,
            _ => InlineDecision::Keep,
        }
    }

    fn record_use(&mut self, role: AccessRole) {
        self.uses.push(Access {
            role,
            after_barrier: self.crossed_barrier,
        });
    }

    fn walk(&mut self, expression: &Expression, role: AccessRole) {
        match expression {
            Expression::Identifier { resolution, .. } => {
                let names_local = *resolution == IdentifierResolution::Binding(self.binding_id)
                    || resolution
                        .binding_id()
                        .is_some_and(|id| self.alternative_ids.contains(&id));
                if names_local {
                    self.record_use(role);
                }
            }
            Expression::Literal { literal, .. } => {
                if let Literal::FormatString(parts) = literal {
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            self.walk(expression, role);
                        }
                    }
                    self.crossed_barrier = true;
                }
            }

            Expression::Call { .. } | Expression::Propagate { .. } => {
                for child in expression.children() {
                    self.walk(child, role);
                }
                self.crossed_barrier = true;
            }
            Expression::Assignment { target, value, .. } => {
                self.walk(target, AccessRole::Write);
                self.walk(value, role);
                self.crossed_barrier = true;
            }
            Expression::Reference { expression, .. } => {
                self.walk(expression, AccessRole::Address);
            }

            Expression::Block { items, .. } => {
                self.walk_block(items, role);
            }
            Expression::IfLet {
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.walk(scrutinee, role);
                self.walk(consequence, role);
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative, role);
                }
            }
            Expression::Match { subject, arms, .. } => {
                self.walk(subject, role);
                for arm in arms {
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, role);
                    }
                    self.walk(&arm.expression, role);
                }
            }

            Expression::StructCall {
                field_assignments, ..
            } => {
                for fa in field_assignments {
                    self.walk(&fa.value, role);
                }
            }

            Expression::Loop { body, .. } => self.walk(body, AccessRole::Capture),
            Expression::While {
                condition, body, ..
            } => {
                let role = AccessRole::Capture;
                self.walk(condition, role);
                self.walk(body, role);
            }
            Expression::WhileLet {
                scrutinee, body, ..
            } => {
                let role = AccessRole::Capture;
                self.walk(scrutinee, role);
                self.walk(body, role);
            }
            Expression::For { iterable, body, .. } => {
                self.walk(iterable, role);
                self.walk(body, AccessRole::Capture);
            }

            Expression::Lambda { body, .. } => self.walk(body, AccessRole::Capture),
            Expression::Function { body, .. } => {
                if let Some(body) = body.definition() {
                    self.walk(body, AccessRole::Capture);
                }
            }
            Expression::Task { expression, .. } | Expression::Defer { expression, .. } => {
                self.walk(expression, AccessRole::Capture);
                self.crossed_barrier = true;
            }

            Expression::Select { arms, .. } => {
                // Mark the barrier before walking arms so uses inside any arm
                // see the select wait as preceding.
                self.crossed_barrier = true;
                for arm in arms {
                    self.walk_select_arm(arm, role);
                }
            }
            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                self.walk_block(items, role);
                self.crossed_barrier = true;
            }
            Expression::RawGo { .. } => {
                self.opaque_raw_go_in_region = true;
                self.crossed_barrier = true;
            }

            Expression::Interface { .. } => {}
            _ => {
                for child in expression.children() {
                    self.walk(child, role);
                }
            }
        }
    }

    fn walk_block(&mut self, items: &[Expression], role: AccessRole) {
        for item in items {
            self.walk(item, role);
        }
    }

    fn walk_select_arm(&mut self, pattern: &SelectArm, role: AccessRole) {
        match pattern {
            SelectArm::Receive {
                receive_expression,
                body,
                ..
            } => {
                self.walk(receive_expression, role);
                self.walk(body, AccessRole::Capture);
            }
            SelectArm::Send {
                send_expression,
                body,
            } => {
                self.walk(send_expression, role);
                self.walk(body, AccessRole::Capture);
            }
            SelectArm::MatchReceive {
                receive_expression,
                arms,
            } => {
                self.walk(receive_expression, role);
                let role = AccessRole::Capture;
                for arm in arms {
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, role);
                    }
                    self.walk(&arm.expression, role);
                }
            }
            SelectArm::WildCard { body } => {
                self.walk(body, AccessRole::Capture);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark_uses(expression: &mut Expression, selected: &[usize], seen: &mut usize) {
        match expression {
            Expression::Identifier {
                value, resolution, ..
            } if value == "value" => {
                if selected.contains(seen) {
                    *resolution = IdentifierResolution::Binding(BindingId::new(1));
                }
                *seen += 1;
            }
            Expression::Block { items, .. }
            | Expression::Tuple {
                elements: items, ..
            } => {
                for item in items {
                    mark_uses(item, selected, seen);
                }
            }
            Expression::Let { value, .. } => mark_uses(value, selected, seen),
            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                mark_uses(expression, selected, seen);
                for arg in args {
                    mark_uses(arg, selected, seen);
                }
                if let Some(spread) = spread {
                    mark_uses(spread, selected, seen);
                }
            }
            Expression::Literal {
                literal: Literal::FormatString(parts),
                ..
            } => {
                for part in parts {
                    if let FormatStringPart::Expression(expression) = part {
                        mark_uses(expression, selected, seen);
                    }
                }
            }
            Expression::Literal {
                literal: Literal::Slice(items),
                ..
            } => {
                for item in items {
                    mark_uses(item, selected, seen);
                }
            }
            Expression::Paren { expression, .. } => mark_uses(expression, selected, seen),
            Expression::Const { .. }
            | Expression::StructCall { .. }
            | Expression::Identifier { .. }
            | Expression::Literal { .. } => {}
            other => panic!("test helper does not handle {other:?}"),
        }
    }

    fn inline_decision(source: &str, selected: &[usize]) -> InlineDecision {
        let mut parsed = syntax::build_ast(&format!("fn test() {{ {source} }}"), 0);
        assert!(!parsed.has_errors(), "{:?}", parsed.errors);
        let Expression::Function { body, .. } = &mut parsed.ast[0] else {
            panic!("expected a function");
        };
        let body = body.definition_mut().unwrap();
        mark_uses(body, selected, &mut 0);
        analyze_inline_candidate(BindingId::new(1), &[body])
    }

    #[test]
    fn call_argument_can_inline_before_a_later_spread_call() {
        assert_eq!(
            inline_decision("consume(value, later()...)", &[0]),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn spread_use_stays_bound_after_an_earlier_argument_call() {
        assert_eq!(
            inline_decision("consume(earlier(), value...)", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn tuple_use_stays_bound_after_an_earlier_element_call() {
        assert_eq!(
            inline_decision("(earlier(), value)", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn format_string_blocks_a_later_use() {
        assert_eq!(
            inline_decision("f\"{other}\"\nvalue", &[0]),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn shadowing_starts_after_the_let_initializer() {
        assert_eq!(
            inline_decision("let value = value\nvalue", &[0]),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn block_constant_shadows_even_a_preceding_use() {
        assert_eq!(
            inline_decision("value\nconst value = 1", &[]),
            InlineDecision::Unused,
        );
    }

    #[test]
    fn slice_literal_contents_do_not_count_as_inline_uses() {
        assert_eq!(inline_decision("[value]", &[0]), InlineDecision::Unused);
    }

    #[test]
    fn struct_spread_does_not_count_as_an_inline_use() {
        assert_eq!(
            inline_decision("Record { field: other, ..value }", &[]),
            InlineDecision::Unused,
        );
    }
}
