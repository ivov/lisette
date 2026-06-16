use std::cell::RefCell;

use diagnostics::LocalSink;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use syntax::ast::{
    Expression, ExpressionKind, FormatStringPart, Literal, MatchArm, Pattern, PatternKind,
    RestPattern, SelectArm, SelectArmPattern, Span,
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
        Expression::Break {
            value: Some(val), ..
        } => {
            visit_node(val, expression_visitor, pattern_visitor);
        }

        Expression::Literal {
            literal: Literal::Slice(elements),
            ..
        } => {
            for element in elements {
                visit_node(element, expression_visitor, pattern_visitor);
            }
        }

        Expression::Literal {
            literal: Literal::FormatString(parts),
            ..
        } => {
            for part in parts {
                if let FormatStringPart::Expression(expression) = part {
                    visit_node(expression, expression_visitor, pattern_visitor);
                }
            }
        }

        Expression::Interface {
            method_signatures, ..
        } => {
            for signature in method_signatures {
                visit_node(signature, expression_visitor, pattern_visitor);
            }
        }

        Expression::Literal { .. }
        | Expression::Identifier { .. }
        | Expression::Unit { .. }
        | Expression::Break { value: None, .. }
        | Expression::Continue { .. }
        | Expression::RawGo { .. }
        | Expression::NoOp
        | Expression::Enum { .. }
        | Expression::Struct { .. }
        | Expression::TypeAlias { .. }
        | Expression::VariableDeclaration { .. }
        | Expression::ModuleImport { .. } => {}

        Expression::Function { params, body, .. } => {
            for param in params {
                visit_pattern(&param.pattern, pattern_visitor);
            }
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::Lambda { params, body, .. } => {
            for param in params {
                visit_pattern(&param.pattern, pattern_visitor);
            }
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => {
            for item in items {
                visit_node(item, expression_visitor, pattern_visitor);
            }
        }

        Expression::Let {
            binding,
            value,
            else_block,
            ..
        } => {
            visit_pattern(&binding.pattern, pattern_visitor);
            visit_node(value, expression_visitor, pattern_visitor);
            if let Some(else_block) = else_block {
                visit_node(else_block, expression_visitor, pattern_visitor);
            }
        }

        Expression::Call {
            expression,
            args,
            spread,
            ..
        } => {
            visit_node(expression, expression_visitor, pattern_visitor);
            for arg in args {
                visit_node(arg, expression_visitor, pattern_visitor);
            }
            if let Some(spread_expr) = spread.as_ref() {
                visit_node(spread_expr, expression_visitor, pattern_visitor);
            }
        }

        Expression::If {
            condition,
            consequence,
            alternative,
            ..
        } => {
            visit_node(condition, expression_visitor, pattern_visitor);
            visit_node(consequence, expression_visitor, pattern_visitor);
            visit_node(alternative, expression_visitor, pattern_visitor);
        }

        Expression::IfLet {
            pattern,
            scrutinee,
            consequence,
            alternative,
            ..
        } => {
            visit_pattern(pattern, pattern_visitor);
            visit_node(scrutinee, expression_visitor, pattern_visitor);
            visit_node(consequence, expression_visitor, pattern_visitor);
            visit_node(alternative, expression_visitor, pattern_visitor);
        }

        Expression::Match { subject, arms, .. } => {
            visit_node(subject, expression_visitor, pattern_visitor);
            for arm in arms {
                visit_match_arm(arm, expression_visitor, pattern_visitor);
            }
        }

        Expression::Tuple { elements, .. } => {
            for element in elements {
                visit_node(element, expression_visitor, pattern_visitor);
            }
        }

        Expression::StructCall {
            field_assignments,
            spread,
            ..
        } => {
            for assignment in field_assignments {
                visit_node(&assignment.value, expression_visitor, pattern_visitor);
            }
            if let Some(spread_expression) = spread.as_expression() {
                visit_node(spread_expression, expression_visitor, pattern_visitor);
            }
        }

        Expression::DotAccess { expression, .. }
        | Expression::Return { expression, .. }
        | Expression::Propagate { expression, .. }
        | Expression::Unary { expression, .. }
        | Expression::Paren { expression, .. }
        | Expression::Reference { expression, .. }
        | Expression::Task { expression, .. }
        | Expression::Defer { expression, .. } => {
            visit_node(expression, expression_visitor, pattern_visitor);
        }

        Expression::Const { expression, .. } => {
            visit_node(expression, expression_visitor, pattern_visitor);
        }

        Expression::Assignment { target, value, .. } => {
            visit_node(target, expression_visitor, pattern_visitor);
            visit_node(value, expression_visitor, pattern_visitor);
        }

        Expression::Binary { left, right, .. } => {
            visit_node(left, expression_visitor, pattern_visitor);
            visit_node(right, expression_visitor, pattern_visitor);
        }

        Expression::ImplBlock { methods, .. } => {
            for method in methods {
                visit_node(method, expression_visitor, pattern_visitor);
            }
        }

        Expression::Loop { body, .. } => {
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::While {
            condition, body, ..
        } => {
            visit_node(condition, expression_visitor, pattern_visitor);
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::WhileLet {
            pattern,
            scrutinee,
            body,
            ..
        } => {
            visit_pattern(pattern, pattern_visitor);
            visit_node(scrutinee, expression_visitor, pattern_visitor);
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::For {
            binding,
            iterable,
            body,
            ..
        } => {
            visit_pattern(&binding.pattern, pattern_visitor);
            visit_node(iterable, expression_visitor, pattern_visitor);
            visit_node(body, expression_visitor, pattern_visitor);
        }

        Expression::IndexedAccess {
            expression, index, ..
        } => {
            visit_node(expression, expression_visitor, pattern_visitor);
            visit_node(index, expression_visitor, pattern_visitor);
        }

        Expression::Select { arms, .. } => {
            for arm in arms {
                visit_select_arm(arm, expression_visitor, pattern_visitor);
            }
        }

        Expression::Range { start, end, .. } => {
            if let Some(start) = start {
                visit_node(start, expression_visitor, pattern_visitor);
            }
            if let Some(end) = end {
                visit_node(end, expression_visitor, pattern_visitor);
            }
        }

        Expression::Cast { expression, .. } => {
            visit_node(expression, expression_visitor, pattern_visitor);
        }
    }
}

fn visit_match_arm<E, P>(arm: &MatchArm, expression_visitor: &mut E, pattern_visitor: &mut P)
where
    E: FnMut(&Expression),
    P: FnMut(&Pattern),
{
    visit_pattern(&arm.pattern, pattern_visitor);
    if let Some(guard) = &arm.guard {
        visit_node(guard, expression_visitor, pattern_visitor);
    }
    visit_node(&arm.expression, expression_visitor, pattern_visitor);
}

fn visit_select_arm<E, P>(arm: &SelectArm, expression_visitor: &mut E, pattern_visitor: &mut P)
where
    E: FnMut(&Expression),
    P: FnMut(&Pattern),
{
    match &arm.pattern {
        SelectArmPattern::Receive {
            receive_expression,
            body,
            ..
        } => {
            visit_node(receive_expression, expression_visitor, pattern_visitor);
            visit_node(body, expression_visitor, pattern_visitor);
        }
        SelectArmPattern::Send {
            send_expression,
            body,
        } => {
            visit_node(send_expression, expression_visitor, pattern_visitor);
            visit_node(body, expression_visitor, pattern_visitor);
        }
        SelectArmPattern::MatchReceive {
            receive_expression,
            arms,
        } => {
            visit_node(receive_expression, expression_visitor, pattern_visitor);
            for match_arm in arms {
                visit_match_arm(match_arm, expression_visitor, pattern_visitor);
            }
        }
        SelectArmPattern::WildCard { body } => {
            visit_node(body, expression_visitor, pattern_visitor);
        }
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
