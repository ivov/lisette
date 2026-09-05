use syntax::ast::{Expression, FormatStringPart, Literal, SelectArm};

use crate::patterns::binding_decls::pattern_binds_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InlineDecision {
    Inline,
    Unused,
    Keep,
}

pub(crate) fn analyze_inline_candidate(
    lisette_name: &str,
    consumers: &[&Expression],
) -> InlineDecision {
    let mut walker = Walker::new(lisette_name);
    for consumer in consumers {
        walker.walk(consumer, WalkContext::default());
    }
    walker.decide()
}

pub(crate) fn region_blocks_inline<'a, I>(trees: I, lisette_name: &str) -> bool
where
    I: IntoIterator<Item = &'a Expression>,
{
    let mut walker = Walker::new(lisette_name);
    for tree in trees {
        walker.walk(tree, WalkContext::default());
    }
    walker.any_use_or_opacity()
}

struct Walker<'a> {
    name: &'a str,
    crossed_barrier: bool,
    uses: Vec<InlineEligibility>,
    opaque_raw_go_in_region: bool,
}

#[derive(Clone, Copy, Default)]
struct WalkContext {
    eligibility: InlineEligibility,
    shadowed: bool,
}

#[derive(Clone, Copy, Default)]
enum InlineEligibility {
    #[default]
    Eligible,
    Blocked,
}

impl WalkContext {
    fn reference_operand(self) -> Self {
        Self {
            eligibility: InlineEligibility::Blocked,
            ..self
        }
    }

    fn assignment_target(self) -> Self {
        Self {
            eligibility: InlineEligibility::Blocked,
            ..self
        }
    }

    fn enclosure(self) -> Self {
        Self {
            eligibility: InlineEligibility::Blocked,
            ..self
        }
    }

    fn shadowed_if(self, shadowed: bool) -> Self {
        Self {
            shadowed: self.shadowed || shadowed,
            ..self
        }
    }
}

impl<'a> Walker<'a> {
    fn new(name: &'a str) -> Self {
        Self {
            name,
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
            (false, [InlineEligibility::Eligible]) => InlineDecision::Inline,
            _ => InlineDecision::Keep,
        }
    }

    fn record_use(&mut self, ctx: WalkContext) {
        self.uses.push(if self.crossed_barrier {
            InlineEligibility::Blocked
        } else {
            ctx.eligibility
        });
    }

    fn walk(&mut self, expression: &Expression, ctx: WalkContext) {
        if ctx.shadowed {
            return;
        }
        match expression {
            Expression::Identifier { value, .. } => {
                if value.as_str() == self.name {
                    self.record_use(ctx);
                }
            }
            Expression::Literal { literal, .. } => {
                if let Literal::FormatString(parts) = literal {
                    for part in parts {
                        if let FormatStringPart::Expression(expression) = part {
                            self.walk(expression, ctx);
                        }
                    }
                    self.crossed_barrier = true;
                }
            }

            Expression::Call { .. } | Expression::Propagate { .. } => {
                for child in expression.children() {
                    self.walk(child, ctx);
                }
                self.crossed_barrier = true;
            }
            Expression::Assignment { target, value, .. } => {
                self.walk(target, ctx.assignment_target());
                self.walk(value, ctx);
                self.crossed_barrier = true;
            }
            Expression::Reference { expression, .. } => {
                self.walk(expression, ctx.reference_operand());
            }

            Expression::Block { items, .. } => {
                self.walk_block(items, ctx);
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.walk(scrutinee, ctx);
                self.walk(
                    consequence,
                    ctx.shadowed_if(pattern_binds_name(pattern, self.name)),
                );
                if let Some(alternative) = alternative.expression() {
                    self.walk(alternative, ctx);
                }
            }
            Expression::Match { subject, arms, .. } => {
                self.walk(subject, ctx);
                for arm in arms {
                    let arm_ctx = ctx.shadowed_if(pattern_binds_name(&arm.pattern, self.name));
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, arm_ctx);
                    }
                    self.walk(&arm.expression, arm_ctx);
                }
            }

            Expression::StructCall {
                field_assignments, ..
            } => {
                for fa in field_assignments {
                    self.walk(&fa.value, ctx);
                }
            }

