use crate::ir::bodies::{
    AssignForm, LoopTransfer, LoweredBlock, LoweredStatement, Statement, for_each_statements_mut,
};
use crate::ir::go_expression::GoExpressionNode;
use crate::ir::local::{GoIdentifier, LocalId};
use crate::ir::values::GoExpression;
use crate::ir::visit::{VisitorMut, visit_statements_mut};
use rustc_hash::FxHashMap as HashMap;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyErrorKind {
    EmptyIfCondition,
    MissingGoTermination,
    UnresolvedLoopTarget,
    ConflictingLocalSpelling,
    ShadowedLocalReference,
    UnboundLocalReference,
    UnidentifiedLocal,
    UnorderedOperand,
    MissingValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyError {
    pub(crate) kind: BodyErrorKind,
}

impl Display for BodyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "invalid lowered Go body: {:?}", self.kind)
    }
}

pub(crate) fn verify_control_structure(statements: &mut Vec<Statement>) -> Result<(), BodyError> {
    let mut error = None;
    for_each_statements_mut(statements, &mut |list| {
        for statement in list.iter() {
            if error.is_some() {
                return;
            }
            let kind = match &statement.kind {
                LoweredStatement::If(plan) if plan.condition.is_empty() => {
                    Some(BodyErrorKind::EmptyIfCondition)
                }
                LoweredStatement::Break(LoopTransfer::Source(_))
                | LoweredStatement::Continue(LoopTransfer::Source(_)) => {
                    Some(BodyErrorKind::UnresolvedLoopTarget)
                }
                _ => None,
            };
            error = kind.or(error);
        }
    });
    error.map_or(Ok(()), |kind| Err(BodyError { kind }))
}

pub(crate) fn verify_operand_order(values: &[GoExpression]) -> Result<(), BodyError> {
    for (index, value) in values.iter().enumerate() {
        if value.effects().panics_or_blocks()
            && values[index + 1..]
                .iter()
                .any(|later| later.effects().runs_effectful_code())
        {
            return Err(BodyError {
                kind: BodyErrorKind::UnorderedOperand,
            });
        }
    }
    Ok(())
}

pub(crate) fn verify_final_function_body(
    body: &mut LoweredBlock,
    has_result: bool,
) -> Result<(), BodyError> {
    verify_control_structure(&mut body.statements)?;
    verify_values(&mut body.statements)?;
    if has_result && !body.go_terminates() {
        return Err(BodyError {
            kind: BodyErrorKind::MissingGoTermination,
        });
    }
    Ok(())
}

fn verify_values(statements: &mut Vec<Statement>) -> Result<(), BodyError> {
    let mut missing = false;
    for statement in statements.iter() {
        statement.kind.visit_expressions(&mut |node| {
            node.visit_children(&mut |child| {
                missing |= matches!(child, GoExpressionNode::Empty);
            });
        });
    }
    for_each_statements_mut(statements, &mut |list| {
        for statement in list.iter() {
            let values: Vec<&GoExpression> = match &statement.kind {
                LoweredStatement::Define(definition) => vec![&definition.value],
                LoweredStatement::Discard { value, .. } => vec![value],
                LoweredStatement::Return(values) => values.iter().collect(),
                LoweredStatement::Assign(AssignForm::Simple { value, .. }) => vec![value],
                _ => Vec::new(),
            };
            missing |= values
                .iter()
                .any(|value| matches!(value.node(), GoExpressionNode::Empty));
        }
    });
    if missing {
        return Err(BodyError {
            kind: BodyErrorKind::MissingValue,
        });
    }
    Ok(())
}

pub(crate) fn verify_local_scopes(
    statements: &mut Vec<Statement>,
    bindings: &[GoIdentifier],
) -> Result<(), BodyError> {
    struct Check {
        frames: Vec<HashMap<String, LocalId>>,
        spellings: HashMap<LocalId, String>,
        error: Option<BodyErrorKind>,
    }

    impl Check {
        fn check_spelling(&mut self, name: &GoIdentifier) {
            let Some(id) = name.id() else {
                return;
            };
            match self.spellings.get(&id) {
                Some(previous) if previous != name.spelling() => {
                    self.error
                        .get_or_insert(BodyErrorKind::ConflictingLocalSpelling);
                }
                None => {
                    self.spellings.insert(id, name.spelling().to_string());
                }
                _ => {}
            }
        }
    }

    impl VisitorMut for Check {
        fn expression(&mut self, node: &mut GoExpressionNode) {
            let GoExpressionNode::Identifier(name) = node else {
                return;
            };
            if name.is_pending() {
                self.error.get_or_insert(BodyErrorKind::UnidentifiedLocal);
                return;
            }
            self.check_spelling(name);
            let active = self
                .frames
                .iter()
                .rev()
                .find_map(|frame| frame.get(name.spelling()));
            if let Some(active) = active {
                if name.id().is_none() {
                    self.error.get_or_insert(BodyErrorKind::UnidentifiedLocal);
                } else if name.id() != Some(*active) {
                    self.error
                        .get_or_insert(BodyErrorKind::ShadowedLocalReference);
                }
            } else if let Some(id) = name.id()
                && !self
                    .frames
                    .iter()
                    .any(|frame| frame.values().any(|active| *active == id))
            {
                self.error
                    .get_or_insert(BodyErrorKind::UnboundLocalReference);
            }
        }

        fn local_binding(&mut self, name: &mut GoIdentifier) {
            if name.spelling() == "_" {
                return;
            }
            let Some(id) = name.id() else {
                self.error.get_or_insert(BodyErrorKind::UnidentifiedLocal);
                return;
            };
            self.check_spelling(name);
            self.frames
                .last_mut()
                .expect("body has a scope")
                .insert(name.spelling().to_string(), id);
        }

        fn enter_scope(&mut self) {
            self.frames.push(HashMap::default());
        }

        fn exit_scope(&mut self) {
            self.frames.pop().expect("body has a scope");
        }
    }

    let mut check = Check {
        frames: vec![HashMap::default()],
        spellings: HashMap::default(),
        error: None,
    };
    for name in bindings {
        check.local_binding(&mut name.clone());
    }
    visit_statements_mut(statements, &mut check);
    check.error.map_or(Ok(()), |kind| Err(BodyError { kind }))
}
