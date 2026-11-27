mod aliases;
mod compound_assignments;
pub(crate) mod name_uses;
mod search_loops;
pub(crate) mod terminal_else;
mod unread_temps;

use aliases::{inline_name_aliases, inline_return_aliases};
use compound_assignments::fold_compound_assignments;
use search_loops::return_found_elements_directly;
use terminal_else::{unwrap_scoped_terminal_else, unwrap_terminal_else};
use unread_temps::drop_unread_temps;

use std::mem;

use rustc_hash::FxHashSet;

use crate::lookup::facts::package_names::PackageNames;
use crate::lookup::state::scope::ScopeState;
use crate::output::finish::hygiene::drop_unused_locals;
use crate::output::finish::source_reuse::reuse_dead_copy_sources;
use crate::target::ir::bodies::{
    Definition, ElseArm, IfPlan, LoweredStatement, Statement, for_each_statements_mut,
};
use crate::target::ir::go_expression::GoExpressionNode;
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::types::go_type::GoType;

pub(crate) fn clean_up(
    statements: &mut Vec<Statement>,
    shadowing: &FxHashSet<LocalId>,
    parameters: &[GoIdentifier],
    scope: &ScopeState,
) {
    reuse_dead_copy_sources(statements, parameters, scope);
    inline_name_aliases(statements);
    inline_return_aliases(statements);
    drop_unread_temps(statements, shadowing);
    fold_compound_assignments(statements);
    return_found_elements_directly(statements);
    unwrap_terminal_else(statements);
    unwrap_scoped_terminal_else(statements, parameters, scope);
}

pub(crate) fn drop_unreachable_statements(statements: &mut Vec<Statement>) {
    let mut dropped = false;
    for_each_statements_mut(statements, &mut |list| {
        let Some(end) = list
            .iter()
            .position(|statement| statement.kind.blocks_fallthrough())
        else {
            return;
        };

        let terminated = matches!(
            list.get(end + 1).map(|statement| &statement.kind),
            Some(LoweredStatement::UnreachablePanic)
        );
        let kept = end + 1 + usize::from(terminated);
        dropped |= kept < list.len();
        list.truncate(kept);
    });
    if dropped {
        drop_unused_locals(statements);
    }
}

pub(crate) fn declare_zero_values(
    statements: &mut Vec<Statement>,
    scope: &mut ScopeState,
    package: &PackageNames,
) {
    for_each_statements_mut(statements, &mut |block| {
        for mut statement in mem::take(block) {
            if let LoweredStatement::Define(Definition { names, value }) = &statement.kind
                && let [name] = names.as_slice()
                && let Some(go_type) = value.node().new_zero_type().cloned()
            {
                statement.kind = LoweredStatement::VarDeclaration {
                    name: name.clone(),
                    go_type,
                    value: None,
                };
            }
            let values: Vec<&mut GoExpression> = match &mut statement.kind {
                LoweredStatement::Return(values) => values.iter_mut().collect(),
                LoweredStatement::Define(definition) => vec![&mut definition.value],
                LoweredStatement::If(plan) => if_initializer_values(plan),
                LoweredStatement::Loop(_)
                | LoweredStatement::Block(_)
                | LoweredStatement::Body(_)
                | LoweredStatement::Break(_)
                | LoweredStatement::Continue(_)
                | LoweredStatement::Const(_)
                | LoweredStatement::Assign(_)
                | LoweredStatement::Async { .. }
                | LoweredStatement::Select(_)
                | LoweredStatement::Switch(_)
                | LoweredStatement::VarDeclaration { .. }
                | LoweredStatement::Discard { .. }
                | LoweredStatement::ExpressionStatement { .. }
                | LoweredStatement::UnreachablePanic => Vec::new(),
            };

            let mut zeros: Vec<(GoType, GoIdentifier)> = Vec::new();
            for value in values {
                name_zero_values(value.node_mut(), &mut zeros, scope, package);
            }
            for (go_type, name) in zeros {
                block.push(Statement {
                    line: statement.line.clone(),
                    kind: LoweredStatement::VarDeclaration {
                        name,
                        go_type,
                        value: None,
                    },
                });
            }
            block.push(statement);
        }
    });
}

fn if_initializer_values(plan: &mut IfPlan) -> Vec<&mut GoExpression> {
    let mut values = Vec::new();
    let mut link = Some(plan);
    while let Some(IfPlan {
        initializer,
        else_arm,
        ..
    }) = link
    {
        if let Some(initializer) = initializer {
            values.push(&mut initializer.value);
        }
        link = match else_arm {
            ElseArm::ElseIf(next) => Some(next.as_mut()),
            ElseArm::None | ElseArm::Else { .. } => None,
        };
    }
    values
}

fn name_zero_values(
    node: &mut GoExpressionNode,
    zeros: &mut Vec<(GoType, GoIdentifier)>,
    scope: &mut ScopeState,
    package: &PackageNames,
) {
    if let Some(go_type) = node.new_zero_type().cloned() {
        let name = match zeros.iter().find(|(declared, _)| *declared == go_type) {
            Some((_, name)) => name.clone(),
            None => {
                let name = scope.fresh_go_name(Some("zero"), package);
                zeros.push((go_type, name.clone()));
                name
            }
        };

        *node = GoExpressionNode::Identifier(name);
        return;
    }
    if matches!(node, GoExpressionNode::FunctionLiteral { .. }) {
        return;
    }
    node.visit_children_mut(&mut |child| name_zero_values(child, zeros, scope, package));
}
