use crate::Planner;
use crate::abi::callable::{
    CallableAbi, CallableReturnAbi, LoweredReturnAbi, OptionReturnAbi, PayloadLayout,
};
use crate::abi::coercion::{LayoutBridge, resolve_layout_bridge};
use crate::abi::layout::{FunctionLayout, SlotOrigin, ValueLayout};
use crate::abi::transition::{multi_value_return, reencode_return};
use crate::control_flow::fallible::{
    Fallible, PARTIAL_BOTH_CTOR, PARTIAL_ERR_CTOR, PARTIAL_OK_CTOR, prelude_call,
};
use crate::control_flow::propagation::plain_return;
use crate::is_order_sensitive;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::names::packages::PackageUse;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoweredBlock, LoweredStatement, Statement, assign, define, define_many,
    expression_statement,
};
use crate::plan::go_expression::{
    BinaryOp, FunctionLiteralLayout, GoExpressionNode, GoParameter, UnaryOp,
};
use crate::plan::local::GoIdentifier;
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use syntax::ast::Expression;
use syntax::parse::TUPLE_FIELDS;
use syntax::program::{DefinitionBody, resolved_definition};
use syntax::types::{FunctionParameter, Type};

pub(crate) fn is_nil(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, BinaryOp::Eq, GoExpression::nil())
}

pub(crate) fn non_nil(value: GoExpression) -> GoExpression {
    GoExpression::binary(value, BinaryOp::Ne, GoExpression::nil())
}

pub(crate) fn is_nil_interface(value: GoExpression) -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Prelude, "IsNilInterface"),
        vec![value],
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum NilGuard {
    /// Pointer ok-type: `ptr == nil`.
    Pointer,
    /// Non-error interface ok-type: `lisette.IsNilInterface(v)`.
    Interface,
    /// Sentinel-valued Go result, such as `strings.Index`: `v == -1`.
    Sentinel(i64),
}

