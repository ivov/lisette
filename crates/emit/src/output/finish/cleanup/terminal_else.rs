use crate::lookup::state::scope_stack::ScopeStack;
use crate::target::ir::go_expression::NameRead;
use std::mem;

use rustc_hash::FxHashMap;

use crate::lookup::state::scope::{ScopeState, ScopedTypeParameter};
use crate::target::ir::bodies::{
    ElseArm, ElseLayout, IfOrigin, IfPlan, LoweredBlock, LoweredStatement, Statement,
    for_each_statements_mut, scope_statements,
};
use crate::target::ir::go_expression::{
    GoBinaryOperator, GoExpressionNode, GoUnaryOperator, Operand, verbatim_identifiers,
};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};

pub(super) fn unwrap_terminal_else(statements: &mut Vec<Statement>) {
    for_each_statements_mut(statements, &mut |block| {
        for statement in block.iter_mut() {
            unwrap_terminal_else_of(&mut statement.kind);
        }
    });
}

fn unwrap_terminal_else_of(statement: &mut LoweredStatement) {
    let mut plan = if let LoweredStatement::If(plan) = statement {
        plan
    } else {
        return;
    };
    let mut then_diverges = true;
    let mut keeps_scope = true;
    loop {
        then_diverges &= plan.then_body.ends_with_diverge();
        keeps_scope &= plan.initializer.is_none();
        match &mut plan.else_arm {
            ElseArm::ElseIf(inner) => plan = inner,
            ElseArm::Else { body, layout } => {
                if then_diverges && keeps_scope && declares_no_names(body) {
                    *layout = ElseLayout::Inline;
                }
                return;
            }
            ElseArm::None => return,
        }
    }
}

fn declares_no_names(body: &LoweredBlock) -> bool {
    scope_statements(&body.statements)
        .into_iter()
        .all(|statement| {
            !matches!(
                statement,
                LoweredStatement::Define(_)
                    | LoweredStatement::VarDeclaration { .. }
                    | LoweredStatement::Const(_)
                    | LoweredStatement::Select(_)
                    | LoweredStatement::Switch(_)
                    | LoweredStatement::Loop(_)
                    | LoweredStatement::If(_)
            )
        })
}

pub(super) fn unwrap_scoped_terminal_else(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
    scope: &ScopeState,
) {
    let mut skipped = 0;
    let mut candidates = count_scoped_terminal_else(statements);
    let mut original: Option<Resolution> = None;
    'candidates: loop {
        if skipped >= candidates {
            return;
        }
        let mut flipped = statements.clone();
        let Some(flips) = rewrite_nth_scoped_terminal_else(
            &mut flipped,
            skipped,
            Rewrite::Flip,
            scope.type_params(),
        ) else {
            return;
        };

        let baseline = if flips {
            Resolution::of(&mut flipped, parameters)
        } else {
            original
                .get_or_insert_with(|| Resolution::of(statements, parameters))
                .clone()
        };

        for hoist in [false, true] {
            let mut rewritten = statements.clone();
            rewrite_nth_scoped_terminal_else(
                &mut rewritten,
                skipped,
                Rewrite::Inline { hoist },
                scope.type_params(),
            );
            if Resolution::of(&mut rewritten, parameters) == baseline {
                *statements = rewritten;
                candidates = count_scoped_terminal_else(statements);
                original = None;
                continue 'candidates;
            }
        }
        skipped += 1;
    }
}

fn count_scoped_terminal_else(statements: &mut Vec<Statement>) -> usize {
    let mut count = 0;
    for_each_statements_mut(statements, &mut |block| {
        count += block
            .iter()
            .filter(|statement| matches!(&statement.kind, LoweredStatement::If(plan) if is_candidate(plan)))
            .count();
    });
    count
}

#[derive(Clone, Copy)]
enum Rewrite {
    Flip,
    Inline { hoist: bool },
}

