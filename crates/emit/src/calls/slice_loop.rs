use crate::plan::local::GoIdentifier;
use syntax::ast::{Binding, Expression, Pattern};
use syntax::program::NativeTypeKind;
use syntax::types::Type;

use super::NativeCallContext;
use super::native::NativeCallResult;
use crate::Planner;
use crate::abi::is_closure_literal;
use crate::abi::layout::SlotOrigin;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::prelude_call;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, Statement, assign, define,
};
use crate::plan::go_expression::UnaryOp;
use crate::plan::lower::is_conjunction;
use crate::plan::values::{GoExpression, OperandForm, Stability, ValuePlan};
use crate::statements::let_binding::needs_explicit_type_declaration;
use crate::types::go_type::GoType;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceLoop {
    Map,
    Filter,
    Fold,
    Find,
    All,
}

impl SliceLoop {
    fn from_method(method: &str) -> Option<Self> {
        match method {
            "map" => Some(Self::Map),
            "filter" => Some(Self::Filter),
            "fold" => Some(Self::Fold),
            "find" => Some(Self::Find),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn argument_count(self) -> usize {
        match self {
            Self::Fold => 2,
            _ => 1,
        }
    }
}

/// An escape belongs to the lambda, and inlining would hand it to the
/// enclosing function.
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
    // A nested lambda keeps its own escapes.
    matches!(body, Expression::Lambda { .. })
        || body
            .children()
            .iter()
            .all(|child| body_moves_into_a_loop(child))
}

/// Fold names the accumulator after the result the loop rewrites, so anything
/// holding it past its iteration would read a later one.
fn body_captures_by_reference(body: &Expression) -> bool {
    matches!(
        body,
        Expression::Lambda { .. } | Expression::Task { .. } | Expression::Reference { .. }
    ) || body
        .children()
        .iter()
        .any(|child| body_captures_by_reference(child))
}

fn identifier_of(param: &Binding) -> Option<&Pattern> {
    matches!(param.pattern, Pattern::Identifier { .. }).then_some(&param.pattern)
}

enum LoopCallback<'a> {
    Lambda {
        body: &'a Expression,
        patterns: Vec<&'a Pattern>,
    },
    Value(&'a Expression),
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
    element_ty: &'a Type,
    element_name: &'a GoIdentifier,
}

#[derive(Clone, Copy)]
pub(crate) struct FoundSink<'a> {
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
    },
    Filter {
        result: GoIdentifier,
    },
    Fold {
        result: GoIdentifier,
        init: GoExpression,
        go_type: Option<GoType>,
    },
    Find {
        result: GoIdentifier,
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
            | Self::Filter { result }
            | Self::Fold { result, .. }
            | Self::Find { result }
            | Self::All { result } => Some(result),
            Self::FindInto(sink) => sink.value,
        }
    }
}

fn peel_single_expression_block(body: &Expression) -> &Expression {
    match body {
        Expression::Block { items, .. } if items.len() == 1 => {
            peel_single_expression_block(&items[0])
        }
        _ => body,
    }
}

