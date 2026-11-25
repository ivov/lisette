use crate::target::types::go_type::{GoType, GoTypeKind};

use crate::target::ir::go_expression::{CallPurity, GoBinaryOperator, GoExpressionNode};
use crate::target::ir::values::Stability;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EvaluationEffect {
    #[default]
    Pure,
    PureCall,
    EffectfulCall,
}

impl EvaluationEffect {
    fn has_call(self) -> bool {
        !matches!(self, EvaluationEffect::Pure)
    }

    fn has_effectful_call(self) -> bool {
        matches!(self, EvaluationEffect::EffectfulCall)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Writes {
    #[default]
    Nothing,
    Shared,
    Any,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    calls: EvaluationEffect,
    may_panic: bool,
    may_block: bool,
    reads: Stability,
    writes: Writes,
    creates_identity: bool,
}

impl Effects {
    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            calls: self.calls.max(other.calls),
            may_panic: self.may_panic || other.may_panic,
            may_block: self.may_block || other.may_block,
            reads: self.reads.max(other.reads),
            writes: self.writes.max(other.writes),
            creates_identity: self.creates_identity || other.creates_identity,
        }
    }

    pub(crate) fn anything() -> Self {
        Self {
            calls: EvaluationEffect::EffectfulCall,
            may_panic: true,
            may_block: true,
            reads: Stability::Observable,
            writes: Writes::Any,
            creates_identity: true,
        }
    }

    pub(crate) fn local_read() -> Self {
        Self {
            reads: Stability::StableAcrossCalls,
            ..Self::default()
        }
    }

    pub(crate) fn read_of(stability: Stability) -> Self {
        Self {
            reads: stability,
            ..Self::default()
        }
    }

    pub(crate) fn with_known_reads(self, reads: Stability) -> Self {
        Self {
            reads,
            writes: self.writes.min(Writes::Shared),
            ..self
        }
    }

    pub(crate) fn without_go_order(self) -> Self {
        Self {
            calls: EvaluationEffect::Pure,
            writes: Writes::Nothing,
            ..self
        }
    }

    pub(crate) fn runs_code(self) -> bool {
        self.calls.has_call()
    }

    pub(crate) fn runs_effectful_code(self) -> bool {
        self.calls.has_effectful_call()
    }

    pub(crate) fn panics_or_blocks(self) -> bool {
        self.may_panic || self.may_block
    }

    pub(crate) fn reads(self) -> Stability {
        self.reads
    }

    pub(crate) fn can_erase(self) -> bool {
        !self.runs_code() && !self.panics_or_blocks() && self.writes == Writes::Nothing
    }

    pub(crate) fn can_duplicate(self) -> bool {
        !self.runs_code()
            && !self.may_block
            && self.writes == Writes::Nothing
            && !self.creates_identity
    }

    pub(crate) fn must_evaluate_once(self) -> bool {
        !self.can_duplicate()
    }

    pub(crate) fn can_move_across(self, between: Self) -> bool {
        let orders = |first: Self, second: Self| {
            first.runs_effectful_code() && (second.runs_code() || second.panics_or_blocks())
        };
        !orders(self, between)
            && !orders(between, self)
            && !(self.may_block && between.may_block)
            && !observes(self.reads, between.writes)
            && !observes(between.reads, self.writes)
    }
}

fn observes(reads: Stability, writes: Writes) -> bool {
    !matches!(
        (reads, writes),
        (Stability::Fixed, _)
            | (_, Writes::Nothing)
            | (Stability::StableAcrossCalls, Writes::Shared)
    )
}

fn call_effects(purity: CallPurity) -> Effects {
    match purity {
        CallPurity::Constructor | CallPurity::Conversion => Effects {
            calls: EvaluationEffect::PureCall,
            ..Effects::default()
        },
        CallPurity::ReadOnly => Effects {
            calls: EvaluationEffect::PureCall,
            reads: Stability::Observable,
            ..Effects::default()
        },
        CallPurity::Effectful => Effects {
            calls: EvaluationEffect::EffectfulCall,
            reads: Stability::Observable,
            writes: Writes::Any,
            ..Effects::default()
        },
    }
}

