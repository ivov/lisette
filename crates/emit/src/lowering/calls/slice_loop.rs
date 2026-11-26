use crate::lowering::planner::queries::PlannerQueries;
use crate::target::ir::local::GoIdentifier;
use crate::target::names::source_name::SourceName;
use syntax::EcoString;
use syntax::ast::{Binding, BindingId, Expression, Pattern};
use syntax::program::NativeTypeKind;
use syntax::program::ReceiverCoercion;
use syntax::types::Type;

use super::NativeCallContext;
use super::native_methods::NativeCallResult;
use crate::Planner;
use crate::lowering::abi::is_closure_literal;
use crate::lowering::control_flow::branches::is_conjunction;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::lowering::value_plans::value_plan::PlacePlan;
use crate::lowering::value_plans::value_plan::ValuePlan;
use crate::target::ir::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    Statement, assign, declare, define, directed_first,
};
use crate::target::ir::go_expression::GoUnaryOperator;
use crate::target::ir::prelude::{option_variant, prelude_call};
use crate::target::ir::values::{CaptureBoundary, GoExpression, OperandForm, Stability};
use crate::target::types::go_type::GoType;
use syntax::program::SlotOrigin;

enum SliceLoop<Init> {
    Map { element: Type },
    Filter { element: Type },
    Fold(Init),
    Find { payload: Type },
    All,
}

impl<Init> SliceLoop<Init> {
    fn split_fold<T>(self) -> Result<SliceLoop<T>, Init> {
        match self {
            Self::Map { element } => Ok(SliceLoop::Map { element }),
            Self::Filter { element } => Ok(SliceLoop::Filter { element }),
            Self::Fold(init) => Err(init),
            Self::Find { payload } => Ok(SliceLoop::Find { payload }),
            Self::All => Ok(SliceLoop::All),
        }
    }
}

fn body_moves_into_a_loop(body: &Expression) -> bool {
    if matches!(
        body,
        Expression::Return { .. }
            | Expression::Propagate { .. }
            | Expression::Break { .. }
            | Expression::Continue { .. }
            | Expression::TryBlock { .. }
            | Expression::RecoverBlock { .. }
            | Expression::Defer { .. }
    ) {
        return false;
    }
    matches!(body, Expression::Lambda { .. })
        || body
            .children()
            .iter()
            .all(|child| body_moves_into_a_loop(child))
}

fn body_captures_by_reference(body: &Expression) -> bool {
    matches!(
        body,
        Expression::Lambda { .. } | Expression::Task { .. } | Expression::Reference { .. }
    ) || body
        .children()
        .iter()
        .any(|child| body_captures_by_reference(child))
}

#[derive(Clone, Copy)]
struct LoopParam<'a> {
    identifier: &'a EcoString,
    binding: &'a Option<BindingId>,
}

impl<'a> LoopParam<'a> {
    fn of(param: &'a Binding) -> Option<Self> {
        let Pattern::Identifier {
            identifier,
            binding,
            ..
        } = &param.pattern
        else {
            return None;
        };
        Some(Self {
            identifier,
            binding,
        })
    }

    fn source_name(self) -> SourceName {
        SourceName::new(self.identifier)
    }
}

struct LambdaCallback<'a> {
    body: &'a Expression,
    accumulator: Option<LoopParam<'a>>,
    element: LoopParam<'a>,
}

enum LoopCallback<'a, V> {
    Lambda(LambdaCallback<'a>),
    Value(V),
}

#[derive(Clone, Copy)]
enum LoweredCallback<'a> {
    Body(&'a Expression),
    Callee(&'a GoExpression),
}

#[derive(Clone, Copy)]
struct SliceLoopBody<'a> {
    planned: &'a PlannedLoop<'a>,
    callback: LoweredCallback<'a>,
    result_ty: &'a Type,
    element_name: &'a GoIdentifier,
}

#[derive(Clone, Copy)]
pub(in crate::lowering) struct FoundSink<'a> {
    pub value: Option<&'a GoIdentifier>,
    pub flag: &'a GoIdentifier,
}

