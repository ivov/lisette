use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::calls::bound_value::{BoundValue, LoweredPair, PairValue};
use crate::calls::dispatch::extract_native_method_name;
use crate::calls::go_interop::NilGuard;
use crate::context::expression::ExpressionContext;
use crate::escape_reserved;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{Statement, define, define_many};
use crate::plan::calls::CallableOrigin;
use crate::plan::values::GoExpression;
use crate::state::bindings::{
    BindingValue, ComponentBinding, ComponentKind, WholeValueConstructor,
};
use crate::state::scope::PairStatusKind;
use syntax::ast::Expression;
use syntax::program::CallKind;
use syntax::program::NativeTypeKind;

/// How an Option-typed scrutinee produces its Go comma-ok pair.
enum CommaOkPair {
    /// Call whose lowered ABI is already `(T, bool)`.
    LoweredCall,
    /// `m.get(k)` lowered as `m[k]`.
    MapIndex,
    /// `assert_type<T>(x)` lowered as `x.(T)`.
    TypeAssert,
}

pub(crate) struct CommaOkSource {
    pair: CommaOkPair,
    /// Nil test the tagged wrap would apply on top of `ok`.
    nil_guard: Option<NilGuard>,
}

impl CommaOkSource {
    pub(crate) fn has_nil_guard(&self) -> bool {
        self.nil_guard.is_some()
    }

    pub(crate) fn is_map_index(&self) -> bool {
        matches!(self.pair, CommaOkPair::MapIndex)
    }
}