            Expression::Loop { body, .. } => self.walk(body, ctx.enclosure()),
            Expression::While {
                condition, body, ..
            } => {
                let ctx = ctx.enclosure();
                self.walk(condition, ctx);
                self.walk(body, ctx);
            }
            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                ..
            } => {
                let ctx = ctx.enclosure();
                self.walk(scrutinee, ctx);
                self.walk(
                    body,
                    ctx.shadowed_if(pattern_binds_name(pattern, self.name)),
                );
            }
            Expression::For {
                binding,
                iterable,
                body,
                ..
            } => {
                self.walk(iterable, ctx);
                self.walk(
                    body,
                    ctx.enclosure()
                        .shadowed_if(pattern_binds_name(&binding.pattern, self.name)),
                );
            }

            Expression::Lambda { params, body, .. } => {
                let shadowed = params
                    .iter()
                    .any(|p| pattern_binds_name(&p.pattern, self.name));
                self.walk(body, ctx.enclosure().shadowed_if(shadowed));
            }
            Expression::Function { params, body, .. } => {
                let shadowed = params
                    .iter()
                    .any(|p| pattern_binds_name(&p.pattern, self.name));
                if let Some(body) = body.definition() {
                    self.walk(body, ctx.enclosure().shadowed_if(shadowed));
                }
            }
            Expression::Task { expression, .. } | Expression::Defer { expression, .. } => {
                self.walk(expression, ctx.enclosure());
                self.crossed_barrier = true;
            }

            Expression::Select { arms, .. } => {
                // Mark the barrier before walking arms so uses inside any arm
                // see the select wait as preceding.
                self.crossed_barrier = true;
                for arm in arms {
                    self.walk_select_arm(arm, ctx);
                }
            }
            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                self.walk_block(items, ctx);
                self.crossed_barrier = true;
            }
            Expression::RawGo { .. } => {
                self.opaque_raw_go_in_region = true;
                self.crossed_barrier = true;
            }

            Expression::Interface { .. } => {}
            _ => {
                for child in expression.children() {
                    self.walk(child, ctx);
                }
            }
        }
    }

    fn walk_block(&mut self, items: &[Expression], ctx: WalkContext) {
        let block_shadows = items.iter().any(|item| match item {
            Expression::Const { identifier, .. } => identifier.as_str() == self.name,
            Expression::Function { name, .. } => name.as_str() == self.name,
            _ => false,
        });
        let mut item_ctx = ctx.shadowed_if(block_shadows);
        for item in items {
            self.walk(item, item_ctx);
            if let Expression::Let { binding, .. } = item {
                item_ctx = item_ctx.shadowed_if(pattern_binds_name(&binding.pattern, self.name));
            }
        }
    }

    fn walk_select_arm(&mut self, pattern: &SelectArm, ctx: WalkContext) {
        match pattern {
            SelectArm::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => {
                self.walk(receive_expression, ctx);
                self.walk(
                    body,
                    ctx.enclosure()
                        .shadowed_if(pattern_binds_name(binding, self.name)),
                );
            }
            SelectArm::Send {
                send_expression,
                body,
            } => {
                self.walk(send_expression, ctx);
                self.walk(body, ctx.enclosure());
            }
            SelectArm::MatchReceive {
                receive_expression,
                arms,
            } => {
                self.walk(receive_expression, ctx);
                let ctx = ctx.enclosure();
                for arm in arms {
                    let arm_ctx = ctx.shadowed_if(pattern_binds_name(&arm.pattern, self.name));
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard, arm_ctx);
                    }
                    self.walk(&arm.expression, arm_ctx);
                }
            }
            SelectArm::WildCard { body } => {
                self.walk(body, ctx.enclosure());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_decision(source: &str) -> InlineDecision {
        let parsed = syntax::build_ast(&format!("fn test() {{ {source} }}"), 0);
        assert!(!parsed.has_errors(), "{:?}", parsed.errors);
        let Expression::Function { body, .. } = &parsed.ast[0] else {
            panic!("expected a function");
        };
        analyze_inline_candidate("value", &[body.definition().unwrap()])
    }

    #[test]
    fn call_argument_can_inline_before_a_later_spread_call() {
        assert_eq!(
            inline_decision("consume(value, later()...)"),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn spread_use_stays_bound_after_an_earlier_argument_call() {
        assert_eq!(
            inline_decision("consume(earlier(), value...)"),
            InlineDecision::Keep,
        );
    }

    #[test]
    fn tuple_use_stays_bound_after_an_earlier_element_call() {
        assert_eq!(inline_decision("(earlier(), value)"), InlineDecision::Keep,);
    }

    #[test]
    fn format_string_blocks_a_later_use() {
        assert_eq!(inline_decision("f\"{other}\"\nvalue"), InlineDecision::Keep,);
    }

    #[test]
    fn shadowing_starts_after_the_let_initializer() {
        assert_eq!(
            inline_decision("let value = value\nvalue"),
            InlineDecision::Inline,
        );
    }

    #[test]
    fn block_constant_shadows_even_a_preceding_use() {
        assert_eq!(
            inline_decision("value\nconst value = 1"),
            InlineDecision::Unused,
        );
    }

    #[test]
    fn slice_literal_contents_do_not_count_as_inline_uses() {
        assert_eq!(inline_decision("[value]"), InlineDecision::Unused);
    }

    #[test]
    fn struct_spread_does_not_count_as_an_inline_use() {
        assert_eq!(
            inline_decision("Record { field: other, ..value }"),
            InlineDecision::Unused,
        );
    }
}