#[derive(Clone, Copy)]
pub(super) enum SliceLoopTarget<'a> {
    Fresh,
    Into(&'a GoIdentifier),
    Found(FoundSink<'a>),
}

enum PlannedLoop<'a> {
    Map {
        result: GoIdentifier,
        index: GoIdentifier,
        element: Type,
    },
    Filter {
        result: GoIdentifier,
        element: Type,
    },
    Fold {
        result: GoIdentifier,
        init: GoExpression,
        go_type: Option<GoType>,
    },
    Find {
        result: GoIdentifier,
        payload: Type,
    },
    FindInto(FoundSink<'a>),
    All {
        result: GoIdentifier,
    },
}

impl PlannedLoop<'_> {
    fn result(&self) -> Option<&GoIdentifier> {
        match self {
            Self::Map { result, .. }
            | Self::Filter { result, .. }
            | Self::Fold { result, .. }
            | Self::Find { result, .. }
            | Self::All { result } => Some(result),
            Self::FindInto(sink) => sink.value,
        }
    }
}

struct SliceLoopShape<'a> {
    receiver: &'a Expression,
    kind: SliceLoop<&'a Expression>,
    callback: LoopCallback<'a, &'a Expression>,
    result_ty: Type,
    element_ty: Type,
}

pub(in crate::lowering) struct FindLoop<'a> {
    call: NativeCallContext<'a>,
    shape: SliceLoopShape<'a>,
}

struct LoopOperands<'a> {
    setup: Vec<Statement>,
    source: GoExpression,
    kind: SliceLoop<(GoExpression, Option<GoType>)>,
    callback: LoopCallback<'a, GoExpression>,
}

enum LoopKindOperand<'e, V> {
    Plain(SliceLoop<(GoExpression, Option<GoType>)>),
    Fold(&'e Expression, V),
}

struct PlannedLoopOperands<'a, 'e> {
    source: ValuePlan,
    kind: LoopKindOperand<'e, ValuePlan>,
    callback: LoopCallback<'a, ValuePlan>,
}

fn local(identifier: &GoIdentifier) -> GoExpression {
    GoExpression::identifier(identifier.clone())
}

fn name(text: &str) -> GoExpression {
    GoExpression::external_name(text.to_string())
}

fn callback_param<'a>(ctx: &'a NativeCallContext<'_>) -> (Option<&'a Type>, Option<&'a Type>) {
    let param = ctx.abi.param(ctx.args.len() - 1);
    (
        param.and_then(|param| param.declared.as_ref()),
        param.map(|param| &param.instantiated),
    )
}

