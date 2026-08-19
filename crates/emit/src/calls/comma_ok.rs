use crate::Planner;
use crate::abi::callable::{CallableReturnAbi, OptionReturnAbi, PayloadLayout};
use crate::calls::dispatch::extract_native_method_name;
use crate::calls::go_interop::NilGuard;
use crate::context::expression::ExpressionContext;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::CallableOrigin;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;
use syntax::types::Type;

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
    /// No payload use. The value is still captured when the nil guard needs it.
    Unused,
}

#[derive(Clone, Copy)]
enum PairSuccess {
    Truthy,
    Nil,
}

pub(crate) enum PairKind {
    CommaOk {
        nil_guard: Option<NilGuard>,
    },
    Error {
        carries_value: bool,
        nil_guard: Option<NilGuard>,
    },
}

/// A bound two-result expression and the rule that distinguishes success.
pub(crate) struct LoweredPair {
    pub(crate) statements: Vec<LoweredStatement>,
    pub(crate) value: Option<String>,
    status: String,
    success: PairSuccess,
    nil_guard: Option<NilGuard>,
}

impl LoweredPair {
    pub(crate) fn status(&self) -> &str {
        &self.status
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
        let expression_ty = expression.get_type();
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
                if !matches!(
                    plan.resolved.abi.result,
                    CallableReturnAbi::Option(OptionReturnAbi::CommaOk {
                        payload: PayloadLayout::Packed,
                    })
                ) {
                    return None;
                }
                let ok_ty = self.facts.peel_alias(&expression_ty).ok_type();
                if ok_ty.is_unit() || matches!(self.facts.peel_alias(&ok_ty), Type::Tuple(_)) {
                    return None;
                }
                if self
                    .go_return_payload_bridge(&plan.resolved.abi, &expression_ty)
                    .is_some()
                {
                    return None;
                }
                let nil_guard = if self.is_interface_option(&expression_ty) {
                    Some(NilGuard::Interface)
                } else if self.facts.is_nullable_option(&expression_ty) {
                    Some(NilGuard::Pointer)
                } else {
                    None
                };
                Some(CommaOkSource {
                    pair: CommaOkPair::LoweredCall,
                    nil_guard,
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
        )
    }

    pub(crate) fn bind_pair(
        &mut self,
        mut statements: Vec<LoweredStatement>,
        expression: String,
        slot: CommaOkValueSlot,
        kind: PairKind,
    ) -> LoweredPair {
        let (carries_value, nil_guard, success, hint) = match kind {
            PairKind::CommaOk { nil_guard } => (true, nil_guard, PairSuccess::Truthy, "ok"),
            PairKind::Error {
                carries_value,
                nil_guard,
            } => (carries_value, nil_guard, PairSuccess::Nil, "err"),
        };
        let value = carries_value
            .then(|| match slot {
                CommaOkValueSlot::Named(name) => Some(name),
                CommaOkValueSlot::Temp => Some(self.fresh_pair_value()),
                CommaOkValueSlot::Unused => nil_guard.map(|_| self.fresh_pair_value()),
            })
            .flatten();
        let status = self.fresh_var(Some(hint));
        self.declare(&status);
        let binding = match (carries_value, value.as_deref()) {
            (true, Some(value)) => format!("{value}, {status}"),
            (true, None) => format!("_, {status}"),
            (false, _) => status.clone(),
        };
        statements.push(LoweredStatement::RawGo(format!(
            "{binding} := {expression}\n"
        )));
        LoweredPair {
            statements,
            value,
            status,
            success,
            nil_guard,
        }
    }

    fn fresh_pair_value(&mut self) -> String {
        let v = self.fresh_var(Some("ret"));
        self.declare(&v);
        v
    }

    pub(crate) fn pair_success_condition(&mut self, pair: &LoweredPair) -> String {
        self.pair_condition(pair, true)
    }

    pub(crate) fn pair_failure_condition(&mut self, pair: &LoweredPair) -> String {
        self.pair_condition(pair, false)
    }

    fn pair_condition(&mut self, pair: &LoweredPair, success: bool) -> String {
        let status = match (pair.success, success) {
            (PairSuccess::Truthy, true) => pair.status.clone(),
            (PairSuccess::Truthy, false) => format!("!{}", pair.status),
            (PairSuccess::Nil, true) => format!("{} == nil", pair.status),
            (PairSuccess::Nil, false) => format!("{} != nil", pair.status),
        };
        let Some(guard) = pair.nil_guard else {
            return status;
        };
        if guard.is_interface() {
            self.require_stdlib();
        }
        let value = pair
            .value
            .as_deref()
            .expect("nil guard requires the value var");
        let nil_condition = if success {
            guard.non_nil(value)
        } else {
            guard.is_nil(value)
        };
        let operator = if success { "&&" } else { "||" };
        format!("{status} {operator} {nil_condition}")
    }

    /// Lower the pair-producing Go expression.
    fn lower_comma_ok_pair(
        &mut self,
        expression: &Expression,
        pair: &CommaOkPair,
    ) -> (Vec<LoweredStatement>, String) {
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
                let operand = parenthesize_prefixed(operand);
                let target_ty = self.facts.peel_alias(&expression.get_type()).ok_type();
                let target = self.use_go_type(&target_ty);
                (setup, format!("{}.({})", operand, target))
            }
        }
    }
}

/// `*x` and `&x` bind looser than a postfix `[k]` or `.(T)`.
pub(super) fn parenthesize_prefixed(operand: String) -> String {
    if operand.starts_with('*') || operand.starts_with('&') {
        format!("({operand})")
    } else {
        operand
    }
}
