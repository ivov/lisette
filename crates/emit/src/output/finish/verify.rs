use crate::lookup::state::scope_stack::ScopeStack;
use crate::target::ir::bodies::{
    AssignForm, LoopTransfer, LoweredBlock, LoweredStatement, Statement, for_each_statements_mut,
};
use crate::target::ir::go_expression::{GoExpressionNode, verbatim_identifiers};
use crate::target::ir::local::{GoIdentifier, LocalId};
use crate::target::ir::values::GoExpression;
use crate::target::ir::visit::{BindingSite, VisitorMut, visit_statements_mut};
use rustc_hash::{FxHashMap, FxHashSet};
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyErrorKind {
    EmptyIfCondition,
    InvalidExpressionStatement,
    MissingGoTermination,
    UnresolvedLoopTarget,
    ConflictingLocalSpelling,
    ShadowedLocalReference,
    UnboundLocalReference,
    UnidentifiedLocal,
    UnorderedOperand,
    MissingValue,
    UnusedLocal,
    NoNewShortVariable,
    RepeatedShortVariable,
    RedeclaredLocal,
    DuplicateParameter,
    BlankValueRead,
    UnusedLabel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyResult {
    Value,
    Void,
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

fn verify_control_structure(statements: &mut Vec<Statement>) -> Result<(), BodyError> {
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
                LoweredStatement::ExpressionStatement { expression, .. }
                    if !expression.is_empty() && !expression.stands_as_a_statement() =>
                {
                    Some(BodyErrorKind::InvalidExpressionStatement)
                }
                LoweredStatement::Async { call, .. } if !call.stands_as_a_statement() => {
                    Some(BodyErrorKind::InvalidExpressionStatement)
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
    result: BodyResult,
) -> Result<(), BodyError> {
    verify_control_structure(&mut body.statements)?;
    verify_values(&mut body.statements)?;
    if result == BodyResult::Value && !body.go_terminates() {
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
                LoweredStatement::Discard { value, .. }
                | LoweredStatement::Assign(AssignForm::Simple { value, .. }) => vec![value],
                LoweredStatement::Return(values) => values.iter().collect(),
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

struct LocalScopeCheck {
    frames: ScopeStack<FxHashMap<String, LocalId>>,
    spellings: FxHashMap<LocalId, String>,
    error: Option<BodyErrorKind>,
}

impl LocalScopeCheck {
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

impl VisitorMut for LocalScopeCheck {
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

    fn local_binding(&mut self, name: &mut GoIdentifier, _site: BindingSite) {
        if name.is_blank() {
            return;
        }
        let Some(id) = name.id() else {
            self.error.get_or_insert(BodyErrorKind::UnidentifiedLocal);
            return;
        };
        self.check_spelling(name);
        self.frames
            .innermost_mut()
            .insert(name.spelling().to_string(), id);
    }

    fn enter_scope(&mut self) {
        self.frames.push(FxHashMap::default());
    }

    fn exit_scope(&mut self) {
        self.frames.pop().expect("body has a scope");
    }
}

pub(crate) fn verify_local_scopes(
    statements: &mut Vec<Statement>,
    bindings: &[GoIdentifier],
) -> Result<(), BodyError> {
    let mut check = LocalScopeCheck {
        frames: ScopeStack::new(FxHashMap::default()),
        spellings: FxHashMap::default(),
        error: None,
    };
    for name in bindings {
        check.local_binding(&mut name.clone(), BindingSite::Parameter);
    }
    visit_statements_mut(statements, &mut check);
    check.error.map_or(Ok(()), |kind| Err(BodyError { kind }))
}

struct GoNameCheck {
    frames: ScopeStack<FxHashMap<String, LocalId>>,
    declared: Vec<LocalId>,
    read: FxHashSet<LocalId>,
    labels: FxHashSet<String>,
    jumps: FxHashSet<String>,
    error: Option<BodyErrorKind>,
}

impl GoNameCheck {
    fn read_spelling(&mut self, spelling: &str) {
        if let Some(id) = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(spelling))
        {
            self.read.insert(*id);
        }
    }

    fn finish(self) -> Result<(), BodyError> {
        let unused_local = self.declared.iter().any(|id| !self.read.contains(id));
        let unused_label = self.labels.iter().any(|label| !self.jumps.contains(label));
        let kind = self
            .error
            .or(unused_local.then_some(BodyErrorKind::UnusedLocal))
            .or(unused_label.then_some(BodyErrorKind::UnusedLabel));
        kind.map_or(Ok(()), |kind| Err(BodyError { kind }))
    }
}

impl VisitorMut for GoNameCheck {
    fn expression(&mut self, node: &mut GoExpressionNode) {
        match node {
            GoExpressionNode::Identifier(name) if name.is_blank() => {
                self.error.get_or_insert(BodyErrorKind::BlankValueRead);
            }
            GoExpressionNode::Identifier(name) => {
                if let Some(id) = name.id() {
                    self.read.insert(id);
                }
            }
            GoExpressionNode::Verbatim(source) => {
                for spelling in verbatim_identifiers(source) {
                    self.read_spelling(spelling);
                }
            }
            _ => {}
        }
    }

    fn statements(&mut self, statements: &mut Vec<Statement>) {
        for statement in statements.iter() {
            match &statement.kind {
                LoweredStatement::Loop(plan) => {
                    if let Some(label) = plan.kind.label() {
                        self.labels.insert(label.to_string());
                    }
                }
                LoweredStatement::Break(LoopTransfer::Labeled(label))
                | LoweredStatement::Continue(LoopTransfer::Labeled(label)) => {
                    self.jumps.insert(label.clone());
                }
                _ => {}
            }
        }
    }

    fn assigned_identifier(&mut self, _node: &mut GoExpressionNode) {}

    fn short_variable_declaration(&mut self, names: &[GoIdentifier]) {
        let named: Vec<&str> = names
            .iter()
            .filter(|name| !name.is_blank())
            .map(GoIdentifier::spelling)
            .collect();
        if named
            .iter()
            .enumerate()
            .any(|(index, spelling)| named[..index].contains(spelling))
        {
            self.error
                .get_or_insert(BodyErrorKind::RepeatedShortVariable);
        } else if named
            .iter()
            .all(|spelling| self.frames.innermost().contains_key(*spelling))
        {
            self.error.get_or_insert(BodyErrorKind::NoNewShortVariable);
        }
    }

    fn local_binding(&mut self, name: &mut GoIdentifier, site: BindingSite) {
        let Some(id) = name.id().filter(|_| !name.is_blank()) else {
            return;
        };
        if self.frames.innermost().contains_key(name.spelling()) {
            match site {
                BindingSite::ShortVariable => {}
                BindingSite::Parameter => {
                    self.error.get_or_insert(BodyErrorKind::DuplicateParameter);
                }
                BindingSite::Variable
                | BindingSite::Constant
                | BindingSite::RangeVariable
                | BindingSite::CountedVariable
                | BindingSite::TypeSwitchVariable
                | BindingSite::ReceiveVariable => {
                    self.error.get_or_insert(BodyErrorKind::RedeclaredLocal);
                }
            }
            return;
        }
        self.frames
            .innermost_mut()
            .insert(name.spelling().to_string(), id);
        if site.must_be_read() {
            self.declared.push(id);
        }
    }

    fn enter_scope(&mut self) {
        self.frames.push(FxHashMap::default());
    }

    fn exit_scope(&mut self) {
        self.frames.pop().expect("body has a scope");
    }
}

pub(crate) fn verify_go_names(
    statements: &mut Vec<Statement>,
    parameters: &[GoIdentifier],
) -> Result<(), BodyError> {
    let visible = parameters
        .iter()
        .filter_map(|name| Some((name.spelling().to_string(), name.id()?)))
        .collect();
    let mut check = GoNameCheck {
        frames: ScopeStack::new(visible),
        declared: Vec::new(),
        read: FxHashSet::default(),
        labels: FxHashSet::default(),
        jumps: FxHashSet::default(),
        error: None,
    };
    visit_statements_mut(statements, &mut check);
    check.finish()
}
