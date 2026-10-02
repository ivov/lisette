use crate::plan::bodies::{
    ElseArm, LoopTransfer, LoweredBlock, LoweredStatement, for_each_statement,
};
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::visit::{VisitorMut, visit_statements_mut};
use rustc_hash::FxHashMap as HashMap;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyErrorKind {
    EmptyIfCondition,
    ElseIfHasSetup,
    MissingGoTermination,
    UnresolvedLoopTarget,
    ConflictingLocalSpelling,
    ShadowedLocalReference,
    UnboundLocalReference,
    UnidentifiedLocal,
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

pub(crate) fn verify_control_structure(statements: &[LoweredStatement]) -> Result<(), BodyError> {
    let mut error = None;
    for_each_statement(statements, &mut |statement| {
        if error.is_some() {
            return;
        }
        let kind = match statement {
            LoweredStatement::If(plan) if plan.condition.is_empty() => {
                Some(BodyErrorKind::EmptyIfCondition)
            }
            LoweredStatement::If(plan) if matches!(&plan.else_arm, ElseArm::ElseIf(inner) if !inner.condition_setup.is_empty()) => {
                Some(BodyErrorKind::ElseIfHasSetup)
            }
            LoweredStatement::Break(LoopTransfer::Source(_))
            | LoweredStatement::Continue(LoopTransfer::Source(_)) => {
                Some(BodyErrorKind::UnresolvedLoopTarget)
            }
            _ => None,
        };
        error = kind.or(error);
    });
    error.map_or(Ok(()), |kind| Err(BodyError { kind }))
}

pub(crate) fn verify_final_function_body(
    body: &LoweredBlock,
    has_result: bool,
) -> Result<(), BodyError> {
    verify_control_structure(&body.statements)?;
    if has_result && !body.go_terminates() {
        return Err(BodyError {
            kind: BodyErrorKind::MissingGoTermination,
        });
    }
    Ok(())
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::plan::bodies::{IfPlan, LoopId};
    use crate::plan::values::GoExpression;

    fn block(statements: Vec<LoweredStatement>) -> LoweredBlock {
        LoweredBlock { statements }
    }

    #[test]
    fn source_transfer_must_be_resolved_even_when_nested() {
        let body = block(vec![LoweredStatement::If(IfPlan::plain(
            GoExpression::literal("true".into()),
            block(vec![LoweredStatement::Break(LoopTransfer::Source(LoopId(
                0,
            )))]),
            ElseArm::None,
        ))]);
        assert_eq!(
            verify_final_function_body(&body, false).unwrap_err().kind,
            BodyErrorKind::UnresolvedLoopTarget
        );
    }

    #[test]
    fn else_if_setup_must_be_nested_before_rendering() {
        let mut inner = IfPlan::plain(
            GoExpression::literal("true".into()),
            block(vec![]),
            ElseArm::None,
        );
        inner
            .condition_setup
            .push(LoweredStatement::UnreachablePanic);
        let body = block(vec![LoweredStatement::If(IfPlan::plain(
            GoExpression::literal("false".into()),
            block(vec![]),
            ElseArm::ElseIf(Box::new(inner)),
        ))]);
        assert_eq!(
            verify_final_function_body(&body, false).unwrap_err().kind,
            BodyErrorKind::ElseIfHasSetup
        );
    }

    #[test]
    fn result_body_requires_go_termination() {
        let body = block(vec![LoweredStatement::ExpressionStatement {
            expression: GoExpression::call(GoExpression::name("fail".into()), vec![]),
            diverges: true,
        }]);
        assert_eq!(
            verify_final_function_body(&body, true).unwrap_err().kind,
            BodyErrorKind::MissingGoTermination
        );
    }
}

pub(crate) fn verify_local_scopes(
    statements: &mut [LoweredStatement],
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

        fn binding(&mut self, _name: &mut String) {}

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