fn rewrite_nth_scoped_terminal_else(
    statements: &mut Vec<Statement>,
    target: usize,
    rewrite: Rewrite,
    type_params: &[ScopedTypeParameter],
) -> Option<bool> {
    let mut seen = 0;
    let mut found = None;
    for_each_statements_mut(statements, &mut |block| {
        if found.is_some() {
            return;
        }
        for index in 0..block.len() {
            let LoweredStatement::If(plan) = &mut block[index].kind else {
                continue;
            };
            if !is_candidate(plan) {
                continue;
            }
            let flips = flips_to_early_exit(plan);
            if seen < target {
                seen += 1;
                continue;
            }
            found = Some(flips);
            if flips {
                flip_branches(plan);
            }
            let Rewrite::Inline { hoist } = rewrite else {
                return;
            };
            inline_terminal_else(plan);
            let shadows_a_type_param = plan.initializer.as_ref().is_some_and(|definition| {
                definition.names.iter().any(|name| {
                    type_params
                        .iter()
                        .any(|param| param.go_name == name.spelling())
                })
            });

            if hoist
                && !shadows_a_type_param
                && let Some(definition) = plan.initializer.take()
            {
                let line = block[index].line.clone();
                block.insert(
                    index,
                    Statement {
                        line,
                        kind: LoweredStatement::Define(definition),
                    },
                );
            }
            return;
        }
    });
    found
}

fn is_candidate(plan: &IfPlan) -> bool {
    flips_to_early_exit(plan) || has_scoped_terminal_else(plan)
}

fn has_scoped_terminal_else(plan: &IfPlan) -> bool {
    let outer_initializer = plan.initializer.is_some();
    let mut plan = plan;
    let mut first = true;
    loop {
        if !plan.then_body.ends_with_diverge() || (!first && plan.initializer.is_some()) {
            return false;
        }
        first = false;
        match &plan.else_arm {
            ElseArm::ElseIf(inner) => plan = inner,
            ElseArm::Else {
                body,
                layout: ElseLayout::Block,
            } => return outer_initializer || !declares_no_names(body),
            ElseArm::Else {
                layout: ElseLayout::Inline,
                ..
            }
            | ElseArm::None => return false,
        }
    }
}

fn flips_to_early_exit(plan: &IfPlan) -> bool {
    let ElseArm::Else {
        body,
        layout: ElseLayout::Block,
    } = &plan.else_arm
    else {
        return false;
    };

    (plan.initializer.is_some() || matches!(plan.origin, IfOrigin::Match { .. }))
        && !plan.then_body.ends_with_diverge()
        && !plan.then_body.renders_empty()
        && body.ends_with_diverge()
        && negated(plan.condition.node()).is_some()
}

pub(crate) fn flip_branches(plan: &mut IfPlan) {
    let else_label = match &mut plan.origin {
        IfOrigin::Match { else_label } => else_label.take(),
        IfOrigin::User => None,
    };
    plan.condition = match (else_label, plan.condition.node()) {
        (
            Some(label),
            GoExpressionNode::Binary {
                operator: GoBinaryOperator::Equal,
                left,
                ..
            },
        ) => plan.condition.with_node(GoExpressionNode::Binary {
            operator: GoBinaryOperator::Equal,
            left: left.clone(),
            right: label.into_operand(),
        }),
        _ => plan
            .condition
            .peeled(&negated(plan.condition.node()).expect("a flipped condition negates")),
    };
    let ElseArm::Else { body, .. } = &mut plan.else_arm else {
        unreachable!("a flipped if has an else");
    };
    mem::swap(&mut plan.then_body, body);
}

