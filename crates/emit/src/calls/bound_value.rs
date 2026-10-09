use crate::Planner;
use crate::calls::bounds::BoundsCheckedIndex;
use crate::calls::go_interop::{NilGuard, is_nil, non_nil};
use crate::plan::bodies::{Definition, Statement, define, discard};
use crate::plan::go_expression::{BinaryOp, UnaryOp};
use crate::plan::values::GoExpression;
use crate::state::scope::PairStatusKind;

pub(crate) struct BoundValue {
    pub(crate) statements: Vec<Statement>,
    source: BoundSource,
}

enum BoundSource {
    Pair(LoweredPair),
    Nullable {
        value: String,
        nil_guard: NilGuard,
        initializer_call: Option<GoExpression>,
    },
    Index {
        index: BoundsCheckedIndex,
        target: Option<String>,
    },
    Found {
        value: Option<String>,
        flag: String,
    },
}

/// A bound two-result expression and the rule that distinguishes success.
pub(super) struct LoweredPair {
    pub(super) value: PairValue,
    pub(super) status: String,
    pub(super) status_kind: PairStatusKind,
    pub(super) initializer_call: Option<GoExpression>,
    pub(super) borrowed: bool,
}

pub(super) enum PairValue {
    Absent,
    Discarded,
    Named {
        name: String,
        nil_guard: Option<NilGuard>,
    },
}

impl PairValue {
    pub(super) fn name(&self) -> Option<&str> {
        match self {
            Self::Named { name, .. } => Some(name),
            Self::Absent | Self::Discarded => None,
        }
    }
}

impl LoweredPair {
    pub(super) fn binding(&self) -> Vec<String> {
        match &self.value {
            PairValue::Named { name, .. } => vec![name.clone(), self.status.clone()],
            PairValue::Discarded => vec!["_".to_string(), self.status.clone()],
            PairValue::Absent => vec![self.status.clone()],
        }
    }

    fn initializer(&self) -> Option<Definition> {
        let call = self.initializer_call.as_ref()?;
        Some(Definition {
            names: self.binding().into_iter().map(Into::into).collect(),
            value: call.clone(),
        })
    }

    fn condition(&self, success: bool) -> PairCondition {
        let status = GoExpression::name(self.status.clone());
        let status = match (self.status_kind, success) {
            (PairStatusKind::Ok, true) => status,
            (PairStatusKind::Ok, false) => GoExpression::unary(UnaryOp::Not, status),
            (PairStatusKind::Error, true) => is_nil(status),
            (PairStatusKind::Error, false) => non_nil(status),
        };
        let condition = match &self.value {
            PairValue::Named {
                name,
                nil_guard: Some(guard),
            } => {
                let value = GoExpression::name(name.clone());
                let nil_condition = if success {
                    guard.non_nil(value)
                } else {
                    guard.is_nil(value)
                };
                let operator = if success { BinaryOp::And } else { BinaryOp::Or };
                GoExpression::binary(status, operator, nil_condition)
            }
            _ => status,
        };
        PairCondition {
            initializer: self.initializer(),
            condition,
        }
    }
}

pub(crate) struct PairCondition {
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
}

impl BoundValue {
    pub(super) fn pair(statements: Vec<Statement>, pair: LoweredPair) -> Self {
        Self {
            statements,
            source: BoundSource::Pair(pair),
        }
    }

    pub(crate) fn nullable(
        statements: Vec<Statement>,
        value: String,
        nil_guard: NilGuard,
        initializer_call: Option<GoExpression>,
    ) -> Self {
        Self {
            statements,
            source: BoundSource::Nullable {
                value,
                nil_guard,
                initializer_call,
            },
        }
    }

    pub(crate) fn index(
        statements: Vec<Statement>,
        index: BoundsCheckedIndex,
        target: Option<String>,
    ) -> Self {
        Self {
            statements,
            source: BoundSource::Index { index, target },
        }
    }

