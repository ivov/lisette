use syntax::ast::{Expression, SelectArmPattern};

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
        walker.walk(consumer);
    }
    walker.decide()
}

pub(crate) fn region_blocks_inline<'a, I>(trees: I, lisette_name: &str) -> bool
where
    I: IntoIterator<Item = &'a Expression>,
{
    let mut walker = Walker::new(lisette_name);
    for tree in trees {
        walker.walk(tree);
    }
    walker.any_use_or_opacity()
}

struct Walker<'a> {
    name: &'a str,
    barriers_seen: u32,
    inside_reference_operand: u32,
    inside_assignment_target: u32,
    enclosure_depth: u32,
    shadow_depth: u32,
    uses: Vec<UseRecord>,
    opaque_raw_go_in_region: bool,
}

#[derive(Debug, Clone, Copy)]
struct UseRecord {
    inside_reference_operand: bool,
    inside_assignment_target: bool,
    inside_enclosure: bool,
    preceding_barriers: u32,
}

impl<'a> Walker<'a> {
    fn new(name: &'a str) -> Self {
        Self {
            name,
            barriers_seen: 0,
            inside_reference_operand: 0,
            inside_assignment_target: 0,
            enclosure_depth: 0,
            shadow_depth: 0,
            uses: Vec::new(),
            opaque_raw_go_in_region: false,
        }
    }

    fn any_use_or_opacity(&self) -> bool {
        !self.uses.is_empty() || self.opaque_raw_go_in_region
    }

    fn decide(self) -> InlineDecision {
        if self.uses.is_empty() {
            if self.opaque_raw_go_in_region {
                return InlineDecision::Keep;
            }
            return InlineDecision::Unused;
        }
        if self.opaque_raw_go_in_region {
            return InlineDecision::Keep;
        }
        if self.uses.len() > 1 {
            return InlineDecision::Keep;
        }
        let occ = self.uses[0];
        if occ.inside_reference_operand
            || occ.inside_assignment_target
            || occ.inside_enclosure
            || occ.preceding_barriers > 0
        {
            return InlineDecision::Keep;
        }
        InlineDecision::Inline
    }

    fn record_use(&mut self) {
        self.uses.push(UseRecord {
            inside_reference_operand: self.inside_reference_operand > 0,
            inside_assignment_target: self.inside_assignment_target > 0,
            inside_enclosure: self.enclosure_depth > 0,
            preceding_barriers: self.barriers_seen,
        });
    }