struct SliceLoopShape<'a> {
    kind: SliceLoop,
    callback: LoopCallback<'a>,
    result_ty: Type,
    element_ty: Type,
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
    /// The parts of a slice method call that inline as a loop.
    fn slice_loop_shape<'a>(&self, ctx: &NativeCallContext<'a>) -> Option<SliceLoopShape<'a>> {
        let (receiver, args) = ctx.receiver_and_arguments()?;
        let kind = SliceLoop::from_method(ctx.method)?;
        if args.len() != kind.argument_count() || ctx.spread.is_some() {
            return None;
        }
        let callback = match args.last()?.unwrap_parens() {
            Expression::Lambda { params, body, .. } => {
                let body = peel_single_expression_block(body);
                let patterns: Vec<&Pattern> = params.iter().filter_map(identifier_of).collect();
                if patterns.len() != params.len() || !body_moves_into_a_loop(body) {
                    return None;
                }
                let expects_accumulator = kind == SliceLoop::Fold;
                if patterns.len() != 1 + expects_accumulator as usize {
                    return None;
                }
                if expects_accumulator && body_captures_by_reference(body) {
                    return None;
                }
                LoopCallback::Lambda { body, patterns }
            }
            callee if self.callback_calls_directly(ctx, callee) => LoopCallback::Value(callee),
            _ => return None,
        };
        let result_ty = ctx.call_ty?.clone();
        let element_ty = self
            .facts
            .strip_and_peel(&receiver.get_type())
            .get_type_params()?
            .first()?
            .clone();
        Some(SliceLoopShape {
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
        self.prelude_callback_stages_as_is(callee, declared, instantiated)
    }

    /// `let xs = ys.map(..)` fills `xs` as the loop's own result.
    pub(crate) fn lower_slice_loop_into(
        &mut self,
        value: &Expression,
        go_name: &GoIdentifier,
    ) -> Option<Vec<Statement>> {
        let call = self.native_method_call(value)?;
        if !matches!(call.native_type, NativeTypeKind::Slice) {
            return None;
        }
        self.try_lower_slice_loop(&call, SliceLoopTarget::Into(go_name))
            .map(|result| result.setup)
    }

    pub(crate) fn find_loop_fuses(&self, call: &NativeCallContext<'_>) -> bool {
        if !matches!(call.native_type, NativeTypeKind::Slice) || call.method != "find" {
            return false;
        }
        self.slice_loop_shape(call).is_some()
    }

    pub(crate) fn lower_find_loop(
        &mut self,
        call: &NativeCallContext<'_>,
        sink: FoundSink<'_>,
    ) -> Vec<Statement> {
        self.try_lower_slice_loop(call, SliceLoopTarget::Found(sink))
            .expect("find_loop_fuses accepted this call")
            .setup
    }

    /// Lower `xs.map(|x| ...)` and its siblings to the loop the prelude helper
    /// runs. `None` keeps the helper call.
    pub(super) fn try_lower_slice_loop(
        &mut self,
        ctx: &NativeCallContext,
        target: SliceLoopTarget<'_>,
    ) -> Option<NativeCallResult> {
        let (receiver, args) = ctx.receiver_and_arguments()?;
        let SliceLoopShape {
            kind,
            callback,
            result_ty,
            element_ty,
        } = self.slice_loop_shape(ctx)?;
        if matches!(target, SliceLoopTarget::Found(_)) && kind != SliceLoop::Find {
            return None;
        }

        let mut source_staged = self.plan_operand(receiver, ExpressionContext::value());
        // `map` reads the source twice, for its length and for the range.
        if kind == SliceLoop::Map && !source_staged.effects().can_duplicate() {
            self.pin_staged(&mut source_staged, "src");
        }
        let mut stages = vec![source_staged];
        if kind == SliceLoop::Fold {
            let origin = self.function_type_origin(&result_ty, SlotOrigin::Lisette);
            let init = self.lower_composite_value(
                &args[0],
                ExpressionContext::value().with_function_slot_origin(origin),
            );
            // The accumulator is declared before the loop ranges over the source.
            if !init.effects().can_move_across(stages[0].effects()) {
                self.pin_staged(&mut stages[0], "src");
            }
            stages.push(init);
        }
        if let LoopCallback::Value(callee) = &callback {
            stages.push(self.stage_loop_callee(ctx, callee));
        }
        let sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let source = values.next().expect("the source is staged first");
        let init = (kind == SliceLoop::Fold).then(|| {
            let value = values.next().expect("fold stages its initial value");
            self.accumulator_init(&mut setup, &args[0], value, &result_ty)
        });
        let callee = matches!(callback, LoopCallback::Value(_))
            .then(|| values.next().expect("a function value stages its callee"));

        let planned = match target {
            SliceLoopTarget::Found(sink) => PlannedLoop::FindInto(sink),
            target => {
                let result = match (target, &callback) {
                    (SliceLoopTarget::Into(name), _) => name.clone(),
                    (_, LoopCallback::Lambda { patterns, .. }) if kind == SliceLoop::Fold => {
                        self.accumulator_slot_name(patterns[0], receiver)
                    }
                    _ => self.fresh_local(Some("result")),
                };
                // The result claims its name before the `map` index does.
                self.declare(result.spelling());
                match kind {
                    SliceLoop::Map => {
                        let index = self.fresh_local(Some("i"));
                        self.declare(index.spelling());
                        PlannedLoop::Map { result, index }
                    }
                    SliceLoop::Filter => PlannedLoop::Filter { result },
                    SliceLoop::Fold => {
                        let (init, go_type) = init.expect("fold stages its initial value");
                        PlannedLoop::Fold {
                            result,
                            init,
                            go_type,
                        }
                    }
                    SliceLoop::Find => PlannedLoop::Find { result },
                    SliceLoop::All => PlannedLoop::All { result },
                }
            }
        };
        self.push_slice_loop_declaration(&mut setup, &planned, &result_ty, &element_ty, &source);

        let lowered_callback = match &callback {
            LoopCallback::Lambda { body, .. } => LoweredCallback::Body(body),
            LoopCallback::Value(_) => LoweredCallback::Callee(
                callee.as_ref().expect("a function value stages its callee"),
            ),
        };
        let lower_body = |planner: &mut Self| {
            planner.with_scope(|this| {
                if let (PlannedLoop::Fold { result, .. }, LoopCallback::Lambda { patterns, .. }) =
                    (&planned, &callback)
                {
                    this.bind_loop_callback_param(patterns[0], Some(result.clone()));
                }
                let element_name = this.element_loop_name(kind, &callback);
                let statements = this.lower_slice_loop_body(&SliceLoopBody {
                    planned: &planned,
                    callback: lowered_callback,
                    result_ty: &result_ty,
                    element_ty: &element_ty,
                    element_name: &element_name,
                });
                (element_name, statements)
            })
        };
        let (element_name, body_statements) = match planned.result() {
            Some(result) => {
                self.with_assign_target(&GoExpression::identifier(result.clone()), lower_body)
            }
            None => lower_body(self),
        };

        // Go rejects a range variable nothing reads.
        let header = LoopHeader::Range {
            key: match (&planned, element_name.spelling()) {
                (PlannedLoop::Map { index, .. }, _) => Some(index.clone()),
                (_, "_") => None,
                _ => Some(GoIdentifier::external("_".to_string())),
            },
            value: (element_name.spelling() != "_").then_some(element_name),
            iterable: source,
        };
        setup.push(
            LoweredStatement::Loop(LoopPlan {
                kind: LoopKind::Generated { label: None },
                header,
                body: LoweredBlock {
                    statements: body_statements,
                },
            })
            .into(),
        );

        let result = planned
            .result()
            .cloned()
            .map_or_else(GoExpression::empty, GoExpression::identifier);
        Some(NativeCallResult::new(setup, result))
    }

    fn accumulator_init(
        &mut self,
        setup: &mut Vec<Statement>,
        init: &Expression,
        value: GoExpression,
        result_ty: &Type,
    ) -> (GoExpression, Option<GoType>) {
        let coercion = self.value_slot_coercion(init, result_ty);
        let typed = (coercion.is_identity()
            && self
                .constant_needs_go_type(value.constant_kind(), result_ty)
                .is_some())
            || needs_explicit_type_declaration(self, init, result_ty);
        let (coercion_setup, value) = coercion.lower(self, value);
        setup.extend(coercion_setup);
        (value, typed.then(|| self.go_type(result_ty)))
    }

    fn stage_loop_callee(&mut self, ctx: &NativeCallContext<'_>, callee: &Expression) -> ValuePlan {
        let (declared, instantiated) = callback_param(ctx);
        let mut staged = self.stage_prelude_arg(callee, declared, instantiated);
        // The loop reads the callee once per element.
        if !staged.effects().can_duplicate()
            || self.place_read_stability(callee) == Stability::Observable
            || staged.expression().syntax_form() != OperandForm::Name
        {
            self.pin_staged(&mut staged, "callee");
        }
        staged
    }

    fn accumulator_slot_name(&mut self, pattern: &Pattern, receiver: &Expression) -> GoIdentifier {
        let Pattern::Identifier { binding, .. } = pattern else {
            return self.fresh_local(Some("result"));
        };
        match self.go_name_for_binding(pattern) {
            Some(name)
                if self
                    .scope
                    .expression_reads_go_name(receiver, &name.preferred()) =>
            {
                self.fresh_source(&name)
            }
            Some(name) => self.claim_declared_go_name(binding.as_slice(), &name),
            None => self.fresh_local(Some("result")),
        }
    }

    /// `filter` and `find` read the element back whatever the callback does.
    fn element_loop_name(&mut self, kind: SliceLoop, callback: &LoopCallback<'_>) -> GoIdentifier {
        if let LoopCallback::Lambda { patterns, .. } = callback {
            let name = self.bind_loop_callback_param(patterns[patterns.len() - 1], None);
            if name.spelling() != "_"
                || matches!(kind, SliceLoop::Map | SliceLoop::Fold | SliceLoop::All)
            {
                return name;
            }
        }
        let name = self.fresh_local(Some("v"));
        self.declare(name.spelling());
        name
    }

    fn bind_loop_callback_param(
        &mut self,
        pattern: &Pattern,
        existing: Option<GoIdentifier>,
    ) -> GoIdentifier {
        let Pattern::Identifier {
            identifier,
            binding: ids,
            ..
        } = pattern
        else {
            unreachable!("callback parameters are checked for identifier patterns");
        };
        match existing {
            Some(name) => {
                self.declare(name.spelling());
                self.scope
                    .bind_identifier(identifier.as_str(), ids.as_slice(), name.clone());
                name
            }
            None => match self.go_name_for_binding(pattern) {
                Some(name) => self.claim_declared_binding(identifier, ids.as_slice(), &name),
                None => GoIdentifier::external("_".to_string()),
            },
        }
    }

    fn push_slice_loop_declaration(
        &mut self,
        setup: &mut Vec<Statement>,
        planned: &PlannedLoop<'_>,
        result_ty: &Type,
        element_ty: &Type,
        source: &GoExpression,
    ) {
        let declaration = match planned {
            PlannedLoop::Map { result, .. } => {
                let element = self.first_type_argument_go_type(result_ty);
                define(
                    result.clone(),
                    GoExpression::call(
                        name("make"),
                        vec![
                            GoExpression::type_name(GoType::slice_of(&element)),
                            GoExpression::call(name("len"), vec![source.clone()]),
                        ],
                    ),
                )
            }
            PlannedLoop::Filter { result } => LoweredStatement::VarDecl {
                name: result.clone(),
                go_type: GoType::slice_of(&self.first_type_argument_go_type(result_ty)),
                value: None,
            }
            .into(),
            PlannedLoop::Fold {
                result,
                init,
                go_type,
            } => match go_type {
                Some(go_type) => LoweredStatement::VarDecl {
                    name: result.clone(),
                    go_type: go_type.clone(),
                    value: Some(init.clone()),
                }
                .into(),
                None => define(result.clone(), init.clone()),
            },
            PlannedLoop::Find { result } => {
                let payload = self.first_type_argument_go_type(result_ty);
                define(
                    result.clone(),
                    prelude_call(
                        "MakeOptionNone",
                        GoType::type_argument(&payload),
                        Vec::new(),
                    ),
                )
            }
            PlannedLoop::FindInto(sink) => {
                if let Some(value) = sink.value {
                    let go_type = self.go_type(element_ty);
                    setup.push(
                        LoweredStatement::VarDecl {
                            name: value.clone(),
                            go_type,
                            value: None,
                        }
                        .into(),
                    );
                }
                define(
                    sink.flag.clone(),
                    GoExpression::literal("false".to_string()),
                )
            }
            PlannedLoop::All { result } => {
                define(result.clone(), GoExpression::literal("true".to_string()))
            }
        };
        setup.push(declaration);
    }

    fn lower_slice_loop_body(&mut self, loop_body: &SliceLoopBody<'_>) -> Vec<Statement> {
        let SliceLoopBody {
            planned,
            callback,
            result_ty,
            element_ty,
            element_name,
        } = *loop_body;

        let then_body = match planned {
            // Map and fold store their callback's value, so it lowers into the slot.
            PlannedLoop::Map { result, index } => {
                let slot = GoExpression::index(local(result), local(index));
                return match callback {
                    LoweredCallback::Body(body) => {
                        let element_ty = self
                            .first_type_argument(result_ty)
                            .expect("map returns a slice carrying its element type");
                        self.lower_body_into_slot(body, &slot, &element_ty)
                    }
                    LoweredCallback::Callee(callee) => vec![assign(
                        slot,
                        GoExpression::call(callee.clone(), vec![local(element_name)]),
                    )],
                };
            }
            PlannedLoop::Fold { result, .. } => {
                return match callback {
                    LoweredCallback::Body(body) => {
                        self.lower_body_into_slot(body, &local(result), result_ty)
                    }
                    LoweredCallback::Callee(callee) => vec![assign(
                        local(result),
                        GoExpression::call(
                            callee.clone(),
                            vec![local(result), local(element_name)],
                        ),
                    )],
                };
            }
            PlannedLoop::Filter { result } => vec![assign(
                local(result),
                GoExpression::call(name("append"), vec![local(result), local(element_name)]),
            )],
            PlannedLoop::Find { result } => {
                let payload = self.go_type(element_ty);
                vec![
                    assign(
                        local(result),
                        prelude_call(
                            "MakeOptionSome",
                            GoType::type_argument(&payload),
                            vec![local(element_name)],
                        ),
                    ),
                    LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
                ]
            }
            PlannedLoop::FindInto(sink) => {
                let mut statements = Vec::new();
                if let Some(value) = sink.value {
                    statements.push(assign(local(value), local(element_name)));
                }
                statements.push(assign(
                    GoExpression::identifier(sink.flag.clone()),
                    GoExpression::literal("true".to_string()),
                ));
                statements.push(LoweredStatement::Break(LoopTransfer::Unlabeled).into());
                statements
            }
            PlannedLoop::All { result } => vec![
                assign(local(result), GoExpression::literal("false".to_string())),
                LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
            ],
        };

        let then_body = LoweredBlock {
            statements: then_body,
        };
        let negated = matches!(planned, PlannedLoop::All { .. });
        let plan = match callback {
            LoweredCallback::Callee(callee) => {
                let call = GoExpression::call(callee.clone(), vec![local(element_name)]);
                if negated {
                    ValuePlan::computed(Vec::new(), GoExpression::unary(UnaryOp::Not, call))
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
                self.lower_value(body, ExpressionContext::value())
            }
        };
        let (mut statements, condition) = plan.into_parts();
        statements
            .push(LoweredStatement::If(IfPlan::plain(condition, then_body, ElseArm::None)).into());
        statements
    }

    fn lower_body_into_slot(
        &mut self,
        body: &Expression,
        slot: &GoExpression,
        target_ty: &Type,
    ) -> Vec<Statement> {
        let place = PlacePlan::Assign {
            local: slot,
            target_ty: Some(target_ty),
        };
        self.lower_block_to_place(body, &place).statements
    }

    fn first_type_argument(&self, ty: &Type) -> Option<Type> {
        ty.get_type_params()
            .and_then(|params| params.first().cloned())
    }

    fn first_type_argument_go_type(&self, ty: &Type) -> GoType {
        let argument = self
            .first_type_argument(ty)
            .expect("slice and option results carry a type argument");
        self.go_type(&argument)
    }
}