    pub(crate) fn found(statements: Vec<Statement>, value: Option<String>, flag: String) -> Self {
        Self {
            statements,
            source: BoundSource::Found { value, flag },
        }
    }

    /// The payload expression, valid once the some-condition holds.
    pub(crate) fn payload(&self) -> Option<GoExpression> {
        match &self.source {
            BoundSource::Index { index, .. } => Some(index.element.clone()),
            _ => self
                .payload_name()
                .map(|name| GoExpression::name(name.to_string())),
        }
    }

    pub(crate) fn payload_name(&self) -> Option<&str> {
        match &self.source {
            BoundSource::Pair(pair) => pair.value.name(),
            BoundSource::Nullable { value, .. } => Some(value),
            BoundSource::Index { .. } => None,
            BoundSource::Found { value, .. } => value.as_deref(),
        }
    }

    pub(crate) fn writable_payload(&mut self, planner: &mut Planner<'_>) -> String {
        if let BoundSource::Pair(pair) = &mut self.source
            && pair.borrowed
            && let Some(shared) = pair.value.name()
        {
            let copy = planner.fresh_pair_value();
            self.statements
                .push(define(copy.clone(), GoExpression::name(shared.to_string())));
            pair.value = PairValue::Named {
                name: copy.clone(),
                nil_guard: None,
            };
            pair.borrowed = false;
            return copy;
        }
        self.payload_name()
            .expect("a payload slot was requested")
            .to_string()
    }

    pub(crate) fn status(&self) -> &str {
        match &self.source {
            BoundSource::Pair(pair) => &pair.status,
            _ => unreachable!("only a pair has a status local"),
        }
    }

    pub(crate) fn binds_value(&self) -> bool {
        !matches!(self.source, BoundSource::Index { .. })
    }

    /// The statement that reads a late payload into its requested name.
    pub(crate) fn late_binding(&self) -> Option<Statement> {
        let BoundSource::Index {
            index,
            target: Some(target),
        } = &self.source
        else {
            return None;
        };
        Some(define(target.clone(), index.element.clone()))
    }

    pub(crate) fn discard_value(&mut self) {
        match &mut self.source {
            BoundSource::Pair(pair) => {
                if matches!(
                    pair.value,
                    PairValue::Named {
                        nil_guard: None,
                        ..
                    }
                ) {
                    pair.value = PairValue::Discarded;
                }
            }
            BoundSource::Found {
                value: Some(value), ..
            } => {
                self.statements
                    .push(discard(GoExpression::name(value.clone())));
            }
            _ => {}
        }
    }

    pub(crate) fn success_condition(&self) -> PairCondition {
        self.condition(true)
    }

    pub(crate) fn failure_condition(&self) -> PairCondition {
        self.condition(false)
    }

    fn condition(&self, success: bool) -> PairCondition {
        let plain = |condition: GoExpression| PairCondition {
            initializer: None,
            condition,
        };
        match &self.source {
            BoundSource::Pair(pair) => pair.condition(success),
            BoundSource::Nullable {
                value,
                nil_guard,
                initializer_call,
            } => {
                let tested = GoExpression::name(value.clone());
                let test = if success {
                    nil_guard.non_nil(tested)
                } else {
                    nil_guard.is_nil(tested)
                };
                PairCondition {
                    initializer: initializer_call.as_ref().map(|call| Definition {
                        names: vec![value.clone().into()],
                        value: call.clone(),
                    }),
                    condition: test,
                }
            }
            BoundSource::Index { index, .. } if success => plain(index.in_bounds.clone()),
            BoundSource::Index { index, .. } => plain(index.out_of_bounds.clone()),
            BoundSource::Found { flag, .. } if success => plain(GoExpression::name(flag.clone())),
            BoundSource::Found { flag, .. } => plain(GoExpression::unary(
                UnaryOp::Not,
                GoExpression::name(flag.clone()),
            )),
        }
    }
}
