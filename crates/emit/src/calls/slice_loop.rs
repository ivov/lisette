use syntax::ast::{Binding, Expression, Pattern};
use syntax::program::NativeTypeKind;
use syntax::types::Type;

use super::native::NativeCallResult;
use super::{NativeCallContext, NativeCallForm, NativeMethodCall};
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::control_flow::fallible::prelude_call;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopHeader, LoopKind, LoopPlan, LoopTransfer, LoweredBlock, LoweredStatement,
    PlacePlan, Statement, assign, define,
};
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceLoop {
    Map,
    Filter,
    Fold,
    Find,
}

impl SliceLoop {
    fn from_method(method: &str) -> Option<Self> {
        match method {
            "map" => Some(Self::Map),
            "filter" => Some(Self::Filter),
            "fold" => Some(Self::Fold),
            "find" => Some(Self::Find),
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

#[derive(Clone, Copy)]
struct SliceLoopBody<'a> {
    planned: &'a PlannedLoop<'a>,
    body: &'a Expression,
    result_ty: &'a Type,
    element_ty: &'a Type,
    element_name: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct FoundSink<'a> {
    pub value: Option<&'a str>,
    pub flag: &'a str,
}

#[derive(Clone, Copy)]
pub(super) enum SliceLoopTarget<'a> {
    Fresh,
    Into(&'a str),
    Found(FoundSink<'a>),
}

enum PlannedLoop<'a> {
    Map {
        result: String,
        index: String,
    },
    Filter {
        result: String,
    },
    Fold {
        result: String,
        accumulator: &'a Pattern,
        init: GoExpression,
    },
    Find {
        result: String,
    },
    FindInto(FoundSink<'a>),
}

impl PlannedLoop<'_> {
    fn result(&self) -> Option<&str> {
        match self {
            Self::Map { result, .. }
            | Self::Filter { result }
            | Self::Fold { result, .. }
            | Self::Find { result } => Some(result),
            Self::FindInto(sink) => sink.value,
        }
    }
}

fn loop_context<'a>(call: &'a NativeMethodCall<'a>, call_ty: &'a Type) -> NativeCallContext<'a> {
    NativeCallContext {
        function: call.function,
        form: NativeCallForm::method(call.function),
        args: call.args,
        spread: call.spread,
        resolved_type_args: call.resolved_type_args,
        abi: &call.abi,
        call_ty: Some(call_ty),
        native_type: &NativeTypeKind::Slice,
        method: call.method,
        capture_boundary: CaptureBoundary::SiblingSequence,
        retired_receiver: None,
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
    body: &'a Expression,
    patterns: Vec<&'a Pattern>,
    result_ty: Type,
    element_ty: Type,
}

fn name(text: &str) -> GoExpression {
    GoExpression::name(text.to_string())
}

impl Planner<'_> {
    /// The parts of a `map`, `filter`, `fold`, or `find` call that inline as a loop.
    fn slice_loop_shape<'a>(
        &self,
        ctx: &NativeCallContext<'_>,
        receiver: &Expression,
        args: &'a [Expression],
    ) -> Option<SliceLoopShape<'a>> {
        let kind = SliceLoop::from_method(ctx.method)?;
        if args.len() != kind.argument_count() || ctx.spread.is_some() {
            return None;
        }
        let Expression::Lambda { params, body, .. } = args.last()?.unwrap_parens() else {
            return None;
        };
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
        let result_ty = ctx.call_ty?.clone();
        let element_ty = self
            .facts
            .strip_and_peel(&receiver.get_type())
            .get_type_params()?
            .first()?
            .clone();
        Some(SliceLoopShape {
            kind,
            body,
            patterns,
            result_ty,
            element_ty,
        })
    }

    /// `let xs = ys.map(..)` fills `xs` as the loop's own result.
    pub(crate) fn lower_slice_loop_into(
        &mut self,
        value: &Expression,
        go_name: &str,
    ) -> Option<Vec<Statement>> {
        let call = self.native_method_call(value)?;
        if !matches!(call.kind, NativeTypeKind::Slice) {
            return None;
        }
        let ty = value.get_type();
        let ctx = loop_context(&call, &ty);
        self.try_lower_slice_loop(
            &ctx,
            call.receiver,
            call.arguments,
            SliceLoopTarget::Into(go_name),
        )
        .map(|result| result.setup)
    }

    pub(crate) fn find_loop_fuses(
        &self,
        subject: &Expression,
        call: &NativeMethodCall<'_>,
    ) -> bool {
        if !matches!(call.kind, NativeTypeKind::Slice) || call.method != "find" {
            return false;
        }
        let ty = subject.get_type();
        self.slice_loop_shape(&loop_context(call, &ty), call.receiver, call.arguments)
            .is_some()
    }

    pub(crate) fn lower_find_loop(
        &mut self,
        subject: &Expression,
        call: &NativeMethodCall<'_>,
        sink: FoundSink<'_>,
    ) -> Vec<Statement> {
        let ty = subject.get_type();
        let ctx = loop_context(call, &ty);
        self.try_lower_slice_loop(
            &ctx,
            call.receiver,
            call.arguments,
            SliceLoopTarget::Found(sink),
        )
        .expect("find_loop_fuses accepted this call")
        .setup
    }

    /// Lower `xs.map(|x| ...)` and its siblings to the loop the prelude helper
    /// runs. `None` keeps the helper call.
    pub(super) fn try_lower_slice_loop(
        &mut self,
        ctx: &NativeCallContext,
        receiver: &Expression,
        args: &[Expression],
        target: SliceLoopTarget<'_>,
    ) -> Option<NativeCallResult> {
        let SliceLoopShape {
            kind,
            body,
            patterns,
            result_ty,
            element_ty,
        } = self.slice_loop_shape(ctx, receiver, args)?;
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
            stages.push(self.plan_operand(&args[0], ExpressionContext::value()));
        }
        let sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
        let effect = sequenced.effect;
        let mut setup = sequenced.setup;
        let mut values = sequenced.values.into_iter();
        let source = values.next().expect("the source is staged first");

        let planned = match target {
            SliceLoopTarget::Found(sink) => PlannedLoop::FindInto(sink),
            target => {
                let result = match target {
                    SliceLoopTarget::Into(name) => name.to_string(),
                    _ if kind == SliceLoop::Fold => self.accumulator_slot_name(patterns[0]),
                    _ => self.fresh_var(Some("result")),
                };
                // The result claims its name before the `map` index does.
                self.declare(&result);
                match kind {
                    SliceLoop::Map => {
                        let index = self.fresh_var(Some("i"));
                        self.declare(&index);
                        PlannedLoop::Map { result, index }
                    }
                    SliceLoop::Filter => PlannedLoop::Filter { result },
                    SliceLoop::Fold => PlannedLoop::Fold {
                        result,
                        accumulator: patterns[0],
                        init: values.next().expect("fold stages its initial value"),
                    },
                    SliceLoop::Find => PlannedLoop::Find { result },
                }
            }
        };
        self.push_slice_loop_declaration(&mut setup, &planned, &result_ty, &element_ty, &source);

        let lower_body = |planner: &mut Self| {
            planner.with_scope(|this| {
                if let PlannedLoop::Fold {
                    result,
                    accumulator,
                    ..
                } = &planned
                {
                    this.bind_loop_callback_param(accumulator, Some(result.clone()));
                }
                let element_name = this.element_loop_name(kind, patterns[patterns.len() - 1]);
                let statements = this.lower_slice_loop_body(&SliceLoopBody {
                    planned: &planned,
                    body,
                    result_ty: &result_ty,
                    element_ty: &element_ty,
                    element_name: &element_name,
                });
                (element_name, statements)
            })
        };
        let (element_name, body_statements) = match planned.result() {
            Some(result) => {
                self.with_assign_target(&GoExpression::name(result.to_string()), lower_body)
            }
            None => lower_body(self),
        };

        // Go rejects a range variable nothing reads.
        let header = LoopHeader::Range {
            key: match (&planned, element_name.as_str()) {
                (PlannedLoop::Map { index, .. }, _) => Some(index.clone().into()),
                (_, "_") => None,
                _ => Some("_".to_string().into()),
            },
            value: (element_name != "_").then_some(element_name.into()),
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

        let result = planned.result().map_or_else(GoExpression::empty, name);
        Some(NativeCallResult::new(
            setup,
            result,
            effect.combine(EvaluationEffect::EffectfulCall),
        ))
    }

    fn accumulator_slot_name(&mut self, pattern: &Pattern) -> String {
        let Pattern::Identifier { identifier, .. } = pattern else {
            return self.fresh_var(Some("result"));
        };
        match self.go_name_for_binding(pattern) {
            Some(name) => self.claim_declared_go_name(identifier, name),
            None => self.fresh_var(Some("result")),
        }
    }

    /// `filter` and `find` read the element back whatever the body does.
    fn element_loop_name(&mut self, kind: SliceLoop, pattern: &Pattern) -> String {
        let name = self.bind_loop_callback_param(pattern, None);
        if name != "_" || matches!(kind, SliceLoop::Map | SliceLoop::Fold) {
            return name;
        }
        let name = self.fresh_var(Some("v"));
        self.declare(&name);
        name
    }

    fn bind_loop_callback_param(&mut self, pattern: &Pattern, existing: Option<String>) -> String {
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
                self.declare(&name);
                self.scope
                    .bind_source(identifier.as_str(), ids.as_slice(), name.clone());
                name
            }
            None => match self.go_name_for_binding(pattern) {
                Some(name) => self
                    .claim_declared_binding(identifier, ids.as_slice(), name)
                    .to_string(),
                None => "_".to_string(),
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
                let element = self.first_type_argument_go_string(result_ty);
                define(
                    result.clone(),
                    GoExpression::call(
                        name("make"),
                        vec![
                            GoExpression::type_name(format!("[]{}", element)),
                            GoExpression::call(name("len"), vec![source.clone()]),
                        ],
                    ),
                )
            }
            PlannedLoop::Filter { result } => LoweredStatement::VarDecl {
                name: result.clone().into(),
                go_type: format!("[]{}", self.first_type_argument_go_string(result_ty)),
                value: None,
            }
            .into(),
            PlannedLoop::Fold { result, init, .. } => define(result.clone(), init.clone()),
            PlannedLoop::Find { result } => {
                let payload = self.first_type_argument_go_string(result_ty);
                define(
                    result.clone(),
                    prelude_call("MakeOptionNone", format!("[{}]", payload), Vec::new()),
                )
            }
            PlannedLoop::FindInto(sink) => {
                if let Some(value) = sink.value {
                    let go_type = self.use_go_type(element_ty);
                    setup.push(
                        LoweredStatement::VarDecl {
                            name: value.to_string().into(),
                            go_type,
                            value: None,
                        }
                        .into(),
                    );
                }
                define(
                    sink.flag.to_string(),
                    GoExpression::literal("false".to_string()),
                )
            }
        };
        setup.push(declaration);
    }

    fn lower_slice_loop_body(&mut self, loop_body: &SliceLoopBody<'_>) -> Vec<Statement> {
        let SliceLoopBody {
            planned,
            body,
            result_ty,
            element_ty,
            element_name,
        } = *loop_body;

        let then_body = match planned {
            // Map and fold store their body's value, so it lowers into the slot.
            PlannedLoop::Map { result, index } => {
                let element_ty = self
                    .first_type_argument(result_ty)
                    .expect("map returns a slice carrying its element type");
                let slot = GoExpression::index(name(result), name(index));
                return self.lower_body_into_slot(body, &slot, &element_ty);
            }
            PlannedLoop::Fold { result, .. } => {
                return self.lower_body_into_slot(body, &name(result), result_ty);
            }
            PlannedLoop::Filter { result } => vec![assign(
                name(result),
                GoExpression::call(name("append"), vec![name(result), name(element_name)]),
            )],
            PlannedLoop::Find { result } => {
                let payload = self.use_go_type(element_ty);
                vec![
                    assign(
                        name(result),
                        prelude_call(
                            "MakeOptionSome",
                            format!("[{}]", payload),
                            vec![name(element_name)],
                        ),
                    ),
                    LoweredStatement::Break(LoopTransfer::Unlabeled).into(),
                ]
            }
            PlannedLoop::FindInto(sink) => {
                let mut statements = Vec::new();
                if let Some(value) = sink.value {
                    statements.push(assign(name(value), name(element_name)));
                }
                statements.push(assign(
                    name(sink.flag),
                    GoExpression::literal("true".to_string()),
                ));
                statements.push(LoweredStatement::Break(LoopTransfer::Unlabeled).into());
                statements
            }
        };

        let plan = self.lower_value(body, ExpressionContext::value());
        let (mut statements, condition) = plan.into_parts();
        let then_body = LoweredBlock {
            statements: then_body,
        };
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

    fn first_type_argument_go_string(&mut self, ty: &Type) -> String {
        let argument = self
            .first_type_argument(ty)
            .expect("slice and option results carry a type argument");
        self.use_go_type(&argument)
    }
}
