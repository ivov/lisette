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
            Expression::Unit { .. }
            | Expression::Break { value: None, .. }
            | Expression::Continue { .. } => {}

            Expression::Call {
                expression: callee,
                args,
                spread,
                ..
            } => {
                self.walk(callee, ctx);
                for arg in args {
                    self.walk(arg, ctx);
                }
                if let Some(spread_arg) = spread.as_ref() {
                    self.walk(spread_arg, ctx);
                }
                self.crossed_barrier = true;
            }
            Expression::Propagate { expression, .. } => {
                self.walk(expression, ctx);
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
            Expression::Let {
                binding: _,
                value,
                mode,
                ..
            } => {
                self.walk(value, ctx);
                if let Some(else_b) = mode.else_block() {
                    self.walk(else_b, ctx);
                }
            }

            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.walk(condition, ctx);
                self.walk(consequence, ctx);
                if let Some(alternative) = alternative {
                    self.walk(alternative, ctx);
                }
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

            Expression::Tuple { elements, .. } => {
                for el in elements {
                    self.walk(el, ctx);
                }
            }
            Expression::StructCall {
                field_assignments, ..
            } => {
                for fa in field_assignments {
                    self.walk(&fa.value, ctx);
                }
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.walk(expression, ctx);
                self.walk(index, ctx);
            }
            Expression::Binary { left, right, .. } => {
                self.walk(left, ctx);
                self.walk(right, ctx);
            }
            Expression::Range { start, end, .. } => {
                if let Some(s) = start.as_ref() {
                    self.walk(s, ctx);
                }
                if let Some(e) = end.as_ref() {
                    self.walk(e, ctx);
                }
            }
            Expression::DotAccess { expression, .. }
            | Expression::Unary { expression, .. }
            | Expression::Paren { expression, .. }
            | Expression::Cast { expression, .. }
            | Expression::Return { expression, .. } => self.walk(expression, ctx),
            Expression::Break {
                value: Some(value), ..
            } => self.walk(value, ctx),

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

            Expression::Assert { expression, .. } => self.walk(expression, ctx),

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

            // Block-local `Const`/`Function` shadowing is applied in `walk_block`.
            Expression::Const { expression, .. } => {
                if let Some(value) = expression.value() {
                    self.walk(value, ctx);
                }
            }
            Expression::VariableDeclaration { .. } => {}

            Expression::ImplBlock { methods, .. } => {
                for m in methods {
                    self.walk(m, ctx);
                }
            }

            Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::PackageImport { .. }
            | Expression::Interface { .. } => {}
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