impl Planner<'_> {
    pub(in crate::lowering) fn try_lower_slice_loop_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let call = self.queries().native_method_call(value)?;
        if !matches!(call.native_type, NativeTypeKind::Slice) {
            return None;
        }
        self.try_lower_slice_loop(&call, SliceLoopTarget::Into(go_name))
            .map(|result| result.setup)
    }

    pub(in crate::lowering) fn lower_find_loop(
        &mut self,
        search: FindLoop<'_>,
        sink: FoundSink<'_>,
    ) -> Vec<Statement> {
        self.lower_slice_loop(&search.call, search.shape, SliceLoopTarget::Found(sink))
            .0
    }

    pub(super) fn try_lower_slice_loop(
        &mut self,
        ctx: &NativeCallContext,
        target: SliceLoopTarget<'_>,
    ) -> Option<NativeCallResult> {
        let shape = self.queries().slice_loop_shape(ctx)?;
        let (setup, result) = self.lower_slice_loop(ctx, shape, target);
        let result = result.expect("a slice loop with a value target names its result");
        Some(NativeCallResult::new(
            setup,
            GoExpression::identifier(result),
        ))
    }

    fn lower_slice_loop(
        &mut self,
        ctx: &NativeCallContext,
        shape: SliceLoopShape<'_>,
        target: SliceLoopTarget<'_>,
    ) -> (Vec<Statement>, Option<GoIdentifier>) {
        let SliceLoopShape {
            receiver,
            kind,
            callback,
            result_ty,
            element_ty,
        } = shape;
        let LoopOperands {
            mut setup,
            source,
            kind,
            callback,
        } = self.sequence_slice_loop_operands(ctx, receiver, kind, callback, &result_ty);

        let planned = self.planned_loop(target, kind, &callback);
        self.queries()
            .push_slice_loop_declaration(&mut setup, &planned, &element_ty, &source);

        let (element_name, body_statements) =
            self.lower_slice_loop_iteration(&planned, &callback, &result_ty);

        setup.push(slice_loop_statement(
            &planned,
            element_name,
            source,
            body_statements,
        ));

        (setup, planned.result().cloned())
    }

    fn lower_slice_loop_iteration(
        &mut self,
        planned: &PlannedLoop<'_>,
        callback: &LoopCallback<'_, GoExpression>,
        result_ty: &Type,
    ) -> (GoIdentifier, Vec<Statement>) {
        let lowered_callback = match callback {
            LoopCallback::Lambda(lambda) => LoweredCallback::Body(lambda.body),
            LoopCallback::Value(callee) => LoweredCallback::Callee(callee),
        };

        let lower_body = |planner: &mut Self| {
            planner.with_scope(|this| {
                if let (
                    PlannedLoop::Fold { result, .. },
                    LoopCallback::Lambda(LambdaCallback {
                        accumulator: Some(accumulator),
                        ..
                    }),
                ) = (planned, callback)
                {
                    this.bind_loop_callback_param(*accumulator, Some(result.clone()));
                }
                let element_name = this.element_loop_name(planned, callback);
                let statements = this.lower_slice_loop_body(&SliceLoopBody {
                    planned,
                    callback: lowered_callback,
                    result_ty,
                    element_name: &element_name,
                });

                (element_name, statements)
            })
        };

        match planned.result() {
            Some(result) => {
                self.with_assign_target(&GoExpression::identifier(result.clone()), lower_body)
            }
            None => lower_body(self),
        }
    }

    fn sequence_slice_loop_operands<'s>(
        &mut self,
        ctx: &NativeCallContext,
        receiver: &Expression,
        kind: SliceLoop<&Expression>,
        callback: LoopCallback<'s, &Expression>,
        result_ty: &Type,
    ) -> LoopOperands<'s> {
        let mut source_operand = self.plan_operand(receiver, ExpressionContext::value());
        if self.facts.checked(receiver).receiver_change() == Some(ReceiverCoercion::AutoDeref) {
            source_operand = source_operand.dereference();
        }
        if matches!(kind, SliceLoop::Map { .. }) && source_operand.effects().must_evaluate_once() {
            self.pin_to_temp(&mut source_operand, "src");
        }
        let kind = match kind.split_fold() {
            Ok(kind) => LoopKindOperand::Plain(kind),
            Err(init) => {
                let origin = self
                    .facts
                    .function_type_origin(result_ty, SlotOrigin::Lisette);
                let plan = self.plan_composite_value(
                    init,
                    ExpressionContext::value().with_function_slot_origin(origin),
                );
                if !plan.effects().can_move_across(source_operand.effects()) {
                    self.pin_to_temp(&mut source_operand, "src");
                }
                LoopKindOperand::Fold(init, plan)
            }
        };
        let callback = match callback {
            LoopCallback::Lambda(lambda) => LoopCallback::Lambda(lambda),
            LoopCallback::Value(callee) => LoopCallback::Value(self.plan_loop_callee(ctx, callee)),
        };
        let operands = PlannedLoopOperands {
            source: source_operand,
            kind,
            callback,
        };
        self.sequence_loop_operands(operands, ctx.capture_boundary, result_ty)
    }

    fn sequence_loop_operands<'s>(
        &mut self,
        operands: PlannedLoopOperands<'s, '_>,
        boundary: CaptureBoundary,
        result_ty: &Type,
    ) -> LoopOperands<'s> {
        let PlannedLoopOperands {
            source,
            kind,
            callback,
        } = operands;
        let (mut setup, source, kind, callback) = match (kind, callback) {
            (LoopKindOperand::Fold(init, plan), LoopCallback::Value(callee)) => {
                let sequenced = self.sequence_value_array([source, plan, callee], boundary, "arg");
                let [source, value, callee] = sequenced.values;
                let kind = LoopKindOperand::Fold(init, value);
                (sequenced.setup, source, kind, LoopCallback::Value(callee))
            }
            (LoopKindOperand::Fold(init, plan), LoopCallback::Lambda(lambda)) => {
                let sequenced = self.sequence_value_array([source, plan], boundary, "arg");
                let [source, value] = sequenced.values;
                let kind = LoopKindOperand::Fold(init, value);
                (sequenced.setup, source, kind, LoopCallback::Lambda(lambda))
            }
            (LoopKindOperand::Plain(kind), LoopCallback::Value(callee)) => {
                let sequenced = self.sequence_value_array([source, callee], boundary, "arg");
                let [source, callee] = sequenced.values;
                (
                    sequenced.setup,
                    source,
                    LoopKindOperand::Plain(kind),
                    LoopCallback::Value(callee),
                )
            }
            (LoopKindOperand::Plain(kind), LoopCallback::Lambda(lambda)) => {
                let sequenced = self.sequence_value_array([source], boundary, "arg");
                let [source] = sequenced.values;
                (
                    sequenced.setup,
                    source,
                    LoopKindOperand::Plain(kind),
                    LoopCallback::Lambda(lambda),
                )
            }
        };
        let kind = match kind {
            LoopKindOperand::Plain(kind) => kind,
            LoopKindOperand::Fold(init, value) => {
                SliceLoop::Fold(self.accumulator_init(&mut setup, init, value, result_ty))
            }
        };
        LoopOperands {
            setup,
            source,
            kind,
            callback,
        }
    }

    fn planned_loop<'t>(
        &mut self,
        target: SliceLoopTarget<'t>,
        kind: SliceLoop<(GoExpression, Option<GoType>)>,
        callback: &LoopCallback<'_, GoExpression>,
    ) -> PlannedLoop<'t> {
        match target {
            SliceLoopTarget::Found(sink) => PlannedLoop::FindInto(sink),
            target @ SliceLoopTarget::Fresh | target @ SliceLoopTarget::Into(_) => {
                let result = match (target, callback) {
                    (SliceLoopTarget::Into(name), _) => name.clone(),
                    (
                        _,
                        LoopCallback::Lambda(LambdaCallback {
                            accumulator: Some(accumulator),
                            ..
                        }),
                    ) => self.claim_go_name(accumulator.source_name()),
                    _ => self.fresh_local("result"),
                };

                self.scope.declare_go_name(result.spelling());
                match kind {
                    SliceLoop::Map { element } => {
                        let index = self.fresh_local("i");
                        self.scope.declare_go_name(index.spelling());
                        PlannedLoop::Map {
                            result,
                            index,
                            element,
                        }
                    }
                    SliceLoop::Filter { element } => PlannedLoop::Filter { result, element },
                    SliceLoop::Fold((init, go_type)) => PlannedLoop::Fold {
                        result,
                        init,
                        go_type,
                    },
                    SliceLoop::Find { payload } => PlannedLoop::Find { result, payload },
                    SliceLoop::All => PlannedLoop::All { result },
                }
            }
        }
    }

    fn accumulator_init(
        &mut self,
        setup: &mut Vec<Statement>,
        init: &Expression,
        value: GoExpression,
        result_ty: &Type,
    ) -> (GoExpression, Option<GoType>) {
        let coercion = self.value_slot_coercion(init, result_ty);
        let typed = self.queries().declaration_needs_go_type(
            init,
            value.constant_kind(),
            &coercion,
            result_ty,
        );
        let (coercion_setup, value) = coercion.lower(self, value);
        setup.extend(coercion_setup);
        (value, typed.then(|| self.queries().go_type(result_ty)))
    }

    fn plan_loop_callee(&mut self, ctx: &NativeCallContext<'_>, callee: &Expression) -> ValuePlan {
        let (declared, instantiated) = callback_param(ctx);
        let mut operand = self.plan_prelude_arg(callee, declared, instantiated);
        if operand.effects().must_evaluate_once()
            || self.queries().place_read_stability(callee) == Stability::Observable
            || operand.expression().map(GoExpression::syntax_form) != Some(OperandForm::Name)
        {
            self.pin_to_temp(&mut operand, "callee");
        }
        operand
    }

    fn element_loop_name(
        &mut self,
        planned: &PlannedLoop<'_>,
        callback: &LoopCallback<'_, GoExpression>,
    ) -> GoIdentifier {
        if let LoopCallback::Lambda(lambda) = callback {
            let name = self.bind_loop_callback_param(lambda.element, None);
            if name.spelling() != "_"
                || matches!(
                    planned,
                    PlannedLoop::Map { .. } | PlannedLoop::Fold { .. } | PlannedLoop::All { .. }
                )
            {
                return name;
            }
        }
        let name = self.fresh_local("v");
        self.scope.declare_go_name(name.spelling());
        name
    }

    fn bind_loop_callback_param(
        &mut self,
        param: LoopParam<'_>,
        existing: Option<GoIdentifier>,
    ) -> GoIdentifier {
        match existing {
            Some(name) => {
                self.scope.declare_go_name(name.spelling());
                self.scope.bind_existing_identifier(
                    param.identifier.as_str(),
                    param.binding.as_slice(),
                    name.clone(),
                );
                name
            }
            None => self.claim_binding(
                param.identifier,
                param.binding.as_slice(),
                param.source_name(),
            ),
        }
    }

    fn lower_slice_loop_body(&mut self, loop_body: &SliceLoopBody<'_>) -> Vec<Statement> {
        let line = match loop_body.callback {
            LoweredCallback::Body(body) => self.facts.source_position(&body.get_span()),
            LoweredCallback::Callee(_) => None,
        };
        let statements = self.lower_slice_loop_statements(loop_body);
        directed_first(line, statements)
    }

    fn lower_slice_loop_statements(&mut self, loop_body: &SliceLoopBody<'_>) -> Vec<Statement> {
        let SliceLoopBody {
            planned,
            callback,
            result_ty,
            element_name,
        } = *loop_body;

        let then_body = match planned {
            PlannedLoop::Map {
                result,
                index,
                element,
            } => {
                let slot = GoExpression::index(local(result), local(index));
                return self.lower_callback_into_slot(
                    callback,
                    &slot,
                    element,
                    vec![local(element_name)],
                );
            }
            PlannedLoop::Fold { result, .. } => {
                return self.lower_callback_into_slot(
                    callback,
                    &local(result),
                    result_ty,
                    vec![local(result), local(element_name)],
                );
            }
            PlannedLoop::Filter { result, .. } => vec![assign(
                local(result),
                GoExpression::call(name("append"), vec![local(result), local(element_name)]),
            )],
            PlannedLoop::Find { result, payload } => {
                found_some_statements(result, &self.queries().go_type(payload), element_name)
            }
            PlannedLoop::FindInto(sink) => found_into_statements(sink, element_name),
            PlannedLoop::All { result } => vec![
                assign(local(result), GoExpression::boolean(false)),
                LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
            ],
        };

        self.lower_guarded_loop_step(
            planned,
            callback,
            element_name,
            LoweredBlock {
                statements: then_body,
            },
        )
    }

    fn lower_callback_into_slot(
        &mut self,
        callback: LoweredCallback<'_>,
        slot: &GoExpression,
        slot_ty: &Type,
        callee_arguments: Vec<GoExpression>,
    ) -> Vec<Statement> {
        match callback {
            LoweredCallback::Body(body) => {
                let place = PlacePlan::Assign {
                    local: slot,
                    target_ty: slot_ty,
                };
                self.lower_block_to_place(body, &place).statements
            }
            LoweredCallback::Callee(callee) => vec![assign(
                slot.clone(),
                GoExpression::call(callee.clone(), callee_arguments),
            )],
        }
    }

    fn lower_guarded_loop_step(
        &mut self,
        planned: &PlannedLoop<'_>,
        callback: LoweredCallback<'_>,
        element_name: &GoIdentifier,
        then_body: LoweredBlock,
    ) -> Vec<Statement> {
        let negated = matches!(planned, PlannedLoop::All { .. });
        let plan = match callback {
            LoweredCallback::Callee(callee) => {
                let call = GoExpression::call(callee.clone(), vec![local(element_name)]);
                if negated {
                    ValuePlan::computed(Vec::new(), GoExpression::unary(GoUnaryOperator::Not, call))
                } else {
                    ValuePlan::computed(Vec::new(), call)
                }
            }
            LoweredCallback::Body(body) if negated => {
                self.plan_unary_not(body, ExpressionContext::value())
            }
            LoweredCallback::Body(body) => {
                if is_conjunction(body) {
                    let guard = self.lower_conjunction_guard(body, |_| then_body);
                    return vec![guard.into_statement().into()];
                }
                self.plan_value(body, ExpressionContext::value())
            }
        };

        let (mut statements, condition) = plan.into_parts();
        statements
            .push(LoweredStatement::If(IfPlan::plain(condition, then_body, ElseArm::None)).into());
        statements
    }
}

