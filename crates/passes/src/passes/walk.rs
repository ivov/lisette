use std::cell::{Cell, RefCell};

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
    pub function_role: Cell<FunctionRole<'a>>,
    pub pattern_role: Cell<PatternRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternRole {
    Parameter,
    #[default]
    Binding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FunctionRole<'a> {
    InterfaceMethod {
        public: bool,
    },
    ImplMethod {
        type_name: &'a str,
    },
    #[default]
    Free,
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

pub(crate) fn walk_nodes<'a>(ast: &'a [Expression], ctx: &NodeCtx<'a>, checks: &CheckTable) {
    visit_ast(
        ast,
        &mut |expression, role| {
            ctx.function_role.set(role);
            for check in &checks.expression_buckets[expression.kind() as usize] {
                check(expression, ctx);
            }
        },
        &mut |pattern, role| {
            ctx.pattern_role.set(role);
            for check in &checks.pattern_buckets[pattern.kind() as usize] {
                check(pattern, ctx);
            }
        },
    );
}

pub fn visit_ast<'a, E, P>(
    ast: &'a [Expression],
    expression_visitor: &mut E,
    pattern_visitor: &mut P,
) where
    E: FnMut(&Expression, FunctionRole<'a>),
    P: FnMut(&Pattern, PatternRole),
{
    for expression in ast {
        visit_node(
            expression,
            FunctionRole::Free,
            expression_visitor,
            pattern_visitor,
        );
    }
}

fn visit_node<'a, E, P>(
    expression: &'a Expression,
    role: FunctionRole<'a>,
    expression_visitor: &mut E,
    pattern_visitor: &mut P,
) where
    E: FnMut(&Expression, FunctionRole<'a>),
    P: FnMut(&Pattern, PatternRole),
{
    expression_visitor(expression, role);

    match expression {
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for param in params {
                visit_pattern(&param.pattern, PatternRole::Parameter, pattern_visitor);
            }
        }
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            visit_pattern(&binding.pattern, PatternRole::Binding, pattern_visitor);
        }
        Expression::IfLet { pattern, .. } | Expression::WhileLet { pattern, .. } => {
            visit_pattern(pattern, PatternRole::Binding, pattern_visitor);
        }
        Expression::Match { arms, .. } => {
            for arm in arms {
                visit_pattern(&arm.pattern, PatternRole::Binding, pattern_visitor);
            }
        }
        Expression::Select { arms, .. } => {
            for arm in arms {
                match &arm.pattern {
                    SelectArmPattern::Receive { binding, .. } => {
                        visit_pattern(binding, PatternRole::Binding, pattern_visitor);
                    }
                    SelectArmPattern::MatchReceive {
                        arms: match_arms, ..
                    } => {
                        for match_arm in match_arms {
                            visit_pattern(
                                &match_arm.pattern,
                                PatternRole::Binding,
                                pattern_visitor,
                            );
                        }
                    }
                    SelectArmPattern::Send { .. } | SelectArmPattern::WildCard { .. } => {}
                }
            }
        }
        _ => {}
    }

    let child_role = match expression {
        Expression::Interface { visibility, .. } => FunctionRole::InterfaceMethod {
            public: visibility.is_public(),
        },
        Expression::ImplBlock { receiver_name, .. } => FunctionRole::ImplMethod {
            type_name: receiver_name.as_str(),
        },
        _ => FunctionRole::Free,
    };
    for child in expression.children() {
        visit_node(child, child_role, expression_visitor, pattern_visitor);
    }
}

fn visit_pattern<F: FnMut(&Pattern, PatternRole)>(
    pattern: &Pattern,
    role: PatternRole,
    visitor: &mut F,
) {
    visitor(pattern, role);

    match pattern {
        Pattern::Literal { .. }
        | Pattern::Unit { .. }
        | Pattern::WildCard { .. }
        | Pattern::Identifier { .. } => {}

        Pattern::EnumVariant { fields, .. } => {
            for field in fields {
                visit_pattern(field, role, visitor);
            }
        }

        Pattern::Struct { fields, .. } => {
            for field in fields {
                visit_pattern(&field.value, role, visitor);
            }
        }

        Pattern::Tuple { elements, .. } => {
            for element in elements {
                visit_pattern(element, role, visitor);
            }
        }

        Pattern::Slice { prefix, rest, .. } => {
            for p in prefix {
                visit_pattern(p, role, visitor);
            }
            if let RestPattern::Bind { .. } = rest {
                // rest binding is not a pattern itself
            }
        }

        Pattern::Or { patterns, .. } => {
            for p in patterns {
                visit_pattern(p, role, visitor);
            }
        }

        Pattern::AsBinding { pattern, .. } => {
            visit_pattern(pattern, role, visitor);
        }
    }
}