fn composite_literal_effects(go_type: Option<&GoType>) -> Effects {
    let shape = go_type.map(GoType::physical_kind);
    Effects {
        may_panic: matches!(shape, Some(GoTypeKind::Map)),
        creates_identity: matches!(shape, None | Some(GoTypeKind::Slice | GoTypeKind::Map)),
        ..Effects::default()
    }
}

impl GoExpressionNode {
    pub(crate) fn effects(&self) -> Effects {
        let mut effects = Effects::default();
        self.visit_children(&mut |child| effects = effects.union(child.effects()));
        if matches!(self, Self::Call { .. }) {
            effects.may_panic = false;
            effects.may_block = false;
        }
        effects.union(self.own_effects())
    }

    fn own_effects(&self) -> Effects {
        let reference_read = |may_panic| Effects {
            may_panic,
            reads: Stability::Observable,
            ..Effects::default()
        };
        match self {
            Self::Identifier(_) => Effects::local_read(),
            Self::Qualified { .. } => reference_read(false),
            Self::Call { purity, .. } => call_effects(*purity),
            Self::Verbatim(_) => call_effects(CallPurity::Effectful),
            Self::Index { in_bounds, .. } | Self::Slice { in_bounds, .. } => {
                reference_read(!in_bounds)
            }
            Self::Dereference(_) => reference_read(true),
            Self::Selector { may_panic, .. } if *may_panic => reference_read(true),
            Self::TypeAssertion { .. } => Effects {
                may_panic: true,
                ..Effects::default()
            },
            Self::AddressOf(operand) => Effects {
                creates_identity: matches!(operand.as_ref(), Self::CompositeLiteral { .. }),
                ..Effects::default()
            },
            Self::CompositeLiteral { go_type, .. } => composite_literal_effects(go_type.as_deref()),
            Self::Conversion { go_type, .. }
                if matches!(
                    go_type.physical_kind(),
                    GoTypeKind::Array | GoTypeKind::PointerToArray
                ) =>
            {
                Effects {
                    may_panic: true,
                    ..Effects::default()
                }
            }
            Self::Literal(_)
            | Self::Type(_)
            | Self::Empty
            | Self::Instantiation { .. }
            | Self::Spread(_)
            | Self::FunctionLiteral { .. }
            | Self::Selector { .. }
            | Self::Conversion { .. }
            | Self::Unary { .. } => Effects::default(),
            Self::Binary {
                operator,
                left,
                right,
            } => Self::binary_effects(*operator, left, right),
        }
    }

    fn binary_effects(operator: GoBinaryOperator, left: &Self, right: &Self) -> Effects {
        match operator {
            GoBinaryOperator::Division
            | GoBinaryOperator::Remainder
            | GoBinaryOperator::ShiftLeft
            | GoBinaryOperator::ShiftRight => Effects {
                may_panic: true,
                ..Effects::default()
            },
            GoBinaryOperator::Equal | GoBinaryOperator::NotEqual
                if !matches!(left, Self::Literal(_))
                    && !matches!(right, Self::Literal(_))
                    && !(left.is_unit_struct_literal() && right.is_unit_struct_literal()) =>
            {
                Effects {
                    may_panic: true,
                    ..Effects::default()
                }
            }
            GoBinaryOperator::Addition
            | GoBinaryOperator::Subtraction
            | GoBinaryOperator::Multiplication
            | GoBinaryOperator::BitwiseAnd
            | GoBinaryOperator::BitwiseOr
            | GoBinaryOperator::BitwiseXor
            | GoBinaryOperator::BitwiseAndNot
            | GoBinaryOperator::And
            | GoBinaryOperator::Or
            | GoBinaryOperator::LessThan
            | GoBinaryOperator::LessThanOrEqual
            | GoBinaryOperator::GreaterThan
            | GoBinaryOperator::GreaterThanOrEqual
            | GoBinaryOperator::Equal
            | GoBinaryOperator::NotEqual => Effects::default(),
        }
    }
}
