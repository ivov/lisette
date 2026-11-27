use crate::Planner;
use crate::lookup::facts::component_uses::ValueDemand;
use crate::lookup::state::bindings::{ComponentBinding, ComponentKind};
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::abi::go_returns::multi_value_return;
use crate::lowering::calls::bound_value::{
    BoundPair, BoundValue, PredicatePlacement, SlotPlacement,
};
use crate::lowering::calls::bounds_checks::IndexSite;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::{
    CommaOkSource, CommaOkValueSlot, PlainCommaOk, PlainPairKind, StatementSlot,
};
use crate::lowering::calls::predicates::Polarity;
use crate::lowering::calls::slice_loop::{FindLoop, FoundSink};
use crate::lowering::calls::wrap_err::FusedCall;
use crate::lowering::control_flow::try_recover_blocks::TryBlockPairPlan;
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::{IfOrigin, LoweredBlock, Statement};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::prelude::Fallible;
use crate::target::ir::prelude::prelude_call;
use crate::target::ir::values::GoExpression;
use crate::target::names::source_name::SourceName;
use crate::target::types::go_type::GoType;
use crate::target::types::return_abi::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use syntax::ast::{Expression, Literal};
use syntax::program::NativeOperation;
use syntax::program::NativeTypeKind;
use syntax::types::Type;

pub(in crate::lowering) struct ResultFusePlan<'a> {
    shape: LoweredReturnAbi,
    source: ResultFuseSource<'a>,
}

enum ResultFuseSource<'a> {
    Bound(ComponentBinding),
    Call(FusedCall<'a>),
}

pub(in crate::lowering) enum ComponentSource<'a> {
    Bound(ComponentBinding),
    ResultCall(CallSite<'a>),
    CommaOk {
        subject: CallSite<'a>,
        source: PlainCommaOk<'a>,
    },
    TryBlock(TryBlockPairPlan<'a>),
}

pub(in crate::lowering) enum NullableSource<'a> {
    Field(DotAccessSite<'a>),
    Call(CallSite<'a>),
}

pub(in crate::lowering) enum OptionFusePlan<'a> {
    Bound(ComponentBinding),
    CommaOk {
        subject: CallSite<'a>,
        source: CommaOkSource<'a>,
    },
    Nullable {
        subject: NullableSource<'a>,
        nil_guard: NilGuard,
    },

    Index {
        site: IndexSite<'a>,
    },
    Found {
        search: Box<FindLoop<'a>>,
    },
}

impl OptionFusePlan<'_> {
    pub(in crate::lowering) fn bind<P: SlotPlacement>(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue<P> {
        match self {
            Self::Bound(components) => planner
                .bind_component_pair::<P>(components, slot)
                .into_value(),
            Self::CommaOk { subject, source } => planner.bind_comma_ok_pair(subject, source, slot),
            Self::Nullable { subject, nil_guard } => {
                planner.bind_nullable_pair(subject, nil_guard, slot)
            }
            Self::Index { site } => {
                let (statements, index) = planner.lower_bounds_checked_index(site);
                let target = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    CommaOkValueSlot::Temp
                    | CommaOkValueSlot::Arm(_)
                    | CommaOkValueSlot::Unused
                    | CommaOkValueSlot::Discarded => None,
                };
                BoundValue::index(statements, index, target)
            }
            Self::Found { search } => planner.bind_found_pair(*search, slot),
        }
    }
}

impl<'a> ResultFusePlan<'a> {
    pub(in crate::lowering) fn wraps_error(&self) -> bool {
        matches!(&self.source, ResultFuseSource::Call(call) if call.wraps_error())
    }

    pub(in crate::lowering) fn is_bound(&self) -> bool {
        matches!(self.source, ResultFuseSource::Bound(_))
    }

    pub(in crate::lowering) fn shape(&self) -> &LoweredReturnAbi {
        &self.shape
    }

    pub(in crate::lowering) fn carries_payload(&self) -> bool {
        matches!(self.shape, LoweredReturnAbi::Result { .. })
    }

    pub(in crate::lowering) fn bind<P: SlotPlacement>(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&SourceName>,
    ) -> BoundPair<P> {
        let carries_value = self.carries_payload();
        match self.source {
            ResultFuseSource::Bound(components) => planner.bind_component_pair(components, slot),
            ResultFuseSource::Call(call) => {
                planner.bind_fused_call(call, slot, carries_value, error_name)
            }
        }
    }

    fn into_component_source(self) -> Option<ComponentSource<'a>> {
        if !self.carries_payload() {
            return None;
        }
        match self.source {
            ResultFuseSource::Bound(components) => Some(ComponentSource::Bound(components)),
            ResultFuseSource::Call(call) => call.into_plain_site().map(ComponentSource::ResultCall),
        }
    }
}

