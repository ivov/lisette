use syntax::ast::{Binding, Expression, Pattern};
use syntax::types::Type;

use super::NativeCallContext;
use super::native::NativeCallResult;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::{
    ElseArm, IfPlan, LoopKind, LoopPlan, LoweredBlock, LoweredStatement, PlacePlan,
};
use crate::plan::values::EvaluationEffect;

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
}

fn peel_single_expression_block(body: &Expression) -> &Expression {
    match body {
        Expression::Block { items, .. } if items.len() == 1 => {
            peel_single_expression_block(&items[0])
        }
        _ => body,
    }
}

impl Planner<'_> {
    /// Lower `xs.map(|x| ...)` and its siblings to the loop the prelude helper
    /// runs. `None` keeps the helper call.
    pub(super) fn try_lower_slice_loop(
        &mut self,
        ctx: &NativeCallContext,
        receiver: &Expression,
        args: &[Expression],
    ) -> Option<NativeCallResult> {
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

        let mut source_staged = self.stage_operand(receiver, ExpressionContext::value());
        // `map` reads the source twice, for its length and for the range.
        if kind == SliceLoop::Map && !self.plan_rests_in_stable_name(&source_staged) {
            self.pin_staged(&mut source_staged, "src");
        }
        let mut stages = vec![source_staged];
        if expects_accumulator {
            stages.push(self.stage_operand(&args[0], ExpressionContext::value()));
        }
        let sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
        let effect = sequenced.effect;
        let (mut setup, values) = sequenced.into_rendered();
        let source = values[0].clone();

        let result = self.fresh_var(Some("result"));
        self.declare(&result);
        setup.push(self.slice_loop_declaration(kind, &result, &result_ty, &source, values.get(1)));

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
                    if self.is_declared(&escaped) {
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
                self.require_stdlib();
                let payload = self.go_type_string(element_ty);
                LoweredBlock {
                    statements: vec![
                        LoweredStatement::RawGo(format!(
                            "{} = {}.MakeOptionSome[{}]({})\n",
                            result,
                            go_name::GO_STDLIB_PKG,
                            payload,
                            element_name
                        )),
                        LoweredStatement::RawGo("break\n".to_string()),
                    ],
                }
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
        self.go_type_string(&argument)
    }
}
