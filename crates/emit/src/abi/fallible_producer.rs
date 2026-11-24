use crate::Planner;
use crate::abi::callable::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::transition::tag_check_with_initializer;
use crate::abi::wrapping::NilGuard;
use crate::calls::NativeCallContext;
use crate::calls::bound_value::BoundValue;
use crate::calls::comma_ok::{CommaOkSource, CommaOkValueSlot, PairKind};
use crate::calls::slice_loop::FoundSink;
use crate::calls::wrap_err::WrapMessage;
use crate::control_flow::fallible::Fallible;
use crate::control_flow::fallible::prelude_call;
use crate::control_flow::fallible_blocks::TryBlockPairPlan;
use crate::ir::bodies::{Statement, define};
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::source_name::SourceName;
use crate::state::bindings::{ComponentBinding, ComponentKind};
use crate::state::expression::ExpressionContext;
use crate::types::go_type::GoType;
use syntax::ast::{Expression, Literal};
use syntax::program::NativeOperation;
use syntax::program::NativeTypeKind;
use syntax::types::Type;

pub(crate) struct ResultFusePlan<'a> {
    subject: &'a Expression,
    shape: LoweredReturnAbi,
    nil_guard: Option<NilGuard>,
    wraps: Vec<&'a Expression>,
    bound: Option<ComponentBinding>,
}

pub(crate) enum OptionFusePlan<'a> {
    Bound(ComponentBinding),
    CommaOk {
        subject: &'a Expression,
        source: CommaOkSource,
    },
    Nullable {
        subject: &'a Expression,
        nil_guard: NilGuard,
    },

    Index {
        call: NativeCallContext<'a>,
    },
    Found {
        call: NativeCallContext<'a>,
    },
}

impl OptionFusePlan<'_> {
    pub(crate) fn bind(self, planner: &mut Planner<'_>, slot: CommaOkValueSlot) -> BoundValue {
        match self {
            Self::Bound(components) => planner.bind_component_pair(components, slot),
            Self::CommaOk { subject, source } => planner.bind_comma_ok_pair(subject, source, slot),
            Self::Nullable { subject, nil_guard } => {
                let value = if matches!(subject, Expression::DotAccess { .. }) {
                    planner.plan_raw_nullable_field(subject)
                } else {
                    planner.plan_call_expression(subject, None, ExpressionContext::value())
                };
                let (mut statements, call) = value.into_parts();
                let (value, opens_if) = match slot {
                    CommaOkValueSlot::Named(name) => (name, false),
                    CommaOkValueSlot::Arm(name) => (name, true),
                    CommaOkValueSlot::Temp | CommaOkValueSlot::Discarded => {
                        (planner.fresh_declared("ret"), false)
                    }
                    CommaOkValueSlot::Unused => (planner.fresh_declared("ret"), true),
                };
                let initializer_call = if opens_if {
                    Some(call)
                } else {
                    statements.push(define(value.clone(), call));
                    None
                };
                BoundValue::nullable(statements, value, nil_guard, initializer_call)
            }
            Self::Index { call } => {
                let (statements, index) = planner.lower_bounds_checked_index(&call);
                let target = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    _ => None,
                };
                BoundValue::index(statements, index, target)
            }
            Self::Found { call } => {
                let value = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    CommaOkValueSlot::Arm(name) => {
                        let name = planner.declared_arm_value_name(&name);
                        Some(name)
                    }
                    CommaOkValueSlot::Temp => Some(planner.fresh_declared("ret")),
                    CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => None,
                };
                let flag = planner.fresh_declared("found");
                let statements = planner.lower_find_loop(
                    &call,
                    FoundSink {
                        value: value.as_ref(),
                        flag: &flag,
                    },
                );
                BoundValue::found(statements, value, flag)
            }
        }
    }
}

impl ResultFusePlan<'_> {
    pub(crate) fn wraps_error(&self) -> bool {
        !self.wraps.is_empty()
    }

    pub(crate) fn is_bound(&self) -> bool {
        self.bound.is_some()
    }

    pub(crate) fn has_nil_guard(&self) -> bool {
        self.nil_guard.is_some()
    }

    pub(crate) fn shape(&self) -> &LoweredReturnAbi {
        &self.shape
    }

    pub(crate) fn carries_payload(&self) -> bool {
        matches!(self.shape, LoweredReturnAbi::Result { .. })
    }

    pub(crate) fn bind(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&SourceName>,
    ) -> BoundValue {
        self.bind_wrapped(planner, slot, error_name, false).0
    }

    pub(crate) fn bind_wrapped(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
        error_name: Option<&SourceName>,
        read_error: bool,
    ) -> (BoundValue, Vec<WrapMessage>) {
        if let Some(components) = self.bound {
            return (planner.bind_component_pair(components, slot), Vec::new());
        }

        let carries_value = self.carries_payload();
        let (setup, call) = planner
            .plan_call_expression(self.subject, None, ExpressionContext::value())
            .into_parts();
        let (message_setup, messages) = planner.prepare_wrap_messages(&self.wraps, read_error);

        let slot = if message_setup.is_empty() {
            slot
        } else {
            match slot {
                CommaOkValueSlot::Arm(name) => {
                    let name = planner.declared_arm_value_name(&name);
                    CommaOkValueSlot::Named(name)
                }
                CommaOkValueSlot::Unused => CommaOkValueSlot::Discarded,
                slot => slot,
            }
        };
        let mut pair = planner.bind_pair(
            setup,
            call,
            slot,
            if carries_value {
                PairKind::Result {
                    nil_guard: self.nil_guard,
                }
            } else {
                PairKind::BareError
            },
            error_name,
        );
        pair.statements.extend(message_setup);
        (pair, messages)
    }
}