fn found_some_statements(
    result: &GoIdentifier,
    payload: &GoType,
    element_name: &GoIdentifier,
) -> Vec<Statement> {
    vec![
        assign(
            local(result),
            prelude_call(
                option_variant("Some").constructor,
                Some(GoType::type_argument(payload)),
                vec![local(element_name)],
            ),
        ),
        LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
    ]
}

fn found_into_statements(sink: &FoundSink<'_>, element_name: &GoIdentifier) -> Vec<Statement> {
    let mut statements = Vec::new();
    if let Some(value) = sink.value {
        statements.push(assign(local(value), local(element_name)));
    }
    statements.push(assign(
        GoExpression::identifier(sink.flag.clone()),
        GoExpression::boolean(true),
    ));
    statements.push(LoweredStatement::Break(LoopTransfer::Unlabeled).into());
    statements
}

fn slice_loop_statement(
    planned: &PlannedLoop<'_>,
    element_name: GoIdentifier,
    source: GoExpression,
    body_statements: Vec<Statement>,
) -> Statement {
    let header = LoopHeader::Range {
        key: match (planned, element_name.spelling()) {
            (PlannedLoop::Map { index, .. }, _) => Some(index.clone()),
            (_, "_") => None,
            _ => Some(GoIdentifier::blank()),
        },
        value: (element_name.spelling() != "_").then_some(element_name),
        iterable: source,
    };

    LoweredStatement::Loop(LoopPlan {
        kind: if matches!(planned, PlannedLoop::FindInto(_)) {
            LoopKind::FoundFlagSearch
        } else {
            LoopKind::Generated { label: None }
        },
        header,
        body: LoweredBlock {
            statements: body_statements,
        },
    })
    .into()
}