/// What the caller needs from the pair's value slot.
pub(crate) enum CommaOkValueSlot {
    /// Bind this Go name (already freshened and declared).
    Named(String),
    /// Allocate a fresh temporary.
    Temp,
    Arm(String),
    /// No payload use. The value is still captured when the nil guard needs it.
    Unused,
    /// No payload use, bound as a statement so the status can serve as a value.
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
            CommaOkValueSlot::Named(name) if name != components.value.spelling() => {
                self.declare(&name);
                let copy = define(name.clone(), GoExpression::identifier(components.value));
                (vec![copy], name, false)
            }
            _ => (
                Vec::new(),
                components.value.to_string(),
                components.shared_payload,
            ),
        };
        let status_kind = match components.kind {
            ComponentKind::Option => PairStatusKind::Ok,
            ComponentKind::Result => PairStatusKind::Error,
        };
        let pair = LoweredPair {
            value: PairValue::Named {
                name: value,
                nil_guard: None,
            },
            status: components.status.to_string(),
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

    pub(crate) fn rebuild_from_components(&self, components: &ComponentBinding) -> GoExpression {
        let constructor = match components
            .whole_value_constructor
            .expect("component binding has no whole-value constructor")
        {
            WholeValueConstructor::OptionFromCommaOk => "OptionFromCommaOk",
            WholeValueConstructor::ResultFromPair => "ResultFromPair",
        };
        GoExpression::call(
            GoExpression::generated(
                GeneratedPackage::Prelude,
                format!("{constructor}[{}]", components.payload_go_type),
            ),
            vec![
                GoExpression::identifier(components.value.clone()),
                GoExpression::identifier(components.status.clone()),
            ],
        )
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
            CallableOrigin::Source(CallKind::AssertType) => {
                let [operand] = args.as_slice() else {
                    return None;
                };
                let operand_ty = operand.get_type();
                let asserts_on_interface = self.facts.is_interface_or_unknown(&operand_ty)
                    || self.facts.peel_alias(&operand_ty).is_error();
                asserts_on_interface.then_some(CommaOkSource {
                    pair: CommaOkPair::TypeAssert,
                    nil_guard: None,
                })
            }
            CallableOrigin::Source(CallKind::NativeMethod(kind))
                if matches!(kind, NativeTypeKind::Map)
                    && extract_native_method_name(function) == "get"
                    && args.len() == 1
                    && spread.is_none() =>
            {
                Some(CommaOkSource {
                    pair: CommaOkPair::MapIndex,
                    nil_guard: None,
                })
            }
            _ => {
                let lowered = self.lowered_call_of(expression, Vec::new(), &plan)?;
                if !matches!(
                    lowered.shape,
                    CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                        payload: PayloadLayout::Packed,
                    })
                ) || lowered.ok_ty.is_unit()
                    || lowered.is_bridged()
                {
                    return None;
                }
                Some(CommaOkSource {
                    pair: CommaOkPair::LoweredCall,
                    nil_guard: lowered.nil_guard,
                })
            }
        }
    }

    /// Bind a recognized pair to its value and ok variables.
    pub(crate) fn bind_comma_ok_pair(
        &mut self,
        expression: &Expression,
        source: CommaOkSource,
        slot: CommaOkValueSlot,
    ) -> BoundValue {
        let (statements, pair) = self.lower_comma_ok_pair(expression, &source.pair);
        self.bind_pair(
            statements,
            pair,
            slot,
            PairKind::CommaOk {
                nil_guard: source.nil_guard,
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
        status_hint: Option<&str>,
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
                    CommaOkValueSlot::Named(name) | CommaOkValueSlot::Arm(name) => Some(name),
                    CommaOkValueSlot::Temp => Some(self.fresh_pair_value()),
                    CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => {
                        nil_guard.map(|_| self.fresh_pair_value())
                    }
                };
                match name {
                    Some(name) => PairValue::Named { name, nil_guard },
                    None => PairValue::Discarded,
                }
            }
        };
        let mut status = self.pair_status(status_hint, status_kind, opens_if);
        if opens_if && value.name() == Some(status.as_str()) {
            self.scope.reserve_go_name(&status);
            status = self.fresh_var(Some(&status));
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

    pub(crate) fn fresh_pair_value(&mut self) -> String {
        let v = self.fresh_var(Some("ret"));
        self.declare(&v);
        v
    }

    pub(crate) fn arm_value_name(&mut self, name: &str) -> String {
        let candidate = escape_reserved(name);
        if self.scope.has_binding_for_go_name(&candidate)
            || self.package.is_package_block_name(&candidate)
        {
            self.fresh_var(Some(&candidate))
        } else {
            candidate.into_owned()
        }
    }

    pub(crate) fn declared_arm_value_name(&mut self, name: &str) -> String {
        let candidate = self.arm_value_name(name);
        let name = if self.is_declared(&candidate) {
            self.fresh_var(Some(&candidate))
        } else {
            candidate
        };
        self.declare(&name);
        name
    }

    /// Fresh within a Go block; nested blocks may shadow outer statuses.
    pub(crate) fn pair_status(
        &mut self,
        hint: Option<&str>,
        kind: PairStatusKind,
        opens_if: bool,
    ) -> String {
        let candidate = escape_reserved(hint.unwrap_or(match kind {
            PairStatusKind::Error => "err",
            PairStatusKind::Ok => "ok",
        }));
        let taken = (!opens_if && self.scope.current_block_declares(&candidate))
            || self.scope.has_binding_for_go_name(&candidate)
            || self.package.is_package_block_name(&candidate);
        let name = if taken {
            self.fresh_var(Some(&candidate))
        } else {
            candidate.into_owned()
        };
        if !opens_if {
            self.declare(&name);
        }
        name
    }

    /// Lower the pair-producing Go expression.
    fn lower_comma_ok_pair(
        &mut self,
        expression: &Expression,
        pair: &CommaOkPair,
    ) -> (Vec<Statement>, GoExpression) {
        match pair {
            CommaOkPair::LoweredCall => self
                .lower_call(expression, None, ExpressionContext::value())
                .into_parts(),
            CommaOkPair::MapIndex => self.lower_map_index_pair(expression),
            CommaOkPair::TypeAssert => {
                let Expression::Call { args, .. } = expression else {
                    unreachable!("comma_ok_source only accepts Call expressions");
                };
                let (setup, operand) = self
                    .lower_composite_value(&args[0], ExpressionContext::value())
                    .into_parts();
                let target_ty = self.facts.peel_alias(&expression.get_type()).ok_type();
                let target = self.use_go_type(&target_ty);
                (setup, GoExpression::type_assertion(operand, target))
            }
        }
    }
}