pub(crate) enum FallibleProducer<'a> {
    Result(ResultFusePlan<'a>),
    Option(OptionFusePlan<'a>),
    TryBlock(TryBlockPairPlan<'a>),
}

pub(crate) struct GuardedPayload {
    pub(crate) statements: Vec<Statement>,
    pub(crate) payload: Option<GoExpression>,
    pub(crate) payload_is_index: bool,
}

impl FallibleProducer<'_> {
    pub(crate) fn bind_slot(self, planner: &mut Planner<'_>, slot: CommaOkValueSlot) -> BoundValue {
        match self {
            Self::Result(producer) => producer.bind(planner, slot, None),
            Self::Option(producer) => producer.bind(planner, slot),
            Self::TryBlock(producer) => producer.bind(planner, slot),
        }
    }

    pub(crate) fn payload(
        self,
        planner: &mut Planner<'_>,
        slot: CommaOkValueSlot,
    ) -> (BoundValue, GoIdentifier) {
        let mut bound = self.bind_slot(planner, slot);
        let value = bound.writable_payload(planner);
        (bound, value)
    }

    pub(crate) fn components(
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
                .unwrap_or_else(|| GoIdentifier::external("_".to_string())),
            status: bound.status_identifier().clone(),
            payload_type,
            kind,
            shared_payload,
        };
        Some((bound.statements, components))
    }

    pub(crate) fn control_transfer(
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
                !producer.has_nil_guard() && !producer.wraps_error() && producer.carries_payload()
            }
            Self::Option(OptionFusePlan::CommaOk { source, .. }) => !source.has_nil_guard(),
            Self::TryBlock(_) => true,
            Self::Option(_) => false,
        }
    }
}

impl<'p> Planner<'p> {
    pub(crate) fn rebuild_from_components(
        &mut self,
        components: &ComponentBinding,
    ) -> GoExpression {
        let constructor = components
            .kind
            .whole_value_constructor()
            .expect("component binding has no whole-value constructor");
        let payload_go_type = self.go_type(&components.payload_type);
        prelude_call(
            constructor,
            GoType::type_argument(&payload_go_type),
            vec![
                GoExpression::identifier(components.value.clone()),
                GoExpression::identifier(components.status.clone()),
            ],
        )
    }

    pub(crate) fn fallible_producer<'a>(
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

    pub(crate) fn component_producer<'a>(
        &self,
        subject: &'a Expression,
        needs_whole_value: bool,
        kind: ComponentKind,
    ) -> Option<FallibleProducer<'a>>
    where
        'p: 'a,
    {
        if needs_whole_value && kind.whole_value_constructor().is_none() {
            return None;
        }
        let producer = if let Expression::TryBlock { items, ty, .. } = subject {
            FallibleProducer::TryBlock(self.try_block_pair_plan(items, ty)?)
        } else {
            self.fallible_producer(subject)?
        };
        producer.supports_components().then_some(producer)
    }

    pub(crate) fn result_fuse_plan<'a>(
        &self,
        subject: &'a Expression,
    ) -> Option<ResultFusePlan<'a>> {
        if let Some(components) = self.component_binding(subject) {
            return (components.kind.is_result()).then(|| ResultFusePlan {
                subject,
                shape: LoweredReturnAbi::Result {
                    payload: PayloadLayout::Packed,
                },
                nil_guard: None,
                wraps: Vec::new(),
                bound: Some(components),
            });
        }
        let lowered = self.lowered_call(subject)?;
        if !lowered.is_result() || lowered.is_bridged() {
            return None;
        }
        if lowered.from_go && lowered.has_tuple_payload(self) {
            return None;
        }
        let nil_guard = lowered.nil_guard;
        Some(ResultFusePlan {
            subject: lowered.call,
            shape: lowered.shape,
            nil_guard,
            wraps: lowered.wraps,
            bound: None,
        })
    }

    fn index_fuses(&self, receiver: &Expression, index: &Expression) -> bool {
        match index {
            Expression::Literal {
                literal: Literal::Integer { value, .. },
                ..
            } => self
                .facts
                .array_length_behind_refs(&receiver.get_type())
                .is_none_or(|length| *value < length),
            other => !self.is_go_constant_expression(other),
        }
    }

    pub(crate) fn option_fuse_plan<'a>(&self, subject: &'a Expression) -> Option<OptionFusePlan<'a>>
    where
        'p: 'a,
    {
        if let Some(components) = self.component_binding(subject) {
            return (components.kind == ComponentKind::Option)
                .then_some(OptionFusePlan::Bound(components));
        }
        if let Some(source) = self.comma_ok_source(subject) {
            return Some(OptionFusePlan::CommaOk { subject, source });
        }
        if let Some(call) = self.native_method_call(subject) {
            let (receiver, arguments) = call.receiver_and_arguments()?;
            let indexes = call.native_type != NativeTypeKind::Map
                && NativeOperation::new(call.native_type, call.method).indexes_receiver()
                && arguments.len() == 1
                && call.spread.is_none()
                && self.index_fuses(receiver, &arguments[0]);
            if indexes {
                return Some(OptionFusePlan::Index { call });
            }
            if self.find_loop_fuses(&call) {
                return Some(OptionFusePlan::Found { call });
            }
        }

        if let Some(nil_guard) = self.nullable_field_guard(subject) {
            return Some(OptionFusePlan::Nullable { subject, nil_guard });
        }

        let lowered = self.lowered_call(subject)?;
        if lowered.is_bridged() {
            return None;
        }
        if !matches!(
            lowered.shape,
            LoweredReturnAbi::Option(OptionReturnAbi::Nullable | OptionReturnAbi::Sentinel(_))
        ) {
            return None;
        }
        let nil_guard = lowered.nil_guard?;
        Some(OptionFusePlan::Nullable { subject, nil_guard })
    }
}
