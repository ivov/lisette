use syntax::ast::{Binding, Expression, Pattern};
use syntax::types::Type;

use super::native::NativeCallResult;
use super::{NativeCallContext, NativeMethodCall};
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, PlacePlan,
};
use crate::plan::calls::CallableOrigin;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression};
use crate::types::native::NativeGoType;

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
    kind: SliceLoop,
    body: &'a Expression,
    result_ty: &'a Type,
    element_ty: &'a Type,
    result: &'a str,
    element_name: &'a str,
    index: Option<&'a str>,
    found: Option<FoundSink<'a>>,
}

#[derive(Clone, Copy)]
pub(crate) struct FoundSink<'a> {
    pub value: Option<&'a str>,
    pub flag: &'a str,
}

fn loop_context<'a>(
    call: &NativeMethodCall<'a>,
    call_ty: &'a Type,
    result_name: Option<&'a str>,
) -> NativeCallContext<'a> {
    NativeCallContext {
        function: call.function,
        args: call.args,
        spread: call.spread,
        resolved_type_args: call.resolved_type_args,
        call_ty: Some(call_ty),
        native_type: &NativeGoType::Slice,
        method: call.method,
        capture_boundary: CaptureBoundary::SiblingSequence,
        retired_receiver: None,
        result_name,
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
    ) -> Option<Vec<LoweredStatement>> {
        let call = self.native_method_call(value)?;
        if !matches!(NativeGoType::from_kind(call.kind), NativeGoType::Slice) {
            return None;
        }
        let ty = value.get_type();
        let ctx = loop_context(&call, &ty, Some(go_name));
        self.slice_loop_shape(&ctx, call.receiver, call.arguments)?;
        Some(
            self.lower_native_call(&ctx, &CallableOrigin::NativeMethod(call.kind))
                .setup,
        )
    }

    pub(crate) fn find_loop_fuses(
        &self,
        subject: &Expression,
        call: &NativeMethodCall<'_>,
    ) -> bool {
        if !matches!(NativeGoType::from_kind(call.kind), NativeGoType::Slice)
            || call.method != "find"
        {
            return false;
        }
        let ty = subject.get_type();
        self.slice_loop_shape(
            &loop_context(call, &ty, None),
            call.receiver,
            call.arguments,
        )
        .is_some()
    }

    pub(crate) fn lower_find_loop(
        &mut self,
        subject: &Expression,
        call: &NativeMethodCall<'_>,
        sink: FoundSink<'_>,
    ) -> Vec<LoweredStatement> {
        let ty = subject.get_type();
        let ctx = loop_context(call, &ty, None);
        self.try_lower_slice_loop(&ctx, call.receiver, call.arguments, Some(sink))
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
        found: Option<FoundSink<'_>>,
    ) -> Option<NativeCallResult> {
        let SliceLoopShape {
            kind,
            body,
            patterns,
            result_ty,
            element_ty,
        } = self.slice_loop_shape(ctx, receiver, args)?;
        let expects_accumulator = kind == SliceLoop::Fold;

        let mut source_staged = self.plan_operand(receiver, ExpressionContext::value());
        // `map` reads the source twice, for its length and for the range.
        if kind == SliceLoop::Map && !self.plan_rests_in_stable_name(&source_staged) {
            self.pin_staged(&mut source_staged, "src");
        }
        let mut stages = vec![source_staged];
        if expects_accumulator {
            stages.push(self.plan_operand(&args[0], ExpressionContext::value()));
        }
        let sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
        let effect = sequenced.effect;
        let (mut setup, values) = sequenced.into_rendered();
        let source = values[0].clone();

        let result = match (found, ctx.result_name) {
            (Some(sink), _) => sink.value.unwrap_or_default().to_string(),
            (None, Some(name)) => name.to_string(),
            (None, None) => self.fresh_var(Some("result")),
        };
        match found {
            Some(sink) => {
                if let Some(value) = sink.value {
                    let go_type = self.use_go_type(&element_ty);
                    setup.push(LoweredStatement::VarDecl {
                        name: value.to_string(),
                        go_type,
                        value: None,
                    });
                }
                setup.push(LoweredStatement::TempBind {
                    name: sink.flag.to_string(),
                    value: "false".to_string(),
                });
            }
            None => {
                self.declare(&result);
                setup.push(self.slice_loop_declaration(
                    kind,
                    &result,
                    &result_ty,
                    &source,
                    values.get(1),
                ));
            }
        }

        let index = (kind == SliceLoop::Map).then(|| {
            let index = self.fresh_var(Some("i"));
            self.declare(&index);
            index
        });

        let (element_name, body_statements) = self.with_scope(|this| {
            if expects_accumulator {
                this.bind_loop_callback_param(patterns[0], Some(result.clone()));
            }
            let element_name = this.element_loop_name(kind, patterns[patterns.len() - 1]);
            let statements = this.lower_slice_loop_body(&SliceLoopBody {
                kind,
                body,
                result_ty: &result_ty,
                element_ty: &element_ty,
                result: &result,
                element_name: &element_name,
                index: index.as_deref(),
                found,
            });
            (element_name, statements)
        });

        // Go rejects a range variable nothing reads.
        let header = match (&index, element_name.as_str()) {
            (Some(index), "_") => index.clone(),
            (Some(index), element) => format!("{}, {}", index, element),
            (None, "_") => String::new(),
            (None, element) => format!("_, {}", element),
        };
        let header = if header.is_empty() {
            format!("for range {} {{\n", source)
        } else {
            format!("for {} := range {} {{\n", header, source)
        };
        setup.push(LoweredStatement::Loop(LoopPlan {
            prologue: Vec::new(),
            kind: LoopKind::Generated { label: None },
            header,
            body: LoweredBlock {
                statements: body_statements,
            },
        }));

        let result = if result.is_empty() {
            GoExpression::empty()
        } else {
            GoExpression::name(result)
        };
        Some(NativeCallResult::new(
            setup,
            result,
            effect.combine(EvaluationEffect::EffectfulCall),
            false,
        ))
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
        let Pattern::Identifier { identifier, .. } = pattern else {
            unreachable!("callback parameters are checked for identifier patterns");
        };
        let go_name = match existing {
            Some(name) => name,
            None => match self.go_name_for_binding(pattern) {
                Some(name) => {
                    let escaped = go_name::escape_reserved(&name).into_owned();
                    if self.shadows_declaration(&escaped) {
                        self.fresh_var(Some(&name))
                    } else {
                        escaped
                    }
                }
                None => "_".to_string(),
            },
        };
        if go_name != "_" {
            self.declare(&go_name);
            self.scope.bind(identifier.as_str(), go_name.clone());
        }
        go_name
    }

    fn slice_loop_declaration(
        &mut self,
        kind: SliceLoop,
        result: &str,
        result_ty: &Type,
        source: &str,
        init: Option<&String>,
    ) -> LoweredStatement {
        match kind {
            SliceLoop::Map => {
                let element = self.first_type_argument_go_string(result_ty);
                LoweredStatement::TempBind {
                    name: result.to_string(),
                    value: format!("make([]{}, len({}))", element, source),
                }
            }
            SliceLoop::Filter => LoweredStatement::RawGo(format!(
                "var {} []{}\n",
                result,
                self.first_type_argument_go_string(result_ty)
            )),
            SliceLoop::Fold => LoweredStatement::TempBind {
                name: result.to_string(),
                value: init.expect("fold stages its initial value").clone(),
            },
            SliceLoop::Find => {
                self.require_stdlib();
                let payload = self.first_type_argument_go_string(result_ty);
                LoweredStatement::TempBind {
                    name: result.to_string(),
                    value: format!("{}.MakeOptionNone[{}]()", go_name::GO_STDLIB_PKG, payload),
                }
            }
        }
    }

    fn lower_slice_loop_body(&mut self, loop_body: &SliceLoopBody<'_>) -> Vec<LoweredStatement> {
        let SliceLoopBody {
            kind,
            body,
            result_ty,
            element_ty,
            result,
            element_name,
            index,
            found,
        } = *loop_body;

        // Map and fold store their body's value, so it lowers into the slot.
        if let SliceLoop::Map | SliceLoop::Fold = kind {
            let (slot, target_ty) = match kind {
                SliceLoop::Map => (
                    format!("{}[{}]", result, index.expect("map binds a loop index")),
                    self.first_type_argument(result_ty)
                        .expect("map returns a slice carrying its element type"),
                ),
                _ => (result.to_string(), result_ty.clone()),
            };
            let place = PlacePlan::Assign {
                local: &slot,
                target_ty: Some(&target_ty),
            };
            return self.lower_block_to_place(body, &place).statements;
        }

        let plan = self.lower_value(body, ExpressionContext::value());
        let (mut statements, condition) = plan.into_parts();
        let then_body = match kind {
            SliceLoop::Filter => LoweredBlock {
                statements: vec![LoweredStatement::RawGo(format!(
                    "{} = append({}, {})\n",
                    result, result, element_name
                ))],
            },
            SliceLoop::Find => {
                let mut statements = Vec::new();
                match found {
                    Some(sink) => {
                        if let Some(value) = sink.value {
                            statements.push(LoweredStatement::RawGo(format!(
                                "{value} = {element_name}\n"
                            )));
                        }
                        statements.push(LoweredStatement::RawGo(format!("{} = true\n", sink.flag)));
                    }
                    None => {
                        self.require_stdlib();
                        let payload = self.use_go_type(element_ty);
                        statements.push(LoweredStatement::RawGo(format!(
                            "{} = {}.MakeOptionSome[{}]({})\n",
                            result,
                            go_name::GO_STDLIB_PKG,
                            payload,
                            element_name
                        )));
                    }
                }
                statements.push(LoweredStatement::RawGo("break\n".to_string()));
                LoweredBlock { statements }
            }
            SliceLoop::Map | SliceLoop::Fold => unreachable!("assign-place kinds returned above"),
        };
        statements.push(LoweredStatement::If(IfPlan {
            condition_setup: Vec::new(),
            condition,
            then_body,
            else_arm: ElseArm::None,
        }));
        statements
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
