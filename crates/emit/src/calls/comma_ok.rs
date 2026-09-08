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
    Arm(String),
    /// No payload use. The value is still captured when the nil guard needs it.
    Unused,
    /// No payload use, bound as a statement so the status can serve as a value.
    Discarded,
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
    has_value_slot: bool,
    initializer_call: Option<GoExpression>,
}

pub(crate) struct PairCondition {
    pub(crate) initializer: Option<Definition>,
    pub(crate) condition: GoExpression,
}

impl LoweredPair {
    pub(crate) fn status(&self) -> &str {
        &self.status
    }

    pub(crate) fn discard_value(&mut self) {
        if self.nil_guard.is_none() {
            self.value = None;
        }
    }

    fn binding(&self) -> Vec<String> {
        match (self.has_value_slot, &self.value) {
            (true, Some(value)) => vec![value.clone(), self.status.clone()],
            (true, None) => vec!["_".to_string(), self.status.clone()],
            (false, _) => vec![self.status.clone()],
        }
    }

    fn initializer(&self) -> Option<Definition> {
        let call = self.initializer_call.as_ref()?;
        Some(Definition {
            names: self.binding(),
            value: header_call(call.clone()),
        })
    }
}

/// Go reads a bare `T{` in an `if` header as the block, so such a receiver takes parentheses.
pub(crate) fn header_call(call: GoExpression) -> GoExpression {
    let text = call.as_str();
    let mut rest = text.trim_start_matches(['&', '*']);
    let type_name_end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
        .unwrap_or(rest.len());
    if type_name_end == 0 {
        return call;
    }
    rest = &rest[type_name_end..];
    if let Some(after_bracket) = rest.strip_prefix('[') {
        let mut depth = 1usize;
        let close = after_bracket.char_indices().find(|(_, character)| {
            match character {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
            depth == 0
        });
        let Some((close, _)) = close else {
            return call;
        };
        rest = &after_bracket[close + 1..];
    }
    if rest.starts_with('{') {
        GoExpression::parenthesized(call)
    } else {
        call
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
        let (carries_value, nil_guard, success, status_kind) = match kind {
            PairKind::CommaOk { nil_guard } => {
                (true, nil_guard, PairSuccess::Truthy, PairStatusKind::Ok)
            }
            PairKind::Error {
                carries_value,
                nil_guard,
            } => (
                carries_value,
                nil_guard,
                PairSuccess::Nil,
                PairStatusKind::Error,
            ),
        };
        let opens_if = matches!(slot, CommaOkValueSlot::Arm(_) | CommaOkValueSlot::Unused);
        let value = carries_value
            .then(|| match slot {
                CommaOkValueSlot::Named(name) | CommaOkValueSlot::Arm(name) => Some(name),
                CommaOkValueSlot::Temp => Some(self.fresh_pair_value()),
                CommaOkValueSlot::Unused | CommaOkValueSlot::Discarded => {
                    nil_guard.map(|_| self.fresh_pair_value())
                }
            })
            .flatten();
        let mut status = self.pair_status(status_hint, status_kind, opens_if);
        if opens_if && value.as_deref() == Some(status.as_str()) {
            status = self.fresh_var(Some(&status));
        }
        let mut pair = LoweredPair {
            statements,
            value,
            status,
            success,
            nil_guard,
            has_value_slot: carries_value,
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
        let status = match (pair.success, success) {
            (PairSuccess::Truthy, true) => status,
            (PairSuccess::Truthy, false) => GoExpression::unary("!", status),
            (PairSuccess::Nil, true) => is_nil(status),
            (PairSuccess::Nil, false) => non_nil(status),
        };
        let condition = match pair.nil_guard {
            None => status,
            Some(guard) => {
                let value = GoExpression::name(
                    pair.value
                        .clone()
                        .expect("nil guard requires the value var"),
                );
                let nil_condition = if success {
                    guard.non_nil(value)
                } else {
                    guard.is_nil(value)
                };
                let operator = if success { "&&" } else { "||" };
                GoExpression::binary(status, operator, nil_condition)
            }
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
                let operand = parenthesize_prefixed_expression(operand);
                let target_ty = self.facts.peel_alias(&expression.get_type()).ok_type();
                let target = self.use_go_type(&target_ty);
                (setup, GoExpression::type_assertion(operand, target))
            }
        }
    }
}

/// `*x` and `&x` bind looser than a postfix `[k]` or `.(T)`.
pub(super) fn parenthesize_prefixed_expression(operand: GoExpression) -> GoExpression {
    if operand.as_str().starts_with('*') || operand.as_str().starts_with('&') {
        GoExpression::parenthesized(operand)
    } else {
        operand
    }
}
