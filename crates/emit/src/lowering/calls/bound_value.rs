use crate::Planner;
use crate::lookup::state::scope::PairStatusKind;
use crate::lowering::abi::go_result_wrapping::{
    NilGuard, nil_condition, non_nil_condition, unexpected_nil_error,
};
use crate::lowering::abi::go_returns::tag_check_with_initializer;
use crate::lowering::calls::bounds_checks::BoundsCheckedIndex;
use crate::lowering::calls::comma_ok::CommaOkValueSlot;
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::calls::wrap_err::{WrapMessage, wrap_error};
use crate::output::finish::cleanup::name_uses::NameUses;
use crate::target::ir::bodies::{
    Definition, ElseArm, ElseLayout, IfOrigin, IfPlan, LoweredBlock, LoweredStatement,
    PairCondition, Statement, assign, define, define_many,
};
use crate::target::ir::go_expression::{GoBinaryOperator, GoUnaryOperator};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;

pub(in crate::lowering) trait SlotPlacement {
    type Initializer: Default;
    type Condition;

    fn opens_if(slot: &CommaOkValueSlot) -> bool;

    fn binds_index_target() -> bool;

    fn place(
        opens_if: bool,
        call: GoExpression,
        define: impl FnOnce(GoExpression),
    ) -> Self::Initializer;

    fn condition(
        initializer: &Self::Initializer,
        names: impl FnOnce() -> Vec<GoIdentifier>,
        test: GoExpression,
    ) -> Self::Condition;
}

pub(in crate::lowering) trait HeaderPlacement {
    const BINDS_INDEX_TARGET: bool;
}

pub(in crate::lowering) trait PredicatePlacement: SlotPlacement {
    fn test_slot() -> CommaOkValueSlot;
}

pub(in crate::lowering) struct InBranch;

pub(in crate::lowering) struct InIfHeader;

pub(in crate::lowering) struct InExpression;

#[derive(Default)]
pub(in crate::lowering) struct Placed;

impl HeaderPlacement for InBranch {
    const BINDS_INDEX_TARGET: bool = true;
}

impl HeaderPlacement for InIfHeader {
    const BINDS_INDEX_TARGET: bool = false;
}

impl PredicatePlacement for InIfHeader {
    fn test_slot() -> CommaOkValueSlot {
        CommaOkValueSlot::Unused
    }
}

impl PredicatePlacement for InExpression {
    fn test_slot() -> CommaOkValueSlot {
        CommaOkValueSlot::Discarded
    }
}

impl<H: HeaderPlacement> SlotPlacement for H {
    type Initializer = Option<GoExpression>;
    type Condition = PairCondition;

    fn opens_if(slot: &CommaOkValueSlot) -> bool {
        matches!(slot, CommaOkValueSlot::Arm(_) | CommaOkValueSlot::Unused)
    }

    fn binds_index_target() -> bool {
        H::BINDS_INDEX_TARGET
    }

    fn place(
        opens_if: bool,
        call: GoExpression,
        define: impl FnOnce(GoExpression),
    ) -> Self::Initializer {
        if opens_if {
            Some(call)
        } else {
            define(call);
            None
        }
    }

    fn condition(
        initializer: &Self::Initializer,
        names: impl FnOnce() -> Vec<GoIdentifier>,
        test: GoExpression,
    ) -> Self::Condition {
        let initializer = initializer.as_ref().map(|call| Definition {
            names: names(),
            value: call.clone(),
        });
        PairCondition::initialized(initializer, test)
    }
}

impl SlotPlacement for InExpression {
    type Initializer = Placed;
    type Condition = GoExpression;

    fn opens_if(_: &CommaOkValueSlot) -> bool {
        false
    }

    fn binds_index_target() -> bool {
        false
    }

    fn place(_: bool, call: GoExpression, define: impl FnOnce(GoExpression)) -> Self::Initializer {
        define(call);
        Placed
    }

    fn condition(
        _: &Self::Initializer,
        _: impl FnOnce() -> Vec<GoIdentifier>,
        test: GoExpression,
    ) -> Self::Condition {
        test
    }
}

