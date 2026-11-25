use crate::Planner;
use crate::lookup::state::bindings::{BindingValue, ComponentBinding, ComponentKind};
use crate::lookup::state::scope::PairStatusKind;
use crate::lowering::abi::go_result_wrapping::NilGuard;
use crate::lowering::calls::bound_value::{BoundValue, LoweredPair, PairValue};
use crate::lowering::calls::call_plan::CallableOrigin;
use crate::lowering::calls::call_site::CallSite;
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

pub(in crate::lowering) enum CommaOkSource {
    LoweredCall { nil_guard: Option<NilGuard> },
    MapIndex,
    TypeAssert,
}

impl CommaOkSource {
    fn nil_guard(&self) -> Option<NilGuard> {
        match self {
            Self::LoweredCall { nil_guard } => *nil_guard,
            Self::MapIndex | Self::TypeAssert => None,
        }
    }

    pub(in crate::lowering) fn has_nil_guard(&self) -> bool {
        self.nil_guard().is_some()
    }

    pub(in crate::lowering) fn is_map_index(&self) -> bool {
        matches!(self, Self::MapIndex)
    }
}

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

impl Planner<'_> {
    pub(in crate::lowering) fn bind_component_pair(
        &mut self,
        components: ComponentBinding,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let (statements, value, borrowed) = match slot {
            CommaOkValueSlot::Named(name) if name.spelling() != components.value.spelling() => {
                self.scope.declare_go_name(name.spelling());
                let copy = define(name.clone(), GoExpression::identifier(components.value));
                (vec![copy], name, false)
            }
            _ => (Vec::new(), components.value, components.shared_payload),
        };

        let status_kind = match components.kind {
            ComponentKind::Option => PairStatusKind::Ok,
            ComponentKind::ErrorResult | ComponentKind::OtherResult => PairStatusKind::Error,
        };
        let pair = LoweredPair {
            value: PairValue::Named {
                name: value,
                nil_guard: None,
            },
            status: components.status,
            status_kind,
            initializer_call: None,
            borrowed,
        };

        BoundValue::pair(statements, pair)
    }

    pub(in crate::lowering) fn bind_comma_ok_pair(
        &mut self,
        site: CallSite<'_>,
        source: CommaOkSource,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
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
    }

    pub(in crate::lowering) fn bind_pair(
        &mut self,
        statements: Vec<Statement>,
        expression: GoExpression,
        slot: CommaOkValueSlot,
        kind: PairKind,
        status_hint: Option<&SourceName>,
    ) -> BoundValue {
        let status_kind = match kind {
            PairKind::CommaOk { .. } => PairStatusKind::Ok,
            PairKind::Result { .. } | PairKind::BareError => PairStatusKind::Error,
        };
        let placement = if matches!(slot, CommaOkValueSlot::Arm(_) | CommaOkValueSlot::Unused) {
            PairPlacement::IfInitializer
        } else {
            PairPlacement::Statement
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
        let mut statements = statements;
        let mut pair = LoweredPair {
            value,
            status,
            status_kind,
            initializer_call: None,
            borrowed: false,
        };

        match placement {
            PairPlacement::IfInitializer => pair.initializer_call = Some(expression),
            PairPlacement::Statement => statements.push(define_many(pair.binding(), expression)),
        }
        BoundValue::pair(statements, pair)
    }

    pub(in crate::lowering) fn arm_value_name(&mut self, name: &SourceName) -> GoIdentifier {
        let candidate = name.preferred();
        if self.scope.has_binding_for_go_name(&candidate)
            || self.names.is_package_block_name(&candidate)
            || self.scope.declares_type_param(&candidate)
        {
            self.fresh_local(name)
        } else {
            self.scope.new_local_identifier(candidate)
        }
    }

    pub(in crate::lowering) fn declared_arm_value_name(
        &mut self,
        name: &GoIdentifier,
    ) -> GoIdentifier {
        let candidate = name.clone();
        let name = if self.scope.is_go_name_declared(&candidate) {
            self.fresh_local(&candidate)
        } else {
            candidate
        };
        self.scope.declare_go_name(&name);
        name
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
        let taken = (placement == PairPlacement::Statement
            && self.scope.current_block_declares(&candidate))
            || self.scope.has_binding_for_go_name(&candidate)
            || self.names.is_package_block_name(&candidate);
        let name = match hint {
            Some(hint) if taken => self.fresh_local(hint),
            None if taken => self.fresh_local(default),
            _ => self.scope.new_local_identifier(candidate),
        };
        if placement == PairPlacement::Statement {
            self.scope.declare_go_name(&name);
        }
        name
    }

    fn plan_comma_ok_pair(
        &mut self,
        site: CallSite<'_>,
        source: &CommaOkSource,
    ) -> (Vec<Statement>, GoExpression) {
        match source {
            CommaOkSource::LoweredCall { .. } => self
                .plan_call_expression(site, None, ExpressionContext::value())
                .into_parts(),
            CommaOkSource::MapIndex => self.plan_map_index_pair(site),
            CommaOkSource::TypeAssert => {
                let args = site.args;
                let expression = site.expression;
                let plan = self.plan_composite_value(&args[0], ExpressionContext::value());
                let (setup, operand) = self.convert_to_recorded_slot(&args[0], plan).into_parts();
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

    pub(in crate::lowering) fn comma_ok_source(&self, site: CallSite<'_>) -> Option<CommaOkSource> {
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
                && matches!(function, Expression::DotAccess { .. })
                && args.len() == 1
                && spread.is_none() =>
            {
                Some(CommaOkSource::MapIndex)
            }
            _ => {
                let lowered = self.lowered_call_of(Vec::new(), &plan)?;
                let pair = lowered.raw_pair()?;
                if !matches!(
                    lowered.shape,
                    LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. })
                ) || pair.payload_layout_change.is_some()
                {
                    return None;
                }
                Some(CommaOkSource::LoweredCall {
                    nil_guard: pair.nil_guard,
                })
            }
        }
    }
}
