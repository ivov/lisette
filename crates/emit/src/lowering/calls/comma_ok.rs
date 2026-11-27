use crate::Planner;
use crate::lookup::state::bindings::{BindingValue, ComponentBinding, ComponentKind};
use crate::lookup::state::scope::PairStatusKind;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::calls::bound_value::{BoundPair, BoundValue, PairValue, SlotPlacement};
use crate::lowering::calls::call_plan::CallableOrigin;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::calls::wrap_err::WrapChain;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::{Statement, define, define_many};
use crate::target::ir::local::GoIdentifier;
use crate::target::ir::values::GoExpression;
use crate::target::names::source_name::SourceName;
use crate::target::types::return_abi::{LoweredReturnAbi, OptionReturnAbi};
use syntax::ast::Expression;
use syntax::program::CallOperation;
use syntax::program::NativeOperation;
use syntax::program::NativeTypeKind;

pub(in crate::lowering) enum CommaOkSource<'a> {
    LoweredCall { nil_guard: Option<NilGuard> },
    MapIndex { receiver: &'a Expression },
    TypeAssert,
}

impl<'a> CommaOkSource<'a> {
    fn nil_guard(&self) -> Option<NilGuard> {
        match self {
            Self::LoweredCall { nil_guard } => *nil_guard,
            Self::MapIndex { .. } | Self::TypeAssert => None,
        }
    }

    pub(in crate::lowering) fn plain(self) -> Option<PlainCommaOk<'a>> {
        self.nil_guard().is_none().then_some(PlainCommaOk(self))
    }

    pub(in crate::lowering) fn map_index_receiver(&self) -> Option<&'a Expression> {
        match self {
            Self::MapIndex { receiver } => Some(receiver),
            Self::LoweredCall { .. } | Self::TypeAssert => None,
        }
    }
}

pub(in crate::lowering) struct PlainCommaOk<'a>(CommaOkSource<'a>);

pub(in crate::lowering) enum CommaOkValueSlot {
    Named(GoIdentifier),
    Temp,
    Arm(GoIdentifier),
    Unused,
    Discarded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lowering) enum PairPlacement {
    IfInitializer,
    Statement,
}

pub(in crate::lowering) enum PairKind {
    CommaOk { nil_guard: Option<NilGuard> },
    Result { nil_guard: Option<NilGuard> },
    BareError,
}

#[derive(Clone, Copy)]
pub(in crate::lowering) enum PlainPairKind {
    CommaOk,
    Result,
}

impl From<PlainPairKind> for PairKind {
    fn from(kind: PlainPairKind) -> Self {
        match kind {
            PlainPairKind::CommaOk => PairKind::CommaOk { nil_guard: None },
            PlainPairKind::Result => PairKind::Result { nil_guard: None },
        }
    }
}

pub(in crate::lowering) enum StatementSlot {
    Named(GoIdentifier),
    Temp,
    Discarded,
}

impl From<StatementSlot> for CommaOkValueSlot {
    fn from(slot: StatementSlot) -> Self {
        match slot {
            StatementSlot::Named(name) => CommaOkValueSlot::Named(name),
            StatementSlot::Temp => CommaOkValueSlot::Temp,
            StatementSlot::Discarded => CommaOkValueSlot::Discarded,
        }
    }
}

pub(in crate::lowering) struct PlainPair {
    pub(in crate::lowering) statements: Vec<Statement>,
    pub(in crate::lowering) value: Option<GoIdentifier>,
    pub(in crate::lowering) status: GoIdentifier,
}

