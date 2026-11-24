use crate::Planner;
use crate::lowering::abi::go_result_wrapping::{NilGuard, nil_condition, non_nil_condition};
use crate::lowering::calls::bounds::BoundsCheckedIndex;
use crate::lowering::calls::predicates::Polarity;

use crate::lookup::state::scope::PairStatusKind;
use crate::target::ir::bodies::{Definition, PairCondition, Statement, define, discard};
use crate::target::ir::go_expression::{BinaryOp, UnaryOp};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;

pub(in crate::lowering) struct BoundValue {
    pub(in crate::lowering) statements: Vec<Statement>,
    source: BoundSource,
}

enum BoundSource {
    Pair(LoweredPair),
    Nullable {
        value: GoIdentifier,
        nil_guard: NilGuard,
        initializer_call: Option<GoExpression>,
    },
    Index {
        index: BoundsCheckedIndex,
        target: Option<GoIdentifier>,
    },
    Found {
        value: Option<GoIdentifier>,
        flag: GoIdentifier,
    },
}

pub(super) struct LoweredPair {
    pub(super) value: PairValue,
    pub(super) status: GoIdentifier,
    pub(super) status_kind: PairStatusKind,
    pub(super) initializer_call: Option<GoExpression>,
    pub(super) borrowed: bool,
}

pub(super) enum PairValue {
    Absent,
    Discarded,
    Named {
        name: GoIdentifier,
        nil_guard: Option<NilGuard>,
    },
}

impl PairValue {
    pub(super) fn name(&self) -> Option<&GoIdentifier> {
        match self {
            Self::Named { name, .. } => Some(name),
            Self::Absent | Self::Discarded => None,
        }
    }
}

impl LoweredPair {
    pub(super) fn binding(&self) -> Vec<GoIdentifier> {
        let status = self.status.clone();
        match &self.value {
            PairValue::Named { name, .. } => vec![name.clone(), status],
            PairValue::Discarded => vec![GoIdentifier::blank(), status],
            PairValue::Absent => vec![status],
        }
    }

    fn initializer(&self) -> Option<Definition> {
        let call = self.initializer_call.as_ref()?;
        Some(Definition {
            names: self.binding(),
            value: call.clone(),
        })
    }

    fn condition(&self, polarity: Polarity) -> PairCondition {
        let status = GoExpression::identifier(self.status.clone());
        let status = match (self.status_kind, polarity) {
            (PairStatusKind::Ok, Polarity::Positive) => status,
            (PairStatusKind::Ok, Polarity::Negated) => GoExpression::unary(UnaryOp::Not, status),
            (PairStatusKind::Error, Polarity::Positive) => nil_condition(status),
            (PairStatusKind::Error, Polarity::Negated) => non_nil_condition(status),
        };
        let condition = match &self.value {
            PairValue::Named {
                name,
                nil_guard: Some(guard),
            } => {
                let value = GoExpression::identifier(name.clone());
                let (nil_condition, operator) = match polarity {
                    Polarity::Positive => (guard.non_nil_condition(value), BinaryOp::And),
                    Polarity::Negated => (guard.nil_condition(value), BinaryOp::Or),
                };
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

impl BoundValue {
    pub(super) fn pair(statements: Vec<Statement>, pair: LoweredPair) -> Self {
        Self {
            statements,
            source: BoundSource::Pair(pair),
        }
    }

    pub(in crate::lowering) fn nullable(
        statements: Vec<Statement>,
        value: GoIdentifier,
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

    pub(in crate::lowering) fn index(
        statements: Vec<Statement>,
        index: BoundsCheckedIndex,
        target: Option<GoIdentifier>,
    ) -> Self {
        Self {
            statements,
            source: BoundSource::Index { index, target },
        }
    }

    pub(in crate::lowering) fn found(
        statements: Vec<Statement>,
        value: Option<GoIdentifier>,
        flag: GoIdentifier,
    ) -> Self {
        Self {
            statements,
            source: BoundSource::Found { value, flag },
        }
    }

    pub(in crate::lowering) fn payload(&self) -> Option<GoExpression> {
        match &self.source {
            BoundSource::Index { index, .. } => Some(index.element()),
            _ => self
                .payload_identifier()
                .cloned()
                .map(GoExpression::identifier),
        }
    }

    pub(in crate::lowering) fn payload_identifier(&self) -> Option<&GoIdentifier> {
        match &self.source {
            BoundSource::Pair(pair) => pair.value.name(),
            BoundSource::Nullable { value, .. } => Some(value),
            BoundSource::Index { .. } => None,
            BoundSource::Found { value, .. } => value.as_ref(),
        }
    }

    pub(in crate::lowering) fn writable_payload(
        &mut self,
        planner: &mut Planner<'_>,
    ) -> GoIdentifier {
        if let BoundSource::Pair(pair) = &mut self.source
            && pair.borrowed
            && let Some(shared) = pair.value.name()
        {
            let copy = planner.fresh_declared("ret");
            let shared = GoExpression::identifier(shared.clone());
            self.statements.push(define(copy.clone(), shared));
            pair.value = PairValue::Named {
                name: copy.clone(),
                nil_guard: None,
            };
            pair.borrowed = false;
            return copy;
        }
        self.payload_identifier()
            .expect("a payload slot was requested")
            .clone()
    }

    pub(in crate::lowering) fn status_identifier(&self) -> &GoIdentifier {
        match &self.source {
            BoundSource::Pair(pair) => &pair.status,
            _ => unreachable!("only a pair has a status local"),
        }
    }

    pub(in crate::lowering) fn binds_value(&self) -> bool {
        !matches!(self.source, BoundSource::Index { .. })
    }

    pub(in crate::lowering) fn late_binding(&self) -> Option<Statement> {
        let BoundSource::Index {
            index,
            target: Some(target),
        } = &self.source
        else {
            return None;
        };
        Some(define(target.clone(), index.element()))
    }

    pub(in crate::lowering) fn discard_value(&mut self) {
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
                    .push(discard(GoExpression::identifier(value.clone())));
            }
            _ => {}
        }
    }

    pub(in crate::lowering) fn success_condition(&self) -> PairCondition {
        self.condition(Polarity::Positive)
    }

    pub(in crate::lowering) fn failure_condition(&self) -> PairCondition {
        self.condition(Polarity::Negated)
    }

    fn condition(&self, polarity: Polarity) -> PairCondition {
        let plain = |condition: GoExpression| PairCondition {
            initializer: None,
            condition,
        };
        match &self.source {
            BoundSource::Pair(pair) => pair.condition(polarity),
            BoundSource::Nullable {
                value,
                nil_guard,
                initializer_call,
            } => {
                let tested = GoExpression::identifier(value.clone());
                let test = match polarity {
                    Polarity::Positive => nil_guard.non_nil_condition(tested),
                    Polarity::Negated => nil_guard.nil_condition(tested),
                };
                PairCondition {
                    initializer: initializer_call.as_ref().map(|call| Definition {
                        names: vec![value.clone()],
                        value: call.clone(),
                    }),
                    condition: test,
                }
            }
            BoundSource::Index { index, .. } => plain(index.condition(polarity)),
            BoundSource::Found { flag, .. } if polarity == Polarity::Positive => {
                plain(GoExpression::identifier(flag.clone()))
            }
            BoundSource::Found { flag, .. } => plain(GoExpression::unary(
                UnaryOp::Not,
                GoExpression::identifier(flag.clone()),
            )),
        }
    }
}