pub(in crate::lowering) enum FallibleProducer<'a> {
    Result(ResultFusePlan<'a>),
    Option(OptionFusePlan<'a>),
    TryBlock(TryBlockPairPlan<'a>),
}

pub(in crate::lowering) struct GuardedPayload {
    pub(in crate::lowering) statements: Vec<Statement>,
    pub(in crate::lowering) payload: Option<GoExpression>,
    pub(in crate::lowering) payload_is_index: bool,
}

impl<'a> FallibleProducer<'a> {
    pub(in crate::lowering) fn bind_slot<P: SlotPlacement>(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue<P> {
        match self {
            Self::Result(producer) => producer.bind(planner, slot, None).into_value(),
            Self::Option(producer) => producer.bind(planner, slot),
            Self::TryBlock(producer) => producer.bind(planner, slot),
        }
    }

    pub(in crate::lowering) fn bind_test<P: PredicatePlacement>(
        self,
        planner: &mut Planner<'_>,
        polarity: Polarity,
    ) -> (Vec<Statement>, P::Condition) {
        self.bind_slot::<P>(planner, P::test_slot())
            .into_test(polarity)
    }

    pub(in crate::lowering) fn payload(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> (BoundValue, GoIdentifier) {
        let mut bound = self.bind_slot(planner, slot);
        let value = bound.writable_payload(planner);
        (bound, value)
    }

    pub(in crate::lowering) fn control_transfer(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        failure_values: Vec<GoExpression>,
    ) -> GuardedPayload {
        let bound: BoundValue = self.bind_slot(planner, slot);
        let payload = bound.payload();
        let payload_is_index = !bound.binds_value();
        let failure = bound.failure_body(|| LoweredBlock {
            statements: vec![multi_value_return(failure_values)],
        });
        GuardedPayload {
            statements: bound.guard(failure, IfOrigin::User),
            payload,
            payload_is_index,
        }
    }

    fn into_component_source(self) -> Option<ComponentSource<'a>> {
        match self {
            Self::Result(producer) => producer.into_component_source(),
            Self::Option(OptionFusePlan::CommaOk { subject, source }) => source
                .plain()
                .map(|source| ComponentSource::CommaOk { subject, source }),
            Self::TryBlock(plan) => Some(ComponentSource::TryBlock(plan)),
            Self::Option(_) => None,
        }
    }
}

impl ComponentSource<'_> {
    pub(in crate::lowering) fn components(
        self,
        planner: &mut Planner<'_>,
        slot: StatementSlot,
        payload_type: Type,
        kind: ComponentKind,
        shared_payload: bool,
    ) -> (Vec<Statement>, ComponentBinding) {
        let pair = match self {
            Self::Bound(components) => planner.bind_plain_component_pair(components, slot),
            Self::ResultCall(site) => {
                let (setup, call) = planner
                    .plan_call_expression(site, None, ExpressionContext::value())
                    .into_parts();
                planner.bind_plain_pair(setup, call, slot, PlainPairKind::Result)
            }
            Self::CommaOk { subject, source } => {
                planner.bind_plain_comma_ok_pair(subject, source, slot)
            }
            Self::TryBlock(plan) => plan.bind_plain(planner, slot),
        };
        let components = ComponentBinding {
            value: pair.value.unwrap_or_else(GoIdentifier::blank),
            status: pair.status,
            payload_type,
            kind,
            shared_payload,
        };
        (pair.statements, components)
    }
}

impl<'p> Planner<'p> {
    fn bind_nullable_pair<P: SlotPlacement>(
        &mut self,
        subject: NullableSource<'_>,
        nil_guard: NilGuard,
        slot: CommaOkValueSlot,
    ) -> BoundValue<P> {
        let value = match subject {
            NullableSource::Field(field) => self.plan_raw_nullable_field(field),
            NullableSource::Call(site) => {
                self.plan_call_expression(site, None, ExpressionContext::value())
            }
        };
        let (statements, call) = value.into_parts();
        let opens_if = P::opens_if(&slot);
        let value = match slot {
            CommaOkValueSlot::Named(name) | CommaOkValueSlot::Arm(name) => name,
            CommaOkValueSlot::Temp | CommaOkValueSlot::Discarded | CommaOkValueSlot::Unused => {
                self.fresh_declared("ret")
            }
        };

        BoundValue::nullable(statements, value, nil_guard, call, opens_if)
    }

    fn bind_found_pair<P: SlotPlacement>(
        &mut self,
        search: FindLoop<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue<P> {
        let value = match slot {
            CommaOkValueSlot::Named(name) => Some(name),
            CommaOkValueSlot::Arm(name) => {
                let name = self.declared_arm_value_name(&name);
                Some(name)
            }
            CommaOkValueSlot::Temp => Some(self.fresh_declared("ret")),
            CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => None,
        };

        let flag = self.fresh_declared("found");
        let statements = self.lower_find_loop(
            search,
            FoundSink {
                value: value.as_ref(),
                flag: &flag,
            },
        );

        BoundValue::found(statements, value, flag)
    }

    pub(in crate::lowering) fn component_producer<'a>(
        &self,
        subject: &'a Expression,
        value_demand: ValueDemand,
        kind: ComponentKind,
    ) -> Option<ComponentSource<'a>>
    where
        'p: 'a,
    {
        if value_demand == ValueDemand::Whole && kind.whole_value_constructor().is_none() {
            return None;
        }
        let producer = if let Expression::TryBlock { items, ty, .. } = subject {
            FallibleProducer::TryBlock(self.try_block_pair_plan(items, ty)?)
        } else {
            self.queries().fallible_producer(subject)?
        };
        producer.into_component_source()
    }
}

impl<'p> PlannerQueries<'_, 'p> {
    pub(in crate::lowering) fn rebuild_from_components(
        &self,
        components: &ComponentBinding,
    ) -> GoExpression {
        let constructor = components
            .kind
            .whole_value_constructor()
            .expect("component binding has no whole-value constructor");
        let payload_go_type = self.go_type(&components.payload_type);
        prelude_call(
            constructor,
            Some(GoType::type_argument(&payload_go_type)),
            vec![
                GoExpression::identifier(components.value.clone()),
                GoExpression::identifier(components.status.clone()),
            ],
        )
    }

    pub(in crate::lowering) fn fallible_producer<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<FallibleProducer<'a>>
    where
        'p: 'a,
    {
        let fallible = self.facts.fallible(&subject.get_type());
        if matches!(fallible, Some(Fallible::Option { .. })) {
            self.option_fuse_plan(subject).map(FallibleProducer::Option)
        } else if matches!(fallible, Some(Fallible::Result { .. })) {
            self.result_fuse_plan(subject).map(FallibleProducer::Result)
        } else {
            None
        }
    }

    pub(in crate::lowering) fn result_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<ResultFusePlan<'a>> {
        if let Some(components) = self.component_binding(subject) {
            return (components.kind.is_result()).then(|| ResultFusePlan {
                shape: LoweredReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                },
                source: ResultFuseSource::Bound(components),
            });
        }
        let lowered = self.lowered_call(subject)?;
        if !lowered.is_result() || lowered.changes_layout() {
            return None;
        }
        if lowered.from_go && lowered.has_tuple_payload(self) {
            return None;
        }
        let shape = lowered.shape.clone();
        Some(ResultFusePlan {
            shape,
            source: ResultFuseSource::Call(lowered.into_fused_call()),
        })
    }

    fn index_fuses(&self, receiver: &Expression, index: &Expression) -> bool {
        let Expression::Literal {
            literal: Literal::Integer { value, .. },
            ..
        } = index
        else {
            return !self.is_go_constant_expression(index);
        };
        self.facts
            .receiver_array_length(&receiver.get_type())
            .is_none_or(|length| *value < length)
    }

    pub(in crate::lowering) fn option_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<OptionFusePlan<'a>>
    where
        'p: 'a,
    {
        if let Some(components) = self.component_binding(subject) {
            return (components.kind == ComponentKind::Option)
                .then_some(OptionFusePlan::Bound(components));
        }
        if let Some(site) = CallSite::of(subject)
            && let Some(source) = self.comma_ok_source(site)
        {
            return Some(OptionFusePlan::CommaOk {
                subject: site,
                source,
            });
        }
        if let Some(call) = self.native_method_call(subject) {
            let (receiver, arguments) = call.receiver_and_arguments()?;
            let indexes = call.native_type != NativeTypeKind::Map
                && NativeOperation::new(call.native_type, call.method).indexes_receiver()
                && arguments.len() == 1
                && call.spread.is_none()
                && self.index_fuses(receiver, &arguments[0]);
            if indexes {
                return Some(OptionFusePlan::Index {
                    site: IndexSite {
                        receiver,
                        index: &arguments[0],
                    },
                });
            }
            if let Some(search) = self.find_loop(call) {
                return Some(OptionFusePlan::Found {
                    search: Box::new(search),
                });
            }
        }

        if let Some(field) = DotAccessSite::of(subject)
            && let Some(nil_guard) = self.nullable_field_guard(field)
        {
            return Some(OptionFusePlan::Nullable {
                subject: NullableSource::Field(field),
                nil_guard,
            });
        }

        self.nullable_call_fuse_plan(subject)
    }

    fn nullable_call_fuse_plan<'a>(&self, subject: &'a Expression) -> Option<OptionFusePlan<'a>> {
        let lowered = self.lowered_call(subject)?;
        if lowered.changes_layout() {
            return None;
        }
        if !matches!(
            lowered.shape,
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_))
        ) {
            return None;
        }
        let nil_guard = lowered.nil_guard?;
        Some(OptionFusePlan::Nullable {
            subject: NullableSource::Call(lowered.site),
            nil_guard,
        })
    }
}
