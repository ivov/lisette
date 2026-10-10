use crate::plan::bodies::{
    AssignForm, LoopTransfer, LoweredBlock, LoweredStatement, Statement, for_each_statements_mut,
};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::values::GoExpression;
use crate::plan::visit::{VisitorMut, visit_statements_mut};
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

/// Go does not order a panic outside a call against a later call.
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
                LoweredStatement::Discard(value) => vec![value],
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
    bindings: &[&GoIdentifier],
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
        check.local_binding(&mut (*name).clone());
    }
    visit_statements_mut(statements, &mut check);
    check.error.map_or(Ok(()), |kind| Err(BodyError { kind }))
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::plan::bodies::{ElseArm, IfPlan, LoopId};
    use crate::plan::values::GoExpression;

    fn block(statements: Vec<Statement>) -> LoweredBlock {
        LoweredBlock { statements }
    }

    #[test]
    fn source_transfer_must_be_resolved_even_when_nested() {
        let mut body = block(vec![
            LoweredStatement::If(IfPlan::plain(
                GoExpression::literal("true".into()),
                block(vec![
                    LoweredStatement::Break(LoopTransfer::Source(LoopId(0))).into(),
                ]),
                ElseArm::None,
            ))
            .into(),
        ]);
        assert_eq!(
            verify_final_function_body(&mut body, false)
                .unwrap_err()
                .kind,
            BodyErrorKind::UnresolvedLoopTarget
        );
    }

    #[test]
    fn result_body_requires_go_termination() {
        let mut body = block(vec![
            LoweredStatement::ExpressionStatement {
                expression: GoExpression::call(GoExpression::name("fail".into()), vec![]),
                diverges: true,
            }
            .into(),
        ]);
        assert_eq!(
            verify_final_function_body(&mut body, true)
                .unwrap_err()
                .kind,
            BodyErrorKind::MissingGoTermination
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::bodies::{define, expression_statement};

    #[test]
    fn a_panic_cannot_stay_inline_before_a_later_call() {
        let index = GoExpression::index(
            GoExpression::name("xs".to_string()),
            GoExpression::name("i".to_string()),
        );
        let call = GoExpression::call(GoExpression::name("bump".to_string()), Vec::new());
        assert_eq!(
            verify_operand_order(&[index.clone(), call.clone()])
                .unwrap_err()
                .kind,
            BodyErrorKind::UnorderedOperand
        );
        assert!(verify_operand_order(&[call, index]).is_ok());
    }

    #[test]
    fn an_empty_expression_cannot_stand_for_a_value() {
        let call = GoExpression::call(
            GoExpression::name("show".to_string()),
            vec![GoExpression::empty()],
        );
        let mut body = LoweredBlock {
            statements: vec![expression_statement(call)],
        };
        assert_eq!(
            verify_final_function_body(&mut body, false)
                .unwrap_err()
                .kind,
            BodyErrorKind::MissingValue
        );

        let mut body = LoweredBlock {
            statements: vec![define("x".to_string(), GoExpression::empty())],
        };
        assert_eq!(
            verify_final_function_body(&mut body, false)
                .unwrap_err()
                .kind,
            BodyErrorKind::MissingValue
        );

        let mut body = LoweredBlock {
            statements: vec![expression_statement(GoExpression::empty())],
        };
        assert!(verify_final_function_body(&mut body, false).is_ok());
    }
}