impl NilGuard {
    pub(crate) fn is_nil(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => is_nil(value),
            NilGuard::Interface => is_nil_interface(value),
            NilGuard::Sentinel(sentinel) => GoExpression::binary(
                value,
                BinaryOp::Eq,
                GoExpression::literal(sentinel.to_string()),
            ),
        }
    }

    pub(crate) fn non_nil(self, value: GoExpression) -> GoExpression {
        match self {
            NilGuard::Pointer => non_nil(value),
            NilGuard::Interface => GoExpression::unary(UnaryOp::Not, is_nil_interface(value)),
            NilGuard::Sentinel(sentinel) => GoExpression::binary(
                value,
                BinaryOp::Ne,
                GoExpression::literal(sentinel.to_string()),
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum WrapperTarget<'a> {
    /// Allocate a fresh `var slot T` and write `slot = X` per branch.
    FreshSlot,
    /// Write `slot = X` per branch into the caller-provided slot name.
    Slot(&'a GoIdentifier),
    /// Emit `return X` per branch; caller skips its trailing return.
    Return,
}

/// `Some(slot_name)` when the wrapper wrote into a fresh or named slot; `None`
/// when it wrote a `return` statement and the caller should not emit its own.
pub(crate) type WrapperOutcome = Option<GoIdentifier>;

pub(super) enum ResolvedSink {
    Slot(GoIdentifier),
    Return,
}

/// `slot = value` or `return value`.
fn leaf_statement(sink: &ResolvedSink, value: GoExpression) -> Statement {
    match sink {
        ResolvedSink::Slot(name) => assign(GoExpression::identifier(name.clone()), value),
        ResolvedSink::Return => plain_return(value),
    }
}

/// A single-statement branch body for a wrapper-dispatch `If`.
pub(super) fn leaf_block(sink: &ResolvedSink, value: GoExpression) -> LoweredBlock {
    LoweredBlock {
        statements: vec![leaf_statement(sink, value)],
    }
}

fn packed_tuple_elements(slots: &[ValueLayout]) -> &[ValueLayout] {
    slots[0]
        .tuple_elements()
        .expect("a payload bridged against a flattened one is a tuple")
}

/// Never nil in Go.
fn names_package_function(node: &GoExpressionNode) -> bool {
    match node {
        GoExpressionNode::Identifier(name) => name.is_external(),
        GoExpressionNode::Qualified { package, .. } => {
            *package == PackageUse::generated(GeneratedPackage::Prelude)
        }
        _ => false,
    }
}

fn names(names: &[GoIdentifier]) -> Vec<GoExpression> {
    names
        .iter()
        .map(|name| GoExpression::identifier(name.clone()))
        .collect()
}

impl Planner<'_> {
    pub(crate) fn plan_function_layout_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        value: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> GoExpression {
        let names_package_function = match value.node() {
            GoExpressionNode::Instantiation { base, .. } => names_package_function(base),
            node => names_package_function(node),
        };
        let never_nil = names_package_function
            || matches!(value.node(), GoExpressionNode::FunctionLiteral { .. });
        let callee = if names_package_function {
            value
        } else {
            GoExpression::identifier(self.hoist_tmp_value_statement(statements, "cb", value))
        };
        let mut body = Vec::new();
        let mut parameters = Vec::new();
        let mut names = Vec::new();
        for (index, target) in target.parameters.iter().enumerate() {
            let name = self
                .scope
                .helper_local(&format!("arg{index}"), &self.package);
            let target_type = target.go_type(self);
            let target_type = self.use_rendered_go_type(target_type);
            parameters.push(GoParameter::new(name.clone(), target_type));
            names.push(GoExpression::identifier(name));
        }
        let arguments = self.plan_bridged_arguments(&mut body, names, source, target);
        let call = GoExpression::call(callee.clone(), arguments);
        self.plan_function_result_bridge(&mut body, call, source, target);
        let result = target
            .result_go_type(self)
            .map(|result| self.use_rendered_go_type(result))
            .unwrap_or_default();
        let callback = GoExpression::function_literal(
            parameters,
            result,
            LoweredBlock { statements: body },
            FunctionLiteralLayout::MultiLine,
        );
        if never_nil {
            return callback;
        }
        let adapted = self.fresh_local(Some("adapted"));
        self.declare(&adapted);
        let go_type = target.go_type(self);
        let go_type = self.use_rendered_go_type(go_type);
        statements.push(
            LoweredStatement::VarDecl {
                name: adapted.clone(),
                go_type,
                value: None,
            }
            .into(),
        );
        statements.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil(callee),
                LoweredBlock {
                    statements: vec![assign(GoExpression::identifier(adapted.clone()), callback)],
                },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(adapted)
    }

    /// Bridge each `target` parameter value into the matching `source` slot.
    pub(crate) fn plan_bridged_arguments(
        &mut self,
        statements: &mut Vec<Statement>,
        parameters: Vec<GoExpression>,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) -> Vec<GoExpression> {
        parameters
            .into_iter()
            .zip(source.parameters.iter().zip(&target.parameters))
            .map(|(parameter, (source, target))| {
                let bridge = resolve_layout_bridge(self, target, source);
                let argument = self.plan_layout_bridge(statements, parameter, &bridge);
                if source.logical_type().get_name() == Some("VarArgs") {
                    GoExpression::spread(argument)
                } else {
                    argument
                }
            })
            .collect()
    }

    pub(crate) fn plan_function_result_bridge(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        source: &FunctionLayout,
        target: &FunctionLayout,
    ) {
        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Tuple { .. })
        ) && target.return_abi == CallableReturnAbi::Value
        {
            let target_slots = target.results[0]
                .tuple_elements()
                .expect("a tagged tuple result has tuple element layouts");
            let bridges = source
                .results
                .iter()
                .zip(target_slots)
                .map(|(source, target)| resolve_layout_bridge(self, source, target))
                .collect();
            let (body, _) = self.lower_abi_wrapping(
                call,
                &source.return_abi,
                &source.result_type,
                Some(super::GoResultBridge::Tuple(bridges)),
                WrapperTarget::Return,
            );
            statements.extend(body);
            return;
        }
        if matches!(
            source.return_abi.lowered(),
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable))
        ) && target.return_abi == CallableReturnAbi::Value
        {
            let source = ValueLayout::NullableOption {
                option_type: source.result_type.clone(),
                payload: Box::new(source.results[0].clone()),
            };
            let bridge = resolve_layout_bridge(self, &source, &target.results[0]);
            let raw = self.hoist_tmp_value_statement(statements, "raw", call);
            let value = self.plan_layout_bridge(statements, GoExpression::identifier(raw), &bridge);
            statements.push(plain_return(value));
            return;
        }
        if !source.return_abi.same_logical_contract(&target.return_abi) {
            if target.return_abi.is_lowered() {
                let tagged = self.value_layout(&source.result_type, SlotOrigin::Lisette);
                let intermediate =
                    self.function_layout(Vec::new(), &tagged, target.return_abi.clone(), None);
                if !intermediate.same_result_representation(target) {
                    // Re-encoding changes the result convention; the remaining
                    // slot bridge changes physical payloads such as Option<int>
                    // to Go's *int. Keep both contracts explicit.
                    let body = reencode_return(
                        self,
                        call,
                        &source.return_abi,
                        &intermediate.return_abi,
                        &source.result_type,
                    );
                    let result = intermediate
                        .result_go_type(self)
                        .expect("a lowered result has a Go signature");
                    let result = self.use_rendered_go_type(result);
                    let call = GoExpression::call(
                        GoExpression::function_literal(
                            Vec::new(),
                            result,
                            LoweredBlock { statements: body },
                            FunctionLiteralLayout::MultiLine,
                        ),
                        Vec::new(),
                    );
                    self.plan_function_result_bridge(statements, call, &intermediate, target);
                    return;
                }
            }
            statements.extend(reencode_return(
                self,
                call,
                &source.return_abi,
                &target.return_abi,
                &source.result_type,
            ));
            return;
        }
        match source.return_abi.lowered() {
            None => {
                if source.results.is_empty() {
                    statements.push(expression_statement(call));
                    if !target.results.is_empty() {
                        statements.push(if source.result_type.is_never() {
                            LoweredStatement::UnreachablePanic.into()
                        } else {
                            plain_return(GoExpression::empty_composite("struct{}".to_string()))
                        });
                    }
                    return;
                }
                if target.results.is_empty() {
                    statements.push(expression_statement(call));
                    return;
                }
                let bridge = resolve_layout_bridge(self, &source.results[0], &target.results[0]);
                let value = self.plan_layout_bridge(statements, call, &bridge);
                statements.push(plain_return(value));
            }
            Some(LoweredReturnAbi::BareError) => statements.push(plain_return(call)),
            Some(
                LoweredReturnAbi::Result { .. }
                | LoweredReturnAbi::Partial { .. }
                | LoweredReturnAbi::Option(OptionReturnAbi::CommaOk { .. }),
            ) => {
                let source_flat = source.return_abi.has_flattened_payload();
                let mut values = self.create_temp_vars("ret", source.results.len() + 1);
                statements.push(define_many(values.clone(), call));
                let auxiliary = values.pop().expect("a lowered callable has a status slot");

                if matches!(
                    source.return_abi.lowered(),
                    Some(LoweredReturnAbi::Partial { .. })
                ) && !source_flat
                {
                    let ok_type = source.result_type.ok_type();
                    if let Some(condition) = self
                        .partial_ok_nil_check(&ok_type, GoExpression::identifier(values[0].clone()))
                    {
                        statements.push(
                            LoweredStatement::If(IfPlan::plain(
                                condition,
                                LoweredBlock {
                                    statements: vec![multi_value_return(vec![
                                        GoExpression::nil(),
                                        GoExpression::identifier(auxiliary.clone()),
                                    ])],
                                },
                                ElseArm::None,
                            ))
                            .into(),
                        );
                    }
                }

                let mut values = self.bridge_payload_slots(
                    statements,
                    names(&values),
                    &source.results,
                    &target.results,
                    target.return_abi.has_flattened_payload(),
                );
                values.push(GoExpression::identifier(auxiliary));
                statements.push(multi_value_return(values));
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Nullable)) => {
                let raw = self.hoist_tmp_value_statement(statements, "raw", call);
                let condition = self
                    .option_nil_guard(&source.result_type)
                    .is_nil(GoExpression::identifier(raw.clone()));
                statements.push(
                    LoweredStatement::If(IfPlan::plain(
                        condition,
                        LoweredBlock {
                            statements: vec![plain_return(GoExpression::nil())],
                        },
                        ElseArm::None,
                    ))
                    .into(),
                );
                let bridge = resolve_layout_bridge(self, &source.results[0], &target.results[0]);
                let value =
                    self.plan_layout_bridge(statements, GoExpression::identifier(raw), &bridge);
                statements.push(plain_return(value));
            }
            Some(LoweredReturnAbi::Option(OptionReturnAbi::Sentinel(_))) => {
                statements.push(plain_return(call))
            }
            Some(LoweredReturnAbi::Tuple { arity }) => {
                let values = self.create_temp_vars("ret", *arity);
                statements.push(define_many(values.clone(), call));
                let values = names(&values)
                    .into_iter()
                    .zip(source.results.iter().zip(&target.results))
                    .map(|(value, (source, target))| {
                        let bridge = resolve_layout_bridge(self, source, target);
                        self.plan_layout_bridge(statements, value, &bridge)
                    })
                    .collect::<Vec<_>>();
                statements.push(multi_value_return(values));
            }
        }
    }

    /// `values` holds one Go value per source slot: the packed payload, or one
    fn bridge_payload_slots(
        &mut self,
        statements: &mut Vec<Statement>,
        values: Vec<GoExpression>,
        source: &[ValueLayout],
        target: &[ValueLayout],
        target_flat: bool,
    ) -> Vec<GoExpression> {
        let source_flat = values.len() > 1;
        if !source_flat && !target_flat {
            let bridge = resolve_layout_bridge(self, &source[0], &target[0]);
            let [value] = <[GoExpression; 1]>::try_from(values)
                .unwrap_or_else(|_| unreachable!("a packed payload is one value"));
            return vec![self.plan_layout_bridge(statements, value, &bridge)];
        }
        let source_slots = if source_flat {
            source
        } else {
            packed_tuple_elements(source)
        };
        let target_slots = if target_flat {
            target
        } else {
            packed_tuple_elements(target)
        };
        let elements: Vec<GoExpression> = if source_flat {
            values
        } else {
            let packed = &values[0];
            (0..source_slots.len())
                .map(|index| {
                    GoExpression::selector(packed.clone(), TUPLE_FIELDS[index].to_string())
                })
                .collect()
        };
        let bridged: Vec<GoExpression> = elements
            .into_iter()
            .zip(source_slots.iter().zip(target_slots))
            .map(|(value, (source, target))| {
                let bridge = resolve_layout_bridge(self, source, target);
                self.plan_layout_bridge(statements, value, &bridge)
            })
            .collect();
        if target_flat {
            bridged
        } else {
            vec![self.plan_tuple_from_vars(statements, bridged)]
        }
    }

    /// Prepare a wrapper sink: declare `var slot T` (slot targets) or route
    /// writes to `return`.
    pub(super) fn push_wrapper_slot(
        &mut self,
        statements: &mut Vec<Statement>,
        target: WrapperTarget<'_>,
        type_str: &str,
        name_hint: &'static str,
    ) -> (ResolvedSink, WrapperOutcome) {
        match target {
            WrapperTarget::FreshSlot => {
                let var = self.fresh_local(Some(name_hint));
                self.declare(&var);
                statements.push(
                    LoweredStatement::VarDecl {
                        name: var.clone(),
                        go_type: type_str.to_string(),
                        value: None,
                    }
                    .into(),
                );
                (ResolvedSink::Slot(var.clone()), Some(var))
            }
            WrapperTarget::Slot(name) => {
                statements.push(
                    LoweredStatement::VarDecl {
                        name: name.clone(),
                        go_type: type_str.to_string(),
                        value: None,
                    }
                    .into(),
                );
                self.declare(name.spelling());
                let owned = name.clone();
                (ResolvedSink::Slot(owned.clone()), Some(owned))
            }
            WrapperTarget::Return => (ResolvedSink::Return, None),
        }
    }

    /// Destructure a Go multi-return into error and value temps. A `Flattened`
    /// tuple ok type (Go-imported `(T1, ..., Tn, error)`) gets N+1 temps and a
    /// rebuilt Lisette tuple; a `Packed` one (Lisette `(Tuple_n[...], error)`)
    /// gets 2 temps like any other ok type.
    fn push_go_returns(
        &mut self,
        statements: &mut Vec<Statement>,
        call: GoExpression,
        ok_ty: &Type,
        layout: PayloadLayout,
    ) -> (GoIdentifier, GoExpression) {
        if layout.is_flattened()
            && let Type::Tuple(elements) = ok_ty
        {
            let tuple_arity = elements.len();
            let temp_vars = self.create_temp_vars("ret", tuple_arity + 1);
            statements.push(define_many(temp_vars.clone(), call));
            let tuple = self.plan_tuple_from_vars(statements, names(&temp_vars[..tuple_arity]));
            (temp_vars.last().unwrap().clone(), tuple)
        } else {
            let val_var = self.fresh_local(Some("ret"));
            self.declare(&val_var);
            let err_var = self.fresh_local(Some("ret"));
            self.declare(&err_var);
            statements.push(define_many(vec![val_var.clone(), err_var.clone()], call));
            (err_var, GoExpression::identifier(val_var))
        }
    }

    /// Single-leaf write for wrappers that fold to one constructor expression.
    pub(super) fn push_simple_wrapper_value(
        &mut self,
        statements: &mut Vec<Statement>,
        target: WrapperTarget<'_>,
        name_hint: &'static str,
        value: GoExpression,
    ) -> WrapperOutcome {
        match target {
            WrapperTarget::FreshSlot => {
                Some(self.hoist_tmp_value_statement(statements, name_hint, value))
            }
            WrapperTarget::Slot(name) => {
                self.declare(name.spelling());
                statements.push(define(name.clone(), value));
                Some(name.clone())
            }
            WrapperTarget::Return => {
                statements.push(plain_return(value));
                None
            }
        }
    }
}

