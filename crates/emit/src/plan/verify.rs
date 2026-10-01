use crate::plan::bodies::LoweredStatement;
use crate::plan::go_expression::GoExpressionNode;
use crate::plan::local::{GoIdentifier, LocalId};
use crate::plan::visit::{VisitorMut, visit_statements_mut};
use rustc_hash::FxHashMap as HashMap;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyErrorKind {
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