pub(in crate::lowering) struct BoundValue<P: SlotPlacement = InBranch> {
    statements: Vec<Statement>,
    source: BoundSource<P>,
}

enum BoundSource<P: SlotPlacement> {
    Pair(LoweredPair<P>),
    Nullable {
        value: GoIdentifier,
        nil_guard: NilGuard,
        initializer_call: P::Initializer,
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

pub(in crate::lowering) struct BoundPair<P: SlotPlacement = InBranch> {
    statements: Vec<Statement>,
    pair: LoweredPair<P>,
}

struct LoweredPair<P: SlotPlacement> {
    value: PairValue,
    status: GoIdentifier,
    status_kind: PairStatusKind,
    initializer_call: P::Initializer,
    borrowed: bool,
    wraps: Vec<WrapMessage>,
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

    pub(super) fn binding(&self, status: &GoIdentifier) -> Vec<GoIdentifier> {
        let status = status.clone();
        match self {
            Self::Named { name, .. } => vec![name.clone(), status],
            Self::Discarded => vec![GoIdentifier::blank(), status],
            Self::Absent => vec![status],
        }
    }
}

pub(in crate::lowering) struct FailureBody(LoweredBlock);

impl FailureBody {
    pub(in crate::lowering) fn renders_empty(&self) -> bool {
        self.0.renders_empty()
    }

    pub(in crate::lowering) fn ends_with_diverge(&self) -> bool {
        self.0.ends_with_diverge()
    }
}

pub(in crate::lowering) enum Arms {
    SuccessFirst {
        success: LoweredBlock,
        failure_layout: ElseLayout,
    },
    FailureFirst {
        success: LoweredBlock,
    },
    FailureOnly,
}

fn status_test(status: &GoIdentifier, kind: PairStatusKind, polarity: Polarity) -> GoExpression {
    let status = GoExpression::identifier(status.clone());
    match (kind, polarity) {
        (PairStatusKind::Ok, Polarity::Positive) => status,
        (PairStatusKind::Ok, Polarity::Negated) => {
            GoExpression::unary(GoUnaryOperator::Not, status)
        }
        (PairStatusKind::Error, Polarity::Positive) => nil_condition(status),
        (PairStatusKind::Error, Polarity::Negated) => non_nil_condition(status),
    }
}

fn guarded_value(
    value: &PairValue,
    status_kind: PairStatusKind,
) -> Option<(&GoIdentifier, NilGuard)> {
    match value {
        PairValue::Named {
            name,
            nil_guard: Some(guard),
        } if status_kind == PairStatusKind::Error => Some((name, *guard)),
        PairValue::Named { .. } | PairValue::Absent | PairValue::Discarded => None,
    }
}

impl<P: SlotPlacement> LoweredPair<P> {
    fn test(&self, polarity: Polarity) -> GoExpression {
        let status = status_test(&self.status, self.status_kind, polarity);

        match &self.value {
            PairValue::Named {
                name,
                nil_guard: Some(guard),
            } => {
                let value = GoExpression::identifier(name.clone());
                let (nil_condition, operator) = match polarity {
                    Polarity::Positive => (guard.non_nil_condition(value), GoBinaryOperator::And),
                    Polarity::Negated => (guard.nil_condition(value), GoBinaryOperator::Or),
                };
                GoExpression::binary(status, operator, nil_condition)
            }
            PairValue::Absent | PairValue::Discarded | PairValue::Named { .. } => status,
        }
    }

    fn condition(&self, polarity: Polarity) -> P::Condition {
        P::condition(
            &self.initializer_call,
            || self.value.binding(&self.status),
            self.test(polarity),
        )
    }

    fn error_body(&self, build: impl FnOnce(&GoIdentifier) -> LoweredBlock) -> FailureBody {
        let Self {
            value,
            status,
            status_kind,
            wraps,
            initializer_call: _,
            borrowed: _,
        } = self;
        let mut block = build(status);
        let guarded = guarded_value(value, *status_kind).is_some();
        if (!guarded && wraps.is_empty())
            || NameUses::of(&mut block.statements).references(status) == 0
        {
            return FailureBody(block);
        }
        let error = GoExpression::identifier(status.clone());
        let mut statements = Vec::new();
        if guarded {
            statements.push(
                LoweredStatement::If(IfPlan::plain(
                    nil_condition(error.clone()),
                    LoweredBlock {
                        statements: vec![assign(error.clone(), unexpected_nil_error())],
                    },
                    ElseArm::None,
                ))
                .into(),
            );
        }
        if !wraps.is_empty() {
            statements.push(assign(error.clone(), wrap_error(wraps, error)));
        }
        statements.extend(block.statements);
        FailureBody(LoweredBlock { statements })
    }
}

impl<P: SlotPlacement> BoundPair<P> {
    pub(super) fn place(
        mut statements: Vec<Statement>,
        value: PairValue,
        status: GoIdentifier,
        status_kind: PairStatusKind,
        call: GoExpression,
        opens_if: bool,
    ) -> Self {
        let initializer_call = P::place(opens_if, call, |call| {
            statements.push(define_many(value.binding(&status), call));
        });
        Self {
            statements,
            pair: LoweredPair {
                value,
                status,
                status_kind,
                initializer_call,
                borrowed: false,
                wraps: Vec::new(),
            },
        }
    }

    pub(super) fn existing(
        statements: Vec<Statement>,
        value: GoIdentifier,
        status: GoIdentifier,
        status_kind: PairStatusKind,
        borrowed: bool,
    ) -> Self {
        Self {
            statements,
            pair: LoweredPair {
                value: PairValue::Named {
                    name: value,
                    nil_guard: None,
                },
                status,
                status_kind,
                initializer_call: P::Initializer::default(),
                borrowed,
                wraps: Vec::new(),
            },
        }
    }

    pub(super) fn with_wraps(mut self, setup: Vec<Statement>, wraps: Vec<WrapMessage>) -> Self {
        self.statements.extend(setup);
        self.pair.wraps = wraps;
        self
    }

    pub(in crate::lowering) fn payload_identifier(&self) -> Option<&GoIdentifier> {
        self.pair.value.name()
    }

    pub(in crate::lowering) fn error_body(
        &self,
        build: impl FnOnce(&GoIdentifier) -> LoweredBlock,
    ) -> FailureBody {
        self.pair.error_body(build)
    }

    pub(in crate::lowering) fn into_value(self) -> BoundValue<P> {
        BoundValue {
            statements: self.statements,
            source: BoundSource::Pair(self.pair),
        }
    }
}

impl BoundPair {
    pub(in crate::lowering) fn propagate(
        self,
        mut exit: impl FnMut(GoExpression) -> (Vec<Statement>, Vec<GoExpression>),
    ) -> (Vec<Statement>, Option<GoIdentifier>) {
        let Self {
            mut statements,
            pair:
                LoweredPair {
                    value,
                    status,
                    status_kind,
                    initializer_call,
                    wraps,
                    borrowed: _,
                },
        } = self;
        let error = wrap_error(&wraps, GoExpression::identifier(status.clone()));
        let (setup, values) = exit(error);
        let status_test = status_test(&status, status_kind, Polarity::Negated);
        let initializer = initializer_call.map(|call| Definition {
            names: value.binding(&status),
            value: call,
        });
        statements.push(tag_check_with_initializer(initializer, status_test, setup, values).into());
        if let Some((name, guard)) = guarded_value(&value, status_kind) {
            let error = wrap_error(&wraps, unexpected_nil_error());
            let (setup, values) = exit(error);
            let check = guard.nil_condition(GoExpression::identifier(name.clone()));
            statements.push(tag_check_with_initializer(None, check, setup, values).into());
        }
        (statements, value.name().cloned())
    }
}

impl<P: SlotPlacement> BoundValue<P> {
    pub(in crate::lowering) fn nullable(
        mut statements: Vec<Statement>,
        value: GoIdentifier,
        nil_guard: NilGuard,
        call: GoExpression,
        opens_if: bool,
    ) -> Self {
        let initializer_call = P::place(opens_if, call, |call| {
            statements.push(define(value.clone(), call));
        });
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
        let target = target.filter(|_| P::binds_index_target());
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

    pub(in crate::lowering) fn extend_setup(&mut self, setup: Vec<Statement>) {
        self.statements.extend(setup);
    }

    pub(in crate::lowering) fn payload(&self) -> Option<GoExpression> {
        match &self.source {
            BoundSource::Index { index, .. } => Some(index.element()),
            BoundSource::Pair(_) | BoundSource::Nullable { .. } | BoundSource::Found { .. } => self
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

    pub(in crate::lowering) fn binds_value(&self) -> bool {
        !matches!(self.source, BoundSource::Index { .. })
    }

    fn late_binding(&self) -> Option<Statement> {
        let BoundSource::Index {
            index,
            target: Some(target),
        } = &self.source
        else {
            return None;
        };

        Some(define(target.clone(), index.element()))
    }

    fn condition(&self, polarity: Polarity) -> P::Condition {
        let plain = |test: GoExpression| P::condition(&P::Initializer::default(), Vec::new, test);
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
                P::condition(initializer_call, || vec![value.clone()], test)
            }
            BoundSource::Index { index, .. } => plain(index.condition(polarity)),
            BoundSource::Found { flag, .. } if polarity == Polarity::Positive => {
                plain(GoExpression::identifier(flag.clone()))
            }
            BoundSource::Found { flag, .. } => plain(GoExpression::unary(
                GoUnaryOperator::Not,
                GoExpression::identifier(flag.clone()),
            )),
        }
    }
}

impl BoundValue {
    pub(in crate::lowering) fn failure_body(
        &self,
        build: impl FnOnce() -> LoweredBlock,
    ) -> FailureBody {
        match &self.source {
            BoundSource::Pair(pair) => pair.error_body(|_| build()),
            BoundSource::Nullable { .. }
            | BoundSource::Index { .. }
            | BoundSource::Found { .. } => FailureBody(build()),
        }
    }

    pub(in crate::lowering) fn guard(
        self,
        failure: FailureBody,
        origin: IfOrigin,
    ) -> Vec<Statement> {
        self.branch(Arms::FailureOnly, failure, origin)
    }

    pub(in crate::lowering) fn branch(
        self,
        arms: Arms,
        failure: FailureBody,
        origin: IfOrigin,
    ) -> Vec<Statement> {
        let late = self.late_binding();
        let FailureBody(failure) = failure;
        let (plan, after) = match arms {
            Arms::SuccessFirst {
                success,
                failure_layout,
            } => {
                let else_arm = match failure_layout {
                    ElseLayout::Block => ElseArm::block(failure),
                    ElseLayout::Inline => ElseArm::inline(failure),
                };
                let test = self.condition(Polarity::Positive);
                let success = after_late_binding(late, success);
                (IfPlan::pair_if(test, success, else_arm, origin), None)
            }
            Arms::FailureFirst { success } => {
                let test = self.condition(Polarity::Negated);
                let else_arm = ElseArm::block(after_late_binding(late, success));
                (IfPlan::pair_if(test, failure, else_arm, origin), None)
            }
            Arms::FailureOnly => {
                let test = self.condition(Polarity::Negated);
                (IfPlan::pair_if(test, failure, ElseArm::None, origin), late)
            }
        };
        let Self {
            mut statements,
            source: _,
        } = self;
        statements.push(LoweredStatement::If(plan).into());
        statements.extend(after);
        statements
    }
}

fn after_late_binding(late: Option<Statement>, success: LoweredBlock) -> LoweredBlock {
    let Some(late) = late else {
        return success;
    };
    let mut statements = vec![late];
    statements.extend(success.statements);
    LoweredBlock { statements }
}

impl<P: PredicatePlacement> BoundValue<P> {
    pub(in crate::lowering) fn into_test(
        self,
        polarity: Polarity,
    ) -> (Vec<Statement>, P::Condition) {
        let condition = self.condition(polarity);
        (self.statements, condition)
    }
}