impl Planner<'_> {
    /// Lower a `(T, error)` Go return into a tagged `Partial`.
    pub(crate) fn lower_partial_wrapping(
        &mut self,
        call: GoExpression,
        partial_layout: &ValueLayout,
        layout: PayloadLayout,
        bridges: Option<(&LayoutBridge, &LayoutBridge)>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let (payload_bridge, error_bridge) = bridges.unzip();
        let ValueLayout::Fallible { payload, error, .. } = partial_layout.underlying() else {
            unreachable!("Partial wrapping requires a fallible layout")
        };
        let ok_ty = payload.logical_type();
        let ok_ty_str = self.use_rendered_go_type(payload.go_type(self));
        let err_ty_str = self.use_rendered_go_type(error.go_type(self));

        let mut statements = Vec::new();
        let (err_var, val_value) = self.push_go_returns(&mut statements, call, ok_ty, layout);
        let err = || GoExpression::identifier(err_var.clone());
        let val = || val_value.clone();
        let nil_check = self.partial_ok_nil_check(ok_ty, val());

        let type_params = format!("[{}, {}]", ok_ty_str, err_ty_str);
        let result_ty_str = self.use_rendered_go_type(GoType::stdlib(format!(
            "{}.Partial{type_params}",
            go_name::GO_STDLIB_PKG
        )));
        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut both_setup, both_value) = self.plan_optional_payload_bridge(val(), payload_bridge);
        let (error_setup, both_error) = self.plan_optional_payload_bridge(err(), error_bridge);
        both_setup.extend(error_setup);
        let both = prelude_call(
            PARTIAL_BOTH_CTOR,
            type_params.clone(),
            vec![both_value, both_error],
        );
        both_setup.push(leaf_statement(&sink, both));
        let both_body = LoweredBlock {
            statements: both_setup,
        };

        let (mut ok_setup, ok_value) = self.plan_optional_payload_bridge(val(), payload_bridge);
        ok_setup.push(leaf_statement(
            &sink,
            prelude_call(PARTIAL_OK_CTOR, type_params.clone(), vec![ok_value]),
        ));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let then_body = if let Some(check) = nil_check {
            let (mut error_setup, error) = self.plan_optional_payload_bridge(err(), error_bridge);
            error_setup.push(leaf_statement(
                &sink,
                prelude_call(PARTIAL_ERR_CTOR, type_params, vec![error]),
            ));
            let inner = IfPlan::plain(
                check,
                LoweredBlock {
                    statements: error_setup,
                },
                ElseArm::from_body(both_body, false),
            );
            LoweredBlock {
                statements: vec![LoweredStatement::If(inner).into()],
            }
        } else {
            both_body
        };

        let else_arm = ElseArm::from_body(ok_body, false);

        statements
            .push(LoweredStatement::If(IfPlan::plain(non_nil(err()), then_body, else_arm)).into());
        (statements, outcome)
    }

    pub(crate) fn partial_ok_nil_guard(&self, ok_ty: &Type) -> Option<NilGuard> {
        self.facts.partial_ok_is_nilable(ok_ty).then(|| {
            if self.facts.as_interface(ok_ty).is_some() {
                NilGuard::Interface
            } else {
                NilGuard::Pointer
            }
        })
    }

    pub(crate) fn partial_ok_nil_check(
        &mut self,
        ok_ty: &Type,
        value: GoExpression,
    ) -> Option<GoExpression> {
        let guard = self.partial_ok_nil_guard(ok_ty)?;
        Some(guard.is_nil(value))
    }

    pub(crate) fn result_nil_guard(&self, ok_ty: &Type) -> Option<NilGuard> {
        if ok_ty.is_ref() {
            return Some(NilGuard::Pointer);
        }
        let id = self.facts.as_interface(ok_ty)?;
        (id != go_name::PRELUDE_ERROR_ID).then_some(NilGuard::Interface)
    }

    pub(crate) fn option_nil_guard(&self, option_ty: &Type) -> NilGuard {
        if self.is_interface_option(option_ty) {
            NilGuard::Interface
        } else {
            NilGuard::Pointer
        }
    }

    pub(crate) fn nullable_option_nil_guard(&self, option_ty: &Type) -> Option<NilGuard> {
        (self.is_interface_option(option_ty) || self.facts.is_nullable_option(option_ty))
            .then(|| self.option_nil_guard(option_ty))
    }

    pub(crate) fn result_from_pair(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        layout: PayloadLayout,
        payload_bridge: Option<&LayoutBridge>,
    ) -> Result<GoExpression, GoExpression> {
        let Some(fallible) = Fallible::from_type(result_layout.underlying().logical_type()) else {
            return Err(call);
        };
        let ok_ty = fallible.ok_ty();
        let err_is_error = fallible.err_ty().is_some_and(|err_ty| {
            matches!(
                self.facts.peel_alias(err_ty),
                Type::Nominal { id, .. } if id.as_str() == go_name::PRELUDE_ERROR_ID
            )
        });
        if !err_is_error
            || payload_bridge.is_some()
            || (layout.is_flattened() && ok_ty.is_tuple())
            || self.result_nil_guard(ok_ty).is_some()
        {
            return Err(call);
        }
        let payload = result_layout
            .payload()
            .expect("Result layout has a payload");
        let ok_ty_str = self.use_rendered_go_type(payload.go_type(self));
        Ok(prelude_call(
            "ResultFromPair",
            format!("[{}]", ok_ty_str),
            vec![call],
        ))
    }

    /// Lower a `(T, error)` Go return into a tagged `Result`.
    pub(crate) fn lower_result_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        layout: PayloadLayout,
        bridges: Option<(&LayoutBridge, &LayoutBridge)>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let (payload_bridge, error_bridge) = bridges.unzip();
        let ValueLayout::Fallible {
            fallible_type,
            payload,
            error,
        } = result_layout.underlying()
        else {
            unreachable!("Result wrapping requires a fallible layout")
        };
        let fallible = Fallible::from_type(fallible_type).expect("Result type expected");

        let mut statements = Vec::new();
        let call = match self.result_from_pair(call, result_layout, layout, payload_bridge) {
            Ok(value) => {
                let outcome =
                    self.push_simple_wrapper_value(&mut statements, target, "result", value);
                return (statements, outcome);
            }
            Err(call) => call,
        };
        let ok_ty = fallible.ok_ty();
        let (err_var, ok_value) = self.push_go_returns(&mut statements, call, ok_ty, layout);
        let err = || GoExpression::identifier(err_var.clone());
        let ok = || ok_value.clone();

        let result_ty_str = self.use_rendered_go_type(result_layout.underlying().go_type(self));
        let ok_ty_str = self.use_rendered_go_type(payload.go_type(self));
        let err_ty_str = self.use_rendered_go_type(error.go_type(self));
        let type_params = format!("[{ok_ty_str}, {err_ty_str}]");

        let nil_guard = self.result_nil_guard(ok_ty);

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut ok_setup, ok_value) = self.plan_optional_payload_bridge(ok(), payload_bridge);
        let ok_wrapper = prelude_call(
            fallible.ok_constructor(),
            type_params.clone(),
            vec![ok_value],
        );
        ok_setup.push(leaf_statement(&sink, ok_wrapper));
        let ok_body = LoweredBlock {
            statements: ok_setup,
        };

        let (mut error_setup, error) = self.plan_optional_payload_bridge(err(), error_bridge);
        let err_wrapper =
            prelude_call(fallible.err_constructor(), type_params.clone(), vec![error]);
        error_setup.push(leaf_statement(&sink, err_wrapper));
        let then_body = LoweredBlock {
            statements: error_setup,
        };

        let else_arm = if let Some(nil_guard) = nil_guard {
            let nil_condition = nil_guard.is_nil(ok());
            let nil_err = prelude_call(
                fallible.err_constructor(),
                type_params,
                vec![unexpected_nil_error()],
            );
            ElseArm::ElseIf(Box::new(IfPlan::plain(
                nil_condition,
                leaf_block(&sink, nil_err),
                ElseArm::from_body(ok_body, false),
            )))
        } else {
            ElseArm::from_body(ok_body, false)
        };

        statements
            .push(LoweredStatement::If(IfPlan::plain(non_nil(err()), then_body, else_arm)).into());
        (statements, outcome)
    }

    /// Lower a bare `error` Go return into a tagged `Result<(), E>`.
    pub(crate) fn lower_bare_error_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        error_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let fallible = Fallible::from_type(result_layout.underlying().logical_type())
            .expect("Result type expected");
        debug_assert!(fallible.ok_ty().is_unit());
        self.lower_unit_result_wrapping(call, result_layout, error_bridge, target)
    }

    fn plan_optional_payload_bridge(
        &mut self,
        value: GoExpression,
        bridge: Option<&LayoutBridge>,
    ) -> (Vec<Statement>, GoExpression) {
        let mut statements = Vec::new();
        let value = match bridge {
            Some(bridge) => self.plan_layout_bridge(&mut statements, value, bridge),
            None => value,
        };
        (statements, value)
    }

    fn lower_unit_result_wrapping(
        &mut self,
        call: GoExpression,
        result_layout: &ValueLayout,
        error_bridge: Option<&LayoutBridge>,
        target: WrapperTarget<'_>,
    ) -> (Vec<Statement>, WrapperOutcome) {
        let mut statements = Vec::new();
        let err_var = self.hoist_tmp_value_statement(&mut statements, "ret", call);
        let err = || GoExpression::identifier(err_var.clone());

        let ValueLayout::Fallible {
            fallible_type,
            payload,
            error,
        } = result_layout.underlying()
        else {
            unreachable!("unit Result wrapping requires a fallible layout")
        };
        let fallible = Fallible::from_type(fallible_type).expect("Result type expected");
        let result_ty_str = self.use_rendered_go_type(result_layout.underlying().go_type(self));
        let ok_type = self.use_rendered_go_type(payload.go_type(self));
        let err_type = self.use_rendered_go_type(error.go_type(self));
        let type_params = format!("[{ok_type}, {err_type}]");

        let (sink, outcome) =
            self.push_wrapper_slot(&mut statements, target, &result_ty_str, "result");

        let (mut error_setup, error) = self.plan_optional_payload_bridge(err(), error_bridge);
        let err_wrapper =
            prelude_call(fallible.err_constructor(), type_params.clone(), vec![error]);
        error_setup.push(leaf_statement(&sink, err_wrapper));
        let then_body = LoweredBlock {
            statements: error_setup,
        };

        let ok_wrapper = {
            prelude_call(
                fallible.ok_constructor(),
                type_params,
                vec![GoExpression::empty_composite("struct{}".to_string())],
            )
        };
        let else_arm = ElseArm::from_body(leaf_block(&sink, ok_wrapper), false);

        statements
            .push(LoweredStatement::If(IfPlan::plain(non_nil(err()), then_body, else_arm)).into());
        (statements, outcome)
    }

    fn hoist_go_fn_if_needed(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
    ) -> GoExpression {
        let go_fn = self.capture_operand_into(setup, expression);

        let is_go_package_fn = matches!(
            expression.unwrap_parens(),
            Expression::DotAccess { expression, .. }
            if expression.get_type().as_import_namespace()
                .is_some_and(|m| m.starts_with(go_name::GO_IMPORT_PREFIX))
        );
        if is_go_package_fn {
            return go_fn;
        }

        if is_order_sensitive(expression) {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, "fn", go_fn))
        } else {
            go_fn
        }
    }

    pub(crate) fn build_wrapper_params(
        &mut self,
        params: &[FunctionParameter],
    ) -> (Vec<GoParameter>, Vec<GoExpression>) {
        let mut param_strs = Vec::new();
        let mut arguments = Vec::new();
        let last_index = params.len().saturating_sub(1);
        for (i, param) in params.iter().enumerate() {
            let name = self.scope.helper_local(&format!("arg{}", i), &self.package);
            let ty_str = self.use_go_type(&param.ty);
            param_strs.push(GoParameter::new(name.clone(), ty_str));
            let argument = GoExpression::identifier(name);
            if i == last_index && param.ty.get_name() == Some("VarArgs") {
                arguments.push(GoExpression::spread(argument));
            } else {
                arguments.push(argument);
            }
        }
        (param_strs, arguments)
    }

    /// Common wrapper-builder prologue: returns `(return_type, param_strs,
    /// call)` for a go-fn expression, or `None` for non-function types.
    fn wrapper_call_parts(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
    ) -> Option<(Type, Vec<GoParameter>, GoExpression)> {
        let fn_type = self
            .facts
            .resolve_to_function_type(&expression.get_type())?;
        let f = fn_type.as_function_type()?;
        let (params, return_type) = (f.params.clone(), (*f.return_type).clone());
        let go_fn = self.hoist_go_fn_if_needed(setup, expression);
        let (param_strs, arguments) = self.build_wrapper_params(&params);
        let call = GoExpression::call(go_fn, arguments);
        Some((return_type, param_strs, call))
    }

    pub(crate) fn emit_go_fn_wrapper(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        abi: &CallableAbi,
    ) -> GoExpression {
        let function = self
            .facts
            .resolve_to_function_type(&expression.get_type())
            .expect("a Go callable has a function type");
        let function = function.as_function_type().expect("resolved function type");
        let return_type = &function.return_type;
        let (param_strs, _) = self.build_wrapper_params(&function.params);
        let result = self.value_layout(return_type, SlotOrigin::Lisette);
        let target = self.function_layout(
            function
                .params
                .iter()
                .map(|p| self.value_layout(&p.ty, SlotOrigin::Lisette))
                .collect(),
            &result,
            CallableReturnAbi::Value,
            None,
        );
        let source = abi.function_layout(self);
        let is_declaration = resolved_definition(expression)
            .and_then(|id| self.facts.definition(id))
            .is_some_and(|definition| {
                matches!(definition.body, DefinitionBody::Value { .. }) && !definition.is_variable()
            });
        let value = self.capture_operand_into(setup, expression);
        let value = if is_declaration {
            value
        } else {
            GoExpression::identifier(self.hoist_tmp_value_statement(setup, "cb", value))
        };
        let mut statements = Vec::new();
        let arguments = param_strs
            .iter()
            .map(|parameter| GoExpression::identifier(parameter.name.clone()))
            .collect();
        let arguments = self.plan_bridged_arguments(&mut statements, arguments, &source, &target);
        let call = GoExpression::call(value.clone(), arguments);
        if abi.result == CallableReturnAbi::Void {
            self.plan_function_result_bridge(&mut statements, call, &source, &target);
        } else {
            let bridge = self.go_result_bridge(abi, return_type);
            let (body, outcome) = self.lower_abi_wrapping(
                call,
                &abi.result,
                return_type,
                bridge,
                WrapperTarget::Return,
            );
            debug_assert!(outcome.is_none(), "Return target emits its own returns");
            statements.extend(body);
        }
        let ret_ty_str = self.use_go_type(return_type);
        let callback = GoExpression::function_literal(
            param_strs,
            ret_ty_str,
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        );
        if is_declaration {
            return callback;
        }
        let adapted = self.fresh_local(Some("adapted"));
        self.declare(&adapted);
        let go_type = self.use_rendered_go_type(target.go_type(self));
        setup.push(
            LoweredStatement::VarDecl {
                name: adapted.clone(),
                go_type,
                value: None,
            }
            .into(),
        );
        setup.push(
            LoweredStatement::If(IfPlan::plain(
                non_nil(value),
                LoweredBlock {
                    statements: vec![assign(GoExpression::identifier(adapted.clone()), callback)],
                },
                ElseArm::None,
            ))
            .into(),
        );
        GoExpression::identifier(adapted)
    }

    /// Closure that bundles a raw `(T1, T2, error)` return into the slot's `(Tuple, error)` shape.
    pub(crate) fn emit_go_fn_lowered_tuple_adapter(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
    ) -> GoExpression {
        let (return_type, param_strs, call) = self
            .wrapper_call_parts(setup, expression)
            .expect("expected function type");

        let ok_ty = return_type.ok_type();
        let err_ty = return_type.err_type();
        let ret_ty_str = format!(
            "({}, {})",
            self.use_go_type(&ok_ty),
            self.use_go_type(&err_ty)
        );
        let arity = ok_ty.tuple_arity().expect("tuple ok type");

        let mut statements = Vec::new();
        let temp_vars = self.create_temp_vars("ret", arity + 1);
        statements.push(define_many(temp_vars.clone(), call));
        let tuple = self.plan_tuple_from_vars(&mut statements, names(&temp_vars[..arity]));
        statements.push(multi_value_return(vec![
            tuple,
            GoExpression::identifier(temp_vars[arity].clone()),
        ]));

        GoExpression::function_literal(
            param_strs,
            ret_ty_str,
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        )
    }

    pub(crate) fn emit_go_fn_sentinel_adapter(
        &mut self,
        setup: &mut Vec<Statement>,
        expression: &Expression,
        sentinel: i64,
    ) -> GoExpression {
        let (return_type, param_strs, call) = self
            .wrapper_call_parts(setup, expression)
            .expect("expected function type");

        let inner_ty_str = self.use_go_type(&return_type.ok_type());
        let ret_var = self.fresh_local(Some("ret"));
        self.declare(&ret_var);
        let ret = || GoExpression::identifier(ret_var.clone());

        let statements = vec![
            define(ret_var.clone(), call),
            multi_value_return(vec![
                ret(),
                GoExpression::binary(
                    ret(),
                    BinaryOp::Ne,
                    GoExpression::literal(sentinel.to_string()),
                ),
            ]),
        ];

        GoExpression::function_literal(
            param_strs,
            format!("({}, bool)", inner_ty_str),
            LoweredBlock { statements },
            FunctionLiteralLayout::MultiLine,
        )
    }
}

pub(crate) fn unexpected_nil_error() -> GoExpression {
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Errors, "New"),
        vec![GoExpression::literal("\"unexpected nil\"".to_string())],
    )
}