impl Planner<'_> {
    fn component_value(
        &mut self,
        components: &ComponentBinding,
        slot: CommaOkValueSlot,
    ) -> (Vec<Statement>, GoIdentifier, bool) {
        match slot {
            CommaOkValueSlot::Named(name) if name.spelling() != components.value.spelling() => {
                self.scope.declare_go_name(name.spelling());
                let copy = define(
                    name.clone(),
                    GoExpression::identifier(components.value.clone()),
                );
                (vec![copy], name, false)
            }
            CommaOkValueSlot::Named(_)
            | CommaOkValueSlot::Temp
            | CommaOkValueSlot::Arm(_)
            | CommaOkValueSlot::Unused
            | CommaOkValueSlot::Discarded => (
                Vec::new(),
                components.value.clone(),
                components.shared_payload,
            ),
        }
    }

    pub(in crate::lowering) fn bind_component_pair<P: SlotPlacement>(
        &mut self,
        components: ComponentBinding,
        slot: CommaOkValueSlot,
    ) -> BoundPair<P> {
        let (statements, value, borrowed) = self.component_value(&components, slot);
        let status_kind = match components.kind {
            ComponentKind::Option => PairStatusKind::Ok,
            ComponentKind::ErrorResult | ComponentKind::OtherResult => PairStatusKind::Error,
        };
        BoundPair::existing(statements, value, components.status, status_kind, borrowed)
    }

    pub(in crate::lowering) fn bind_plain_component_pair(
        &mut self,
        components: ComponentBinding,
        slot: StatementSlot,
    ) -> PlainPair {
        let (statements, value, _) = self.component_value(&components, slot.into());
        PlainPair {
            statements,
            value: Some(value),
            status: components.status,
        }
    }

    pub(in crate::lowering) fn bind_comma_ok_pair<P: SlotPlacement>(
        &mut self,
        site: CallSite<'_>,
        source: CommaOkSource<'_>,
        slot: CommaOkValueSlot,
    ) -> BoundValue<P> {
        let (statements, pair) = self.plan_comma_ok_pair(site, &source);
        self.bind_pair(
            statements,
            pair,
            slot,
            PairKind::CommaOk {
                nil_guard: source.nil_guard(),
            },
            None,
        )
        .into_value()
    }

    pub(in crate::lowering) fn bind_plain_comma_ok_pair(
        &mut self,
        site: CallSite<'_>,
        source: PlainCommaOk<'_>,
        slot: StatementSlot,
    ) -> PlainPair {
        let (statements, pair) = self.plan_comma_ok_pair(site, &source.0);
        self.bind_plain_pair(statements, pair, slot, PlainPairKind::CommaOk)
    }

    fn pair_names(
        &mut self,
        slot: CommaOkValueSlot,
        kind: PairKind,
        status_hint: Option<&SourceName>,
        placement: PairPlacement,
    ) -> (PairValue, GoIdentifier, PairStatusKind) {
        let status_kind = match kind {
            PairKind::CommaOk { .. } => PairStatusKind::Ok,
            PairKind::Result { .. } | PairKind::BareError => PairStatusKind::Error,
        };
        let value = match kind {
            PairKind::BareError => PairValue::Absent,
            PairKind::CommaOk { nil_guard } | PairKind::Result { nil_guard } => {
                let name = match slot {
                    CommaOkValueSlot::Named(name) | CommaOkValueSlot::Arm(name) => Some(name),
                    CommaOkValueSlot::Temp => Some(self.fresh_declared("ret")),
                    CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => {
                        nil_guard.map(|_| self.fresh_declared("ret"))
                    }
                };

                match name {
                    Some(name) => PairValue::Named { name, nil_guard },
                    None => PairValue::Discarded,
                }
            }
        };

        let mut status = self.pair_status(status_hint, status_kind, placement);
        if placement == PairPlacement::IfInitializer
            && value.name().map(GoIdentifier::spelling) == Some(status.spelling())
        {
            self.scope.reserve_go_name(&status);
            status = self.fresh_local(&status);
        }
        (value, status, status_kind)
    }

    pub(in crate::lowering) fn bind_pair<P: SlotPlacement>(
        &mut self,
        statements: Vec<Statement>,
        expression: GoExpression,
        slot: CommaOkValueSlot,
        kind: PairKind,
        status_hint: Option<&SourceName>,
    ) -> BoundPair<P> {
        let opens_if = P::opens_if(&slot);
        let placement = if opens_if {
            PairPlacement::IfInitializer
        } else {
            PairPlacement::Statement
        };
        let (value, status, status_kind) = self.pair_names(slot, kind, status_hint, placement);
        BoundPair::place(statements, value, status, status_kind, expression, opens_if)
    }

    pub(in crate::lowering) fn bind_plain_pair(
        &mut self,
        mut statements: Vec<Statement>,
        expression: GoExpression,
        slot: StatementSlot,
        kind: PlainPairKind,
    ) -> PlainPair {
        let (value, status, _) =
            self.pair_names(slot.into(), kind.into(), None, PairPlacement::Statement);
        statements.push(define_many(value.binding(&status), expression));
        PlainPair {
            statements,
            value: value.name().cloned(),
            status,
        }
    }

    pub(in crate::lowering) fn arm_value_name(&mut self, name: &SourceName) -> GoIdentifier {
        self.scope.new_unreserved_local(name, self.names)
    }

    pub(in crate::lowering) fn declared_arm_value_name(
        &mut self,
        name: &GoIdentifier,
    ) -> GoIdentifier {
        self.claim_go_name(name)
    }

    pub(in crate::lowering) fn pair_status(
        &mut self,
        hint: Option<&SourceName>,
        kind: PairStatusKind,
        placement: PairPlacement,
    ) -> GoIdentifier {
        let default = match kind {
            PairStatusKind::Error => "err",
            PairStatusKind::Ok => "ok",
        };
        let candidate = hint.map_or_else(|| default.to_string(), SourceName::preferred);
        let name = self.scope.new_unreserved_local(candidate, self.names);
        if placement == PairPlacement::Statement {
            self.scope.declare_go_name(&name);
        }
        name
    }

    fn plan_comma_ok_pair(
        &mut self,
        site: CallSite<'_>,
        source: &CommaOkSource<'_>,
    ) -> (Vec<Statement>, GoExpression) {
        match source {
            CommaOkSource::LoweredCall { .. } => self
                .plan_call_expression(site, None, ExpressionContext::value())
                .into_parts(),
            CommaOkSource::MapIndex { receiver } => self.plan_map_index_pair(site, receiver),
            CommaOkSource::TypeAssert => {
                let args = site.args;
                let expression = site.expression;
                let (setup, operand) = self
                    .plan_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                let target_ty = self.facts.ok_payload(&expression.get_type());
                let target = self.queries().go_type(&target_ty);
                (setup, GoExpression::type_assertion(operand, target))
            }
        }
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn component_binding(
        &self,
        expression: &Expression,
    ) -> Option<ComponentBinding> {
        let Expression::Identifier {
            value, resolution, ..
        } = expression
        else {
            return None;
        };

        match self
            .scope
            .resolve_identifier_with_resolution(value, resolution)
        {
            Some(BindingValue::Components(components)) => Some(components.clone()),
            _ => None,
        }
    }

    pub(in crate::lowering) fn comma_ok_source<'s>(
        &self,
        site: CallSite<'s>,
    ) -> Option<CommaOkSource<'s>> {
        let plan = self.plan_call(site);
        let expression = site.expression;
        let function = site.callee;
        let args = site.args;
        let spread = site.spread;

        match &plan.resolved.origin {
            CallableOrigin::Source(CallOperation::AssertType) => {
                let [operand] = args else {
                    return None;
                };
                let operand_ty = operand.get_type();
                let asserts_on_interface =
                    self.facts.is_interface_or_unknown(&operand_ty) || operand_ty.is_error();
                let asserts_function = self
                    .any_slot_function_layouts(&self.facts.ok_payload(&expression.get_type()))
                    .is_some();
                (asserts_on_interface && !asserts_function).then_some(CommaOkSource::TypeAssert)
            }
            CallableOrigin::Source(CallOperation::Native {
                kind: kind @ NativeTypeKind::Map,
                method,
            }) if NativeOperation::new(*kind, method).indexes_receiver()
                && args.len() == 1
                && spread.is_none()
                && let Expression::DotAccess {
                    expression: receiver,
                    ..
                } = function =>
            {
                Some(CommaOkSource::MapIndex { receiver })
            }
            CallableOrigin::GoInterop | CallableOrigin::Source(_) => {
                let lowered = self.lowered_call_of(WrapChain::default(), &plan)?;
                let pair = lowered.raw_pair()?;
                if !matches!(
                    lowered.shape,
                    LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })
                ) || pair.payload_layout_change.is_some()
                {
                    return None;
                }
                Some(CommaOkSource::LoweredCall {
                    nil_guard: pair.value.and_then(|slot| slot.nil_guard),
                })
            }
        }
    }
}
