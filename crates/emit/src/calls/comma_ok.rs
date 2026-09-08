use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::calls::dispatch::extract_native_method_name;
use crate::calls::go_interop::{NilGuard, is_nil, non_nil};
use crate::context::expression::ExpressionContext;
use crate::escape_reserved;
use crate::plan::bodies::{Definition, LoweredStatement, define_many};
use crate::plan::calls::CallableOrigin;
use crate::plan::values::GoExpression;
use crate::state::scope::PairStatusKind;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;

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

/// A bound two-result expression and the rule that distinguishes success.
pub(crate) struct LoweredPair {
    pub(crate) statements: Vec<LoweredStatement>,
    value: PairValue,
    status: String,
    status_kind: PairStatusKind,
    initializer_call: Option<GoExpression>,
}

enum PairValue {
    Absent,
    Discarded,
    Named {
        name: String,
        nil_guard: Option<NilGuard>,
    },
}

impl PairValue {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Named { name, .. } => Some(name),
            Self::Absent | Self::Discarded => None,
        }
    }
}

pub(crate) struct PairCondition {
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
}

impl LoweredPair {
    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn value(&self) -> Option<&str> {
        self.value.name()
    }

    pub(crate) fn discard_value(&mut self) {
        if matches!(
            self.value,
            PairValue::Named {
                nil_guard: None,
                ..
            }
        ) {
            self.value = PairValue::Discarded;
        }
    }

    fn binding(&self) -> Vec<String> {
        match &self.value {
            PairValue::Named { name, .. } => vec![name.clone(), self.status.clone()],
            PairValue::Discarded => vec!["_".to_string(), self.status.clone()],
            PairValue::Absent => vec![self.status.clone()],
        }
    }

    fn initializer(&self) -> Option<Definition> {
        let call = self.initializer_call.as_ref()?;
        Some(Definition {
            names: self.binding(),
            value: call.clone(),
        })
    }
}

impl Planner<'_> {
    /// Recognize a call whose Option value comes from a comma-ok pair.
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
            CallableOrigin::AssertType => {
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
            CallableOrigin::NativeMethod(kind)
                if matches!(NativeGoType::from_kind(*kind), NativeGoType::Map)
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
                    || lowered.has_tuple_payload(self)
                    || lowered.payload_bridge.is_some()
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
    ) -> LoweredPair {
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
        statements: Vec<LoweredStatement>,
        expression: GoExpression,
        slot: CommaOkValueSlot,
        kind: PairKind,
        status_hint: Option<&str>,
    ) -> LoweredPair {
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
            status = self.fresh_var(Some(&status));
        }
        let mut pair = LoweredPair {
            statements,
            value,
            status,
            status_kind,
            initializer_call: None,
        };
        if opens_if {
            pair.initializer_call = Some(expression);
        } else {
            pair.statements
                .push(define_many(pair.binding(), expression));
        }
        pair
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

    pub(crate) fn pair_success_condition(&mut self, pair: &LoweredPair) -> PairCondition {
        self.pair_condition(pair, true)
    }

    pub(crate) fn pair_failure_condition(&mut self, pair: &LoweredPair) -> PairCondition {
        self.pair_condition(pair, false)
    }

    fn pair_condition(&mut self, pair: &LoweredPair, success: bool) -> PairCondition {
        let status = GoExpression::name(pair.status.clone());
        let status = match (pair.status_kind, success) {
            (PairStatusKind::Ok, true) => status,
            (PairStatusKind::Ok, false) => GoExpression::unary("!", status),
            (PairStatusKind::Error, true) => is_nil(status),
            (PairStatusKind::Error, false) => non_nil(status),
        };
        let condition = match &pair.value {
            PairValue::Named {
                name,
                nil_guard: Some(guard),
            } => {
                let value = GoExpression::name(name.clone());
                let nil_condition = if success {
                    guard.non_nil(value)
                } else {
                    guard.is_nil(value)
                };
                let operator = if success { "&&" } else { "||" };
                GoExpression::binary(status, operator, nil_condition)
            }
            _ => status,
        };
        PairCondition {
            initializer: pair.initializer(),
            condition,
        }
    }

    /// Lower the pair-producing Go expression.
    fn lower_comma_ok_pair(
        &mut self,
        expression: &Expression,
        pair: &CommaOkPair,
    ) -> (Vec<LoweredStatement>, GoExpression) {
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