impl PlannerQueries<'_, '_> {
    fn slice_loop_shape<'a>(&self, ctx: &NativeCallContext<'a>) -> Option<SliceLoopShape<'a>> {
        let (receiver, args) = ctx.receiver_and_arguments()?;
        if ctx.spread.is_some() {
            return None;
        }
        let result_ty = ctx.call_ty?.clone();
        let result_argument = || self.facts.first_type_argument(&result_ty);
        let (kind, callback) = match (ctx.method, args) {
            ("map", [callback]) => (
                SliceLoop::Map {
                    element: result_argument()?,
                },
                callback,
            ),
            ("filter", [callback]) => (
                SliceLoop::Filter {
                    element: result_argument()?,
                },
                callback,
            ),
            ("fold", [init, callback]) => (SliceLoop::Fold(init), callback),
            ("find", [callback]) => (
                SliceLoop::Find {
                    payload: result_argument()?,
                },
                callback,
            ),
            ("all", [callback]) => (SliceLoop::All, callback),
            _ => return None,
        };
        let callback = if let Expression::Lambda { params, body, .. } = callback {
            LoopCallback::Lambda(lambda_callback(&kind, params, body)?)
        } else if self.callback_calls_directly(ctx, callback) {
            LoopCallback::Value(callback)
        } else {
            return None;
        };

        let element_ty = self.facts.first_type_argument(&receiver.get_type())?;
        Some(SliceLoopShape {
            receiver,
            kind,
            callback,
            result_ty,
            element_ty,
        })
    }

    fn callback_calls_directly(&self, ctx: &NativeCallContext<'_>, callee: &Expression) -> bool {
        if is_closure_literal(callee) {
            return false;
        }
        let (declared, instantiated) = callback_param(ctx);
        self.prelude_callback_operands_as_is(callee, declared, instantiated)
    }

    pub(in crate::lowering) fn find_loop<'a>(
        &self,
        call: NativeCallContext<'a>,
    ) -> Option<FindLoop<'a>> {
        if !matches!(call.native_type, NativeTypeKind::Slice) || call.method != "find" {
            return None;
        }
        let shape = self.slice_loop_shape(&call)?;
        Some(FindLoop { call, shape })
    }

    fn push_slice_loop_declaration(
        &self,
        setup: &mut Vec<Statement>,
        planned: &PlannedLoop<'_>,
        element_ty: &Type,
        source: &GoExpression,
    ) {
        let declaration = match planned {
            PlannedLoop::Map {
                result, element, ..
            } => define(
                result.clone(),
                GoExpression::call(
                    name("make"),
                    vec![
                        GoExpression::type_name(GoType::slice_of(&self.go_type(element))),
                        GoExpression::call(name("len"), vec![source.clone()]),
                    ],
                ),
            ),
            PlannedLoop::Filter { result, element } => {
                declare(result.clone(), GoType::slice_of(&self.go_type(element)))
            }
            PlannedLoop::Fold {
                result,
                init,
                go_type,
            } => match go_type {
                Some(go_type) => LoweredStatement::VarDeclaration {
                    name: result.clone(),
                    go_type: go_type.clone(),
                    value: Some(init.clone()),
                }
                .into(),
                None => define(result.clone(), init.clone()),
            },
            PlannedLoop::Find { result, payload } => define(
                result.clone(),
                prelude_call(
                    option_variant("None").constructor,
                    Some(GoType::type_argument(&self.go_type(payload))),
                    Vec::new(),
                ),
            ),
            PlannedLoop::FindInto(sink) => {
                if let Some(value) = sink.value {
                    let go_type = self.go_type(element_ty);
                    setup.push(declare(value.clone(), go_type));
                }
                define(sink.flag.clone(), GoExpression::boolean(false))
            }
            PlannedLoop::All { result } => define(result.clone(), GoExpression::boolean(true)),
        };

        setup.push(declaration);
    }
}

fn lambda_callback<'a, Init>(
    kind: &SliceLoop<Init>,
    params: &'a [Binding],
    body: &'a Expression,
) -> Option<LambdaCallback<'a>> {
    let params = params
        .iter()
        .map(LoopParam::of)
        .collect::<Option<Vec<_>>>()?;
    if !body_moves_into_a_loop(body) {
        return None;
    }
    let (accumulator, element) = match (kind, params.as_slice()) {
        (SliceLoop::Fold(_), [accumulator, element]) => (Some(*accumulator), *element),
        (SliceLoop::Fold(_), _) => return None,
        (_, [element]) => (None, *element),
        _ => return None,
    };
    if accumulator.is_some() && body_captures_by_reference(body) {
        return None;
    }
    Some(LambdaCallback {
        body,
        accumulator,
        element,
    })
}
