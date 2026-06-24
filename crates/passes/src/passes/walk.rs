use std::cell::RefCell;

use diagnostics::LocalSink;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::{
    Expression, ExpressionKind, Pattern, PatternKind, RestPattern, SelectArmPattern, Span,
};
use syntax::program::File;

use semantics::facts::Facts;
use semantics::store::Store;

pub(crate) struct NodeCtx<'a> {
    pub store: &'a Store,
    pub facts: &'a Facts,
    pub files: &'a HashMap<u32, File>,
    pub module_id: &'a str,
    pub source: &'a str,
    pub is_d_lis: bool,
    pub sink: &'a LocalSink,
    /// Node spans already claimed by an enclosing node, so a check does not also
    /// judge them standalone (e.g. the nested `&&` of an outer comparison chain).
    pub claimed_spans: RefCell<HashSet<Span>>,
}

pub(crate) type NodeCheck = fn(&Expression, &NodeCtx);
pub(crate) type PatternCheck = fn(&Pattern, &NodeCtx);

pub(crate) struct CheckTable {
    expression_buckets: [Vec<NodeCheck>; ExpressionKind::COUNT],
    pattern_buckets: [Vec<PatternCheck>; PatternKind::COUNT],
}

impl CheckTable {
    pub(crate) fn new(
        expression_checks: &[(NodeCheck, &[ExpressionKind])],
        pattern_checks: &[(PatternCheck, &[PatternKind])],
    ) -> Self {
        let mut expression_buckets: [Vec<NodeCheck>; ExpressionKind::COUNT] =
            std::array::from_fn(|_| Vec::new());
        for (check, kinds) in expression_checks {
            for kind in *kinds {
                expression_buckets[*kind as usize].push(*check);
            }
        }
        let mut pattern_buckets: [Vec<PatternCheck>; PatternKind::COUNT] =
            std::array::from_fn(|_| Vec::new());
        for (check, kinds) in pattern_checks {
            for kind in *kinds {
                pattern_buckets[*kind as usize].push(*check);
            }
        }
        Self {
            expression_buckets,
            pattern_buckets,
        }
    }
}

pub(crate) fn walk_nodes(ast: &[Expression], ctx: &NodeCtx, checks: &CheckTable) {
    visit_ast(
        ast,
        &mut |expression| {
            for check in &checks.expression_buckets[expression.kind() as usize] {
                check(expression, ctx);
            }
        },
        &mut |pattern| {
            for check in &checks.pattern_buckets[pattern.kind() as usize] {
                check(pattern, ctx);
            }
        },
    );
}

pub fn visit_ast<E, P>(ast: &[Expression], expression_visitor: &mut E, pattern_visitor: &mut P)
where
    E: FnMut(&Expression),
    P: FnMut(&Pattern),
{
    for expression in ast {
        visit_node(expression, expression_visitor, pattern_visitor);
    }
}

fn visit_node<E, P>(expression: &Expression, expression_visitor: &mut E, pattern_visitor: &mut P)
where
    E: FnMut(&Expression),
    P: FnMut(&Pattern),
{
    expression_visitor(expression);

    match expression {
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                visit_pattern(&param.pattern, pattern_visitor);
            }
        }
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            visit_pattern(&binding.pattern, pattern_visitor);
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            visit_pattern(pattern, pattern_visitor);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                visit_pattern(&arm.pattern, pattern_visitor);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                if let SelectArmPattern::MatchReceive {
                    arms: match_arms, ..
                } = &arm.pattern
                {
                    for match_arm in match_arms {
                        visit_pattern(&match_arm.pattern, pattern_visitor);
                    }
                }
            }
        }
        _ => {}
    }

    for child in expression.children() {
        visit_node(child, expression_visitor, pattern_visitor);
    }
}

fn visit_pattern<F: FnMut(&Pattern)>(pattern: &Pattern, visitor: &mut F) {
    visitor(pattern);

    match pattern {
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::WildCard { .. }
        | Pattern::Identifier { .. } => {}

        Pattern::EnumVariant { fields, .. } => {
            for field in fields {
                visit_pattern(field, visitor);
            }
        }

        Pattern::Struct { fields, .. } => {
            for field in fields {
                visit_pattern(&field.value, visitor);
            }
        }

        Pattern::Tuple { elements, .. } => {
            for element in elements {
                visit_pattern(element, visitor);
            }
        }

        Pattern::Slice { prefix, rest, .. } => {
            for p in prefix {
                visit_pattern(p, visitor);
            }
            if let RestPattern::Bind { .. } = rest {
                // rest binding is not a pattern itself
            }
        }

        Pattern::Or { patterns, .. } => {
            for p in patterns {
                visit_pattern(p, visitor);
            }
        }

        Pattern::AsBinding { pattern, .. } => {
            visit_pattern(pattern, visitor);
        }
    }
}
