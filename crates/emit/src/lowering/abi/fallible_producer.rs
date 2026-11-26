use crate::Planner;
use crate::lookup::facts::component_uses::ValueDemand;
use crate::lookup::state::bindings::{ComponentBinding, ComponentKind};
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::abi::go_returns::tag_check_with_initializer;
use crate::lowering::calls::bound_value::BoundValue;
use crate::lowering::calls::bounds_checks::IndexSite;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::comma_ok::{CommaOkSource, CommaOkValueSlot, PairKind};
use crate::lowering::calls::slice_loop::{FindLoop, FoundSink};
use crate::lowering::calls::wrap_err::WrapMessage;
use crate::lowering::control_flow::try_recover_blocks::TryBlockPairPlan;
use crate::lowering::expressions::access::dot_access_site::DotAccessSite;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::{Statement, define};
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
    Call {
        site: CallSite<'a>,
        nil_guard: Option<NilGuard>,
        wraps: Vec<&'a Expression>,
    },
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
    pub(in crate::lowering) fn bind(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        match self {
            Self::Bound(components) => planner.bind_component_pair(components, slot),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum ErrorNilGuard {
    Guarded,
    Unguarded,
}

impl ResultFusePlan<'_> {
    pub(in crate::lowering) fn wraps_error(&self) -> bool {
        matches!(&self.source, ResultFuseSource::Call { wraps, .. } if !wraps.is_empty())
    }

    pub(in crate::lowering) fn is_bound(&self) -> bool {
        matches!(self.source, ResultFuseSource::Bound(_))
    }

    pub(in crate::lowering) fn error_nil_guard(&self) -> ErrorNilGuard {
        match self.source {
            ResultFuseSource::Call {
                nil_guard: Some(_), ..
            } => ErrorNilGuard::Guarded,
            ResultFuseSource::Call {
                nil_guard: None, ..
            }
            | ResultFuseSource::Bound(_) => ErrorNilGuard::Unguarded,
        }
    }

    pub(in crate::lowering) fn shape(&self) -> &LoweredReturnAbi {
        &self.shape
    }

    pub(in crate::lowering) fn carries_payload(&self) -> bool {
        matches!(self.shape, LoweredReturnAbi::Result { .. })
    }

    pub(in crate::lowering) fn bind(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&SourceName>,
    ) -> BoundValue {
        self.bind_wrapped(planner, slot, error_name).0
    }

    pub(in crate::lowering) fn bind_wrapped(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&SourceName>,
    ) -> (BoundValue, Vec<WrapMessage>) {
        let carries_value = self.carries_payload();
        let (site, nil_guard, wraps) = match self.source {
            ResultFuseSource::Bound(components) => {
                return (planner.bind_component_pair(components, slot), Vec::new());
            }
            ResultFuseSource::Call {
                site,
                nil_guard,
                wraps,
            } => (site, nil_guard, wraps),
        };
        let (setup, call) = planner
            .plan_call_expression(site, None, ExpressionContext::value())
            .into_parts();
        let (message_setup, messages) = planner.prepare_wrap_messages(&wraps);

        let slot = if message_setup.is_empty() {
            slot
        } else {
            match slot {
                CommaOkValueSlot::Arm(name) => {
                    let name = planner.declared_arm_value_name(&name);
                    CommaOkValueSlot::Named(name)
                }
                CommaOkValueSlot::Unused => CommaOkValueSlot::Discarded,
                slot @ CommaOkValueSlot::Named(_)
                | slot @ CommaOkValueSlot::Temp
                | slot @ CommaOkValueSlot::Discarded => slot,
            }
        };

        let mut pair = planner.bind_pair(
            setup,
            call,
            slot,
            if carries_value {
                PairKind::Result { nil_guard }
            } else {
                PairKind::BareError
            },
            error_name,
        );

        pair.statements.extend(message_setup);
        (pair, messages)
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

impl FallibleProducer<'_> {
    pub(in crate::lowering) fn bind_slot(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        match self {
            Self::Result(producer) => producer.bind(planner, slot, None),
            Self::Option(producer) => producer.bind(planner, slot),
            Self::TryBlock(producer) => producer.bind(planner, slot),
        }
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

    pub(in crate::lowering) fn components(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        payload_type: Type,
        kind: ComponentKind,
        shared_payload: bool,
    ) -> Option<(Vec<Statement>, ComponentBinding)> {
        if !self.supports_components() {
            return None;
        }
        let bound = self.bind_slot(planner, slot);
        let components = ComponentBinding {
            value: bound
                .payload_identifier()
                .cloned()
                .unwrap_or_else(GoIdentifier::blank),
            status: bound.status_identifier().clone(),
            payload_type,
            kind,
            shared_payload,
        };

        Some((bound.statements, components))
    }

    pub(in crate::lowering) fn control_transfer(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        failure_values: Vec<GoExpression>,
    ) -> GuardedPayload {
        let bound = self.bind_slot(planner, slot);
        let condition = bound.failure_condition();
        let late = bound.late_binding();
        let payload = bound.payload();
        let payload_is_index = !bound.binds_value();
        let mut statements = bound.statements;
        statements.push(
            tag_check_with_initializer(
                condition.initializer,
                condition.condition,
                Vec::new(),
                failure_values,
            )
            .into(),
        );
        statements.extend(late);
        GuardedPayload {
            statements,
            payload,
            payload_is_index,
        }
    }

    fn supports_components(&self) -> bool {
        match self {
            Self::Result(producer) => {
                producer.error_nil_guard() == ErrorNilGuard::Unguarded
                    && !producer.wraps_error()
                    && producer.carries_payload()
            }
            Self::Option(OptionFusePlan::CommaOk { source, .. }) => !source.has_nil_guard(),
            Self::TryBlock(_) => true,
            Self::Option(_) => false,
        }
    }
}

impl<'p> Planner<'p> {
    fn bind_nullable_pair(
        &mut self,
        subject: NullableSource<'_>,
        nil_guard: NilGuard,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let value = match subject {
            NullableSource::Field(field) => self.plan_raw_nullable_field(field),
            NullableSource::Call(site) => {
                self.plan_call_expression(site, None, ExpressionContext::value())
            }
        };
        let (mut statements, call) = value.into_parts();
        let (value, opens_if) = match slot {
            CommaOkValueSlot::Named(name) => (name, false),
            CommaOkValueSlot::Arm(name) => (name, true),
            CommaOkValueSlot::Temp | CommaOkValueSlot::Discarded => {
                (self.fresh_declared("ret"), false)
            }
            CommaOkValueSlot::Unused => (self.fresh_declared("ret"), true),
        };

        let initializer_call = if opens_if {
            Some(call)
        } else {
            statements.push(define(value.clone(), call));
            None
        };

        BoundValue::nullable(statements, value, nil_guard, initializer_call)
    }

    fn bind_found_pair(&mut self, search: FindLoop<'_>, slot: CommaOkValueSlot) -> BoundValue {
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
    ) -> Option<FallibleProducer<'a>>
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
        producer.supports_components().then_some(producer)
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
        Some(ResultFusePlan {
            shape: lowered.shape,
            source: ResultFuseSource::Call {
                site: lowered.site,
                nil_guard: lowered.nil_guard,
                wraps: lowered.wraps,
            },
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