    fn walk(&mut self, expression: &Expression) {
        if self.shadow_depth > 0 {
            return;
        }
        match expression {
            Expression::Identifier { value, .. } => {
                if value.as_str() == self.name {
                    self.record_use();
                }
            }
            Expression::Literal { .. }
            | Expression::Unit { .. }
            | Expression::NoOp
            | Expression::Break { value: None, .. }
            | Expression::Continue { .. } => {}

            Expression::Call {
                expression: callee,
                args,
                spread,
                ..
            } => {
                self.walk(callee);
                for arg in args {
                    self.walk(arg);
                }
                if let Some(spread_arg) = spread.as_ref() {
                    self.walk(spread_arg);
                }
                self.barriers_seen += 1;
            }
            Expression::Propagate { expression, .. } => {
                self.walk(expression);
                self.barriers_seen += 1;
            }
            Expression::Assignment { target, value, .. } => {
                self.inside_assignment_target += 1;
                self.walk(target);
                self.inside_assignment_target -= 1;
                self.walk(value);
                self.barriers_seen += 1;
            }
            Expression::Reference { expression, .. } => {
                self.inside_reference_operand += 1;
                self.walk(expression);
                self.inside_reference_operand -= 1;
            }

            Expression::Block { items, .. } => {
                self.walk_block(items);
            }
            Expression::Let {
                binding,
                value,
                else_block,
                ..
            } => {
                self.walk(value);
                if let Some(else_b) = else_block {
                    self.walk(else_b);
                }
                if pattern_binds_name(&binding.pattern, self.name) {
                    self.shadow_depth += 1;
                }
            }

            Expression::If {
                condition,
                consequence,
                alternative,
                ..
            } => {
                self.walk(condition);
                self.walk(consequence);
                self.walk(alternative);
            }
            Expression::IfLet {
                pattern,
                scrutinee,
                consequence,
                alternative,
                ..
            } => {
                self.walk(scrutinee);
                self.with_shadow(pattern_binds_name(pattern, self.name), |w| {
                    w.walk(consequence)
                });
                self.walk(alternative);
            }
            Expression::Match { subject, arms, .. } => {
                self.walk(subject);
                for arm in arms {
                    self.with_shadow(pattern_binds_name(&arm.pattern, self.name), |w| {
                        if let Some(guard) = arm.guard.as_ref() {
                            w.walk(guard);
                        }
                        w.walk(&arm.expression);
                    });
                }
            }

            Expression::Tuple { elements, .. } => {
                for el in elements {
                    self.walk(el);
                }
            }
            Expression::StructCall {
                field_assignments, ..
            } => {
                for fa in field_assignments {
                    self.walk(&fa.value);
                }
            }
            Expression::IndexedAccess {
                expression, index, ..
            } => {
                self.walk(expression);
                self.walk(index);
            }
            Expression::Binary { left, right, .. } => {
                self.walk(left);
                self.walk(right);
            }
            Expression::Range { start, end, .. } => {
                if let Some(s) = start.as_ref() {
                    self.walk(s);
                }
                if let Some(e) = end.as_ref() {
                    self.walk(e);
                }
            }
            Expression::DotAccess { expression, .. }
            | Expression::Unary { expression, .. }
            | Expression::Paren { expression, .. }
            | Expression::Cast { expression, .. }
            | Expression::Return { expression, .. } => self.walk(expression),
            Expression::Break {
                value: Some(value), ..
            } => self.walk(value),

            Expression::Loop { body, .. } => self.walk_in_enclosure(body),
            Expression::While {
                condition, body, ..
            } => {
                self.enclosure_depth += 1;
                self.walk(condition);
                self.walk(body);
                self.enclosure_depth -= 1;
            }
            Expression::WhileLet {
                pattern,
                scrutinee,
                body,
                ..
            } => {
                self.enclosure_depth += 1;
                self.walk(scrutinee);
                self.with_shadow(pattern_binds_name(pattern, self.name), |w| w.walk(body));
                self.enclosure_depth -= 1;
            }
            Expression::For {
                binding,
                iterable,
                body,
                ..
            } => {
                self.walk(iterable);
                self.enclosure_depth += 1;
                self.with_shadow(pattern_binds_name(&binding.pattern, self.name), |w| {
                    w.walk(body)
                });
                self.enclosure_depth -= 1;
            }

            Expression::Lambda { params, body, .. } | Expression::Function { params, body, .. } => {
                self.enclosure_depth += 1;
                let shadowed = params
                    .iter()
                    .any(|p| pattern_binds_name(&p.pattern, self.name));
                self.with_shadow(shadowed, |w| w.walk(body));
                self.enclosure_depth -= 1;
            }
            Expression::Task { expression, .. } | Expression::Defer { expression, .. } => {
                self.walk_in_enclosure(expression);
                self.barriers_seen += 1;
            }

            Expression::Select { arms, .. } => {
                // Mark the barrier before walking arms so uses inside any arm
                // see the select wait as preceding.
                self.barriers_seen += 1;
                for arm in arms {
                    self.walk_select_arm(&arm.pattern);
                }
            }
            Expression::TryBlock { items, .. } | Expression::RecoverBlock { items, .. } => {
                self.walk_block(items);
                self.barriers_seen += 1;
            }
            Expression::RawGo { .. } => {
                self.opaque_raw_go_in_region = true;
                self.barriers_seen += 1;
            }

            // Block-local `Const`/`Function` shadowing is applied in `walk_block`.
            Expression::Const { expression, .. } => self.walk(expression),
            Expression::VariableDeclaration { .. } => {}

            Expression::ImplBlock { methods, .. } => {
                for m in methods {
                    self.walk(m);
                }
            }

            Expression::Enum { .. }
            | Expression::Struct { .. }
            | Expression::TypeAlias { .. }
            | Expression::ModuleImport { .. }
            | Expression::Interface { .. } => {}
        }
    }

    fn walk_in_enclosure(&mut self, expression: &Expression) {
        self.enclosure_depth += 1;
        self.walk(expression);
        self.enclosure_depth -= 1;
    }

    /// Run `f` with `shadow_depth` raised while `shadowed` is true.
    fn with_shadow(&mut self, shadowed: bool, f: impl FnOnce(&mut Self)) {
        if shadowed {
            self.shadow_depth += 1;
        }
        f(self);
        if shadowed {
            self.shadow_depth -= 1;
        }
    }

    fn walk_block(&mut self, items: &[Expression]) {
        let pre_shadow = self.shadow_depth;
        let block_shadows: u32 = items
            .iter()
            .filter(|item| match item {
                Expression::Const { identifier, .. } => identifier.as_str() == self.name,
                Expression::Function { name, .. } => name.as_str() == self.name,
                _ => false,
            })
            .count() as u32;
        self.shadow_depth += block_shadows;
        for item in items {
            self.walk(item);
        }
        self.shadow_depth = pre_shadow;
    }

    fn walk_select_arm(&mut self, pattern: &SelectArmPattern) {
        match pattern {
            SelectArmPattern::Receive {
                binding,
                receive_expression,
                body,
                ..
            } => {
                self.walk(receive_expression);
                let shadow = pattern_binds_name(binding, self.name);
                if shadow {
                    self.shadow_depth += 1;
                }
                self.walk_in_enclosure(body);
                if shadow {
                    self.shadow_depth -= 1;
                }
            }
            SelectArmPattern::Send {
                send_expression,
                body,
            } => {
                self.walk(send_expression);
                self.walk_in_enclosure(body);
            }
            SelectArmPattern::MatchReceive {
                receive_expression,
                arms,
            } => {
                self.walk(receive_expression);
                self.enclosure_depth += 1;
                for arm in arms {
                    let shadow = pattern_binds_name(&arm.pattern, self.name);
                    if shadow {
                        self.shadow_depth += 1;
                    }
                    if let Some(guard) = arm.guard.as_ref() {
                        self.walk(guard);
                    }
                    self.walk(&arm.expression);
                    if shadow {
                        self.shadow_depth -= 1;
                    }
                }
                self.enclosure_depth -= 1;
            }
            SelectArmPattern::WildCard { body } => {
                self.walk_in_enclosure(body);
            }
        }
    }
}
