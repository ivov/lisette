use crate::Planner;
use crate::abi::callable::{LoweredReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::abi::wrapping::NilGuard;
use crate::calls::bound_value::{BoundValue, LoweredPair, PairValue};
use crate::ir::bodies::{Statement, define, define_many};
use crate::ir::local::GoIdentifier;
use crate::ir::values::GoExpression;
use crate::names::source_name::SourceName;
use crate::plan::calls::CallableOrigin;
use crate::state::bindings::{BindingValue, ComponentBinding, ComponentKind};
use crate::state::expression::ExpressionContext;
use crate::state::scope::PairStatusKind;
use syntax::ast::Expression;
use syntax::program::CallOperation;
use syntax::program::NativeTypeKind;

pub(crate) enum CommaOkSource {
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

    pub(crate) fn has_nil_guard(&self) -> bool {
        self.nil_guard().is_some()
    }

    pub(crate) fn is_map_index(&self) -> bool {
        matches!(self, Self::MapIndex)
    }
}

pub(crate) enum CommaOkValueSlot {
    Named(GoIdentifier),
    Temp,
    Arm(GoIdentifier),
    Unused,
    Discarded,
}

pub(crate) enum PairKind {
    CommaOk { nil_guard: Option<NilGuard> },
    Result { nil_guard: Option<NilGuard> },
    BareError,
}

impl Planner<'_> {
    pub(crate) fn bind_component_pair(
        &mut self,
        components: ComponentBinding,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let (statements, value, borrowed) = match slot {
            CommaOkValueSlot::Named(name) if name.spelling() != components.value.spelling() => {
                self.declare(name.spelling());
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

    pub(crate) fn component_binding(&self, expression: &Expression) -> Option<ComponentBinding> {
        let Expression::Identifier {
            value, resolution, ..
        } = expression.unwrap_parens()
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

    pub(crate) fn comma_ok_source(&self, expression: &Expression) -> Option<CommaOkSource> {
        let plan = self.plan_call(expression)?;
        let Expression::Call {
            expression: function,
            args,
            spread,
            ..
        } = expression
        else {
            return None;
        };
        match &plan.resolved.origin {
            CallableOrigin::Source(CallOperation::AssertType) => {
                let [operand] = args.as_slice() else {
                    return None;
                };
                let operand_ty = operand.get_type();
                let asserts_on_interface = self.facts.is_interface_or_unknown(&operand_ty)
                    || self.facts.peel_alias(&operand_ty).is_error();
                let asserts_function = self
                    .any_slot_function_layouts(
                        &self.facts.peel_alias(&expression.get_type()).ok_type(),
                    )
                    .is_some();
                (asserts_on_interface && !asserts_function).then_some(CommaOkSource::TypeAssert)
            }
            CallableOrigin::Source(CallOperation::Native {
                kind: NativeTypeKind::Map,
                method,
            }) if method == "get"
                && matches!(**function, Expression::DotAccess { .. })
                && args.len() == 1
                && spread.is_none() =>
            {
                Some(CommaOkSource::MapIndex)
            }
            _ => {
                let lowered = self.lowered_call_of(expression, Vec::new(), &plan)?;
                if !matches!(
                    lowered.shape,
                    LoweredReturnAbi::Option(OptionReturnAbi::CommaOk {
                        payload: PayloadLayout::Packed,
                    })
                ) || lowered.ok_ty.is_unit()
                    || lowered.is_bridged()
                {
                    return None;
                }
                Some(CommaOkSource::LoweredCall {
                    nil_guard: lowered.nil_guard,
                })
            }
        }
    }

    pub(crate) fn bind_comma_ok_pair(
        &mut self,
        expression: &Expression,
        source: CommaOkSource,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let (statements, pair) = self.plan_comma_ok_pair(expression, &source);
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

    pub(crate) fn bind_pair(
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
        let opens_if = matches!(slot, CommaOkValueSlot::Arm(_) | CommaOkValueSlot::Unused);
        let value = match kind {
            PairKind::BareError => PairValue::Absent,
            PairKind::CommaOk { nil_guard } | PairKind::Result { nil_guard } => {
                let name = match slot {
                    CommaOkValueSlot::Named(name) => Some(name),
                    CommaOkValueSlot::Arm(name) => Some(name),
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
        let mut status = self.pair_status(status_hint, status_kind, opens_if);
        if opens_if && value.name().map(GoIdentifier::spelling) == Some(status.spelling()) {
            self.scope.reserve_go_name(&status);
            status = self.fresh_like(&status);
        }
        let mut statements = statements;
        let mut pair = LoweredPair {
            value,
            status,
            status_kind,
            initializer_call: None,
            borrowed: false,
        };
        if opens_if {
            pair.initializer_call = Some(expression);
        } else {
            statements.push(define_many(pair.binding(), expression));
        }
        BoundValue::pair(statements, pair)
    }

    pub(crate) fn arm_value_name(&mut self, name: &SourceName) -> GoIdentifier {
        let candidate = name.preferred();
        if self.scope.has_binding_for_go_name(&candidate)
            || self.package.is_package_block_name(&candidate)
        {
            self.fresh_source(name)
        } else {
            self.scope.new_local_identifier(candidate)
        }
    }

    pub(crate) fn declared_arm_value_name(&mut self, name: &GoIdentifier) -> GoIdentifier {
        let candidate = name.clone();
        let name = if self.is_declared(&candidate) {
            self.fresh_like(&candidate)
        } else {
            candidate
        };
        self.declare(&name);
        name
    }

    pub(crate) fn pair_status(
        &mut self,
        hint: Option<&SourceName>,
        kind: PairStatusKind,
        opens_if: bool,
    ) -> GoIdentifier {
        let default = match kind {
            PairStatusKind::Error => "err",
            PairStatusKind::Ok => "ok",
        };
        let candidate = hint.map_or_else(|| default.to_string(), SourceName::preferred);
        let taken = (!opens_if && self.scope.current_block_declares(&candidate))
            || self.scope.has_binding_for_go_name(&candidate)
            || self.package.is_package_block_name(&candidate);
        let name = match hint {
            Some(hint) if taken => self.fresh_source(hint),
            None if taken => self.fresh_local(Some(default)),
            _ => self.scope.new_local_identifier(candidate),
        };
        if !opens_if {
            self.declare(&name);
        }
        name
    }

    fn plan_comma_ok_pair(
        &mut self,
        expression: &Expression,
        source: &CommaOkSource,
    ) -> (Vec<Statement>, GoExpression) {
        match source {
            CommaOkSource::LoweredCall { .. } => self
                .plan_call_expression(expression, None, ExpressionContext::value())
                .into_parts(),
            CommaOkSource::MapIndex => self.plan_map_index_pair(expression),
            CommaOkSource::TypeAssert => {
                let Expression::Call { args, .. } = expression else {
                    unreachable!("comma_ok_source only accepts Call expressions");
                };
                let (setup, operand) = self
                    .plan_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                let target_ty = self.facts.peel_alias(&expression.get_type()).ok_type();
                let target = self.go_type(&target_ty);
                (setup, GoExpression::type_assertion(operand, target))
            }
        }
    }
}