fn negated(node: &GoExpressionNode) -> Option<Operand> {
    let negation = match node {
        GoExpressionNode::Binary {
            operator,
            left,
            right,
        } => {
            let (operator, left, right) = match operator {
                GoBinaryOperator::Equal => {
                    (GoBinaryOperator::NotEqual, left.clone(), right.clone())
                }
                GoBinaryOperator::NotEqual => {
                    (GoBinaryOperator::Equal, left.clone(), right.clone())
                }
                GoBinaryOperator::And => (
                    GoBinaryOperator::Or,
                    negated_in_place(left)?,
                    negated_in_place(right)?,
                ),
                GoBinaryOperator::Or => (
                    GoBinaryOperator::And,
                    negated_in_place(left)?,
                    negated_in_place(right)?,
                ),
                GoBinaryOperator::Addition
                | GoBinaryOperator::Subtraction
                | GoBinaryOperator::Multiplication
                | GoBinaryOperator::Division
                | GoBinaryOperator::Remainder
                | GoBinaryOperator::BitwiseAnd
                | GoBinaryOperator::BitwiseOr
                | GoBinaryOperator::BitwiseXor
                | GoBinaryOperator::BitwiseAndNot
                | GoBinaryOperator::ShiftLeft
                | GoBinaryOperator::ShiftRight
                | GoBinaryOperator::LessThan
                | GoBinaryOperator::LessThanOrEqual
                | GoBinaryOperator::GreaterThan
                | GoBinaryOperator::GreaterThanOrEqual => return None,
            };

            GoExpressionNode::Binary {
                operator,
                left,
                right,
            }
        }
        GoExpressionNode::Unary {
            operator: GoUnaryOperator::Not,
            operand,
        } => return Some(operand.clone()),
        GoExpressionNode::Identifier(_) | GoExpressionNode::Call { .. } => {
            GoExpressionNode::Unary {
                operator: GoUnaryOperator::Not,
                operand: node.clone().into(),
            }
        }
        GoExpressionNode::Qualified { .. }
        | GoExpressionNode::Literal(_)
        | GoExpressionNode::Type(_)
        | GoExpressionNode::CompositeLiteral { .. }
        | GoExpressionNode::Instantiation { .. }
        | GoExpressionNode::Selector { .. }
        | GoExpressionNode::Index { .. }
        | GoExpressionNode::Slice { .. }
        | GoExpressionNode::TypeAssertion { .. }
        | GoExpressionNode::Unary { .. }
        | GoExpressionNode::AddressOf(_)
        | GoExpressionNode::Dereference(_)
        | GoExpressionNode::Conversion { .. }
        | GoExpressionNode::Spread(_)
        | GoExpressionNode::FunctionLiteral { .. }
        | GoExpressionNode::Verbatim(_)
        | GoExpressionNode::Unreachable => return None,
    };

    Some(negation.into())
}

fn negated_in_place(operand: &Operand) -> Option<Operand> {
    let mut negation = negated(operand)?;
    if !negation.position().is_set() {
        *negation.position_mut() = operand.position().clone();
    }
    Some(negation)
}

fn inline_terminal_else(plan: &mut IfPlan) {
    let mut arm = &mut plan.else_arm;
    loop {
        match arm {
            ElseArm::ElseIf(inner) => arm = &mut inner.else_arm,
            ElseArm::Else { layout, .. } => {
                *layout = ElseLayout::Inline;
                return;
            }
            ElseArm::None => unreachable!("scoped terminal else ends in an else"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Resolution {
    reads: Vec<Option<usize>>,
    reuses: Vec<usize>,
}

struct Resolve {
    frames: ScopeStack<FxHashMap<String, usize>>,
    bindings: usize,
    resolution: Resolution,
}

impl Resolution {
    fn of(statements: &mut Vec<Statement>, parameters: &[GoIdentifier]) -> Self {
        let mut resolve = Resolve {
            frames: ScopeStack::new(FxHashMap::default()),
            bindings: 0,
            resolution: Resolution {
                reads: Vec::new(),
                reuses: Vec::new(),
            },
        };

        for parameter in parameters {
            resolve.bind(parameter.spelling());
        }
        visit_statements_mut(statements, &mut resolve);
        resolve.resolution
    }
}

impl Resolve {
    fn bind(&mut self, spelling: &str) {
        if spelling == "_" {
            return;
        }
        let frame = self.frames.innermost_mut();
        if let Some(existing) = frame.get(spelling) {
            self.resolution.reuses.push(*existing);
            return;
        }
        frame.insert(spelling.to_string(), self.bindings);
        self.bindings += 1;
    }

    fn read(&mut self, spelling: &str) {
        let binding = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(spelling).copied());
        self.resolution.reads.push(binding);
    }
}

impl VisitorMut for Resolve {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        match node.name_read() {
            NameRead::Local(name) => self.read(name.spelling()),
            NameRead::Verbatim(source) => {
                for spelling in verbatim_identifiers(source) {
                    self.read(spelling);
                    self.bind(spelling);
                }
            }
            NameRead::NoName => {}
        }
    }

    fn local_binding(&mut self, name: &mut GoIdentifier, _site: BindingSite) {
        self.bind(name.spelling());
    }

    fn enter_scope(&mut self) {
        self.frames.push(FxHashMap::default());
    }

    fn exit_scope(&mut self) {
        self.frames.pop();
    }
}

pub(crate) fn drop_empty_else(plan: &mut IfPlan) {
    match &mut plan.else_arm {
        ElseArm::Else { body, .. } if body.renders_empty() => plan.else_arm = ElseArm::None,
        ElseArm::ElseIf(next) => drop_empty_else(next),
        ElseArm::None | ElseArm::Else { .. } => {}
    }
}
