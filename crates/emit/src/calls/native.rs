use super::NativeCallContext;
use crate::EmitEffects;
use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::expressions::access::index_access::range_var_bounds;
use crate::expressions::emission::StagedExpression;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::plan_variadic_spread;
use crate::types::native::NativeGoType;
use crate::utils::{contains_call, reads_mutable_operand};
use syntax::ast::Expression;
use syntax::types::{Type, peel_to_range_type};

#[derive(Clone, Copy)]
pub(super) enum InlineImport {
    None,
    Slices,
    Strings,
    Maps,
    Stdlib,
}

struct InlineRule {
    types: &'static [NativeGoType],
    method: &'static str,
    arity: i8,
    template: &'static str,
    /// Direct Go form of the negated method. Set when the positive template
    /// emits a comparison, so `!method(...)` can flip the operator instead
    /// of prepending `!` (Go's `!` binds tighter than `==`).
    negated_template: Option<&'static str>,
    import: InlineImport,
}

type N = NativeGoType;

static INLINE_METHODS: &[InlineRule] = &[
    // No-arg methods
    InlineRule {
        types: &[
            N::Slice,
            N::Map,
            N::Channel,
            N::Sender,
            N::Receiver,
            N::String,
        ],
        method: "length",
        arity: 0,
        template: "len({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice, N::Channel, N::Sender, N::Receiver],
        method: "capacity",
        arity: 0,
        template: "cap({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[
            N::Slice,
            N::Map,
            N::Channel,
            N::Sender,
            N::Receiver,
            N::String,
        ],
        method: "is_empty",
        arity: 0,
        template: "len({r}) == 0",
        negated_template: Some("len({r}) != 0"),
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "enumerate",
        arity: 0,
        template: "{r}",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "clone",
        arity: 0,
        template: "slices.Clone({r})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::Map],
        method: "clone",
        arity: 0,
        template: "maps.Clone({r})",
        negated_template: None,
        import: InlineImport::Maps,
    },
    InlineRule {
        types: &[N::String],
        method: "bytes",
        arity: 0,
        template: "[]byte({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "runes",
        arity: 0,
        template: "[]rune({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    // Single-arg methods
    InlineRule {
        types: &[N::Map],
        method: "delete",
        arity: 1,
        template: "delete({r}, {0})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "extend",
        arity: 1,
        template: "append({r}, {0}...)",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "copy_from",
        arity: 1,
        template: "copy({r}, {0})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "contains",
        arity: 1,
        template: "slices.Contains({r}, {0})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::String],
        method: "contains",
        arity: 1,
        template: "strings.Contains({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "split",
        arity: 1,
        template: "strings.Split({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "starts_with",
        arity: 1,
        template: "strings.HasPrefix({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "ends_with",
        arity: 1,
        template: "strings.HasSuffix({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "byte_at",
        arity: 1,
        template: "{r}[{0}]",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "rune_at",
        arity: 1,
        template: "lisette.RuneAt({r}, {0})",
        negated_template: None,
        import: InlineImport::Stdlib,
    },
    InlineRule {
        types: &[N::Slice],
        method: "join",
        arity: 1,
        template: "strings.Join({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::Slice],
        method: "any",
        arity: 1,
        template: "slices.ContainsFunc({r}, {0})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    // Variadic methods
    InlineRule {
        types: &[N::Slice],
        method: "append",
        arity: -1,
        template: "append({r+args})",
        negated_template: None,
        import: InlineImport::None,
    },
];

fn render_inline(template: &str, receiver: &str, args: &[String]) -> String {
    let mut result = template.replace("{r}", receiver);
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), arg);
    }
    if result.contains("{args}") {
        result = result.replace("{args}", &args.join(", "));
    }
    if result.contains("{r+args}") {
        let all = std::iter::once(receiver.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(", ");
        result = result.replace("{r+args}", &all);
    }
    result
}

fn lookup_inline_rule(
    native_type: &NativeGoType,
    method: &str,
    arity: usize,
) -> Option<&'static InlineRule> {
    INLINE_METHODS.iter().find(|s| {
        s.method == method
            && s.types.contains(native_type)
            && (s.arity < 0 || s.arity as usize == arity)
    })
}

/// Try to inline a native-type method call. `negated` picks the rule's
/// `negated_template` (returning `None` when the rule lacks one).
pub(super) fn try_inline_native_method(
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    args: &[String],
    negated: bool,
) -> Option<(String, InlineImport)> {
    // Go's `append` requires at least 2 args, so zero-arg `append` returns
    // the receiver unchanged.
    if !negated && method == "append" && args.is_empty() {
        return Some((receiver.to_string(), InlineImport::None));
    }
    let rule = lookup_inline_rule(native_type, method, args.len())?;
    let template = if negated {
        rule.negated_template?
    } else {
        rule.template
    };
    Some((render_inline(template, receiver, args), rule.import))
}

/// Whether a rule for `(type, method, arity)` defines a negated template.
pub(super) fn has_inline_negation(native_type: &NativeGoType, method: &str, arity: usize) -> bool {
    lookup_inline_rule(native_type, method, arity)
        .and_then(|r| r.negated_template)
        .is_some()
}

/// Resolve the inline rule for a dot-access form, applying the static-receiver
/// fallback when the standard receiver shape does not match.
fn apply_inline_lookup(
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    emitted_args: &[String],
    negated: bool,
    fx: &mut EmitEffects,
) -> Option<String> {
    if let Some((inlined, import)) =
        try_inline_native_method(native_type, method, receiver, emitted_args, negated)
    {
        apply_inline_import(import, fx);
        return Some(inlined);
    }
    if let Some((static_receiver, remaining)) = emitted_args.split_first()
        && let Some((inlined, import)) =
            try_inline_native_method(native_type, method, static_receiver, remaining, negated)
    {
        apply_inline_import(import, fx);
        return Some(inlined);
    }
    None
}

/// Resolve the inline rule for an identifier-form call (args[0] is the receiver).
fn apply_inline_identifier_lookup(
    ctx: &NativeCallContext,
    emitted_args: &[String],
    negated: bool,
    fx: &mut EmitEffects,
) -> Option<String> {
    let (receiver, remaining) = emitted_args.split_first()?;
    let (inlined, import) =
        try_inline_native_method(ctx.native_type, ctx.method, receiver, remaining, negated)?;
    apply_inline_import(import, fx);
    Some(inlined)
}

impl Planner<'_> {
    pub(super) fn lower_native_method_dot_access(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        if matches!(ctx.native_type, NativeGoType::String) && ctx.method == "substring" {
            return self.lower_string_substring(expression, ctx.args, fx);
        }

        if ctx.method == "equals"
            && matches!(ctx.native_type, NativeGoType::Slice | NativeGoType::Map)
        {
            let receiver_ty = self.facts.peel_alias(&expression.get_type().strip_refs());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let (setup, receiver, emitted_args) = self.stage_native_dot_access_call(ctx, fx);
                let body =
                    self.render_container_equality(&receiver, &emitted_args[0], &receiver_ty, fx);
                return (setup, body);
            }
        }

        let (setup, receiver, emitted_args) = self.stage_native_dot_access_call(ctx, fx);

        if let Some(inlined) = apply_inline_lookup(
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            false,
            fx,
        ) {
            return (setup, inlined);
        }

        let mut new_args = vec![receiver];
        new_args.extend(emitted_args);
        fx.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = if !ctx.type_args.is_empty() && ctx.call_ty.is_some() {
            let receiver_ty = expression.get_type();
            self.format_type_args_with_receiver(&receiver_ty, ctx.type_args, fx)
        } else {
            self.format_type_args_from_annotations(ctx.type_args, fx)
        };
        (
            setup,
            format!("{}{}({})", fn_name, type_args_string, new_args.join(", ")),
        )
    }

    /// Negated counterpart for dot-access native method calls. Returns
    /// `None` when the rule has no `negated_template`, so the unary-not
    /// caller can fall back to `!expr` without having staged anything.
    pub(super) fn try_emit_negated_native_method_dot_access(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if !has_inline_negation(ctx.native_type, ctx.method, ctx.args.len()) {
            return None;
        }
        let (setup, receiver, emitted_args) = self.stage_native_dot_access_call(ctx, fx);
        output.push_str(&Renderer.render_setup(&setup));
        apply_inline_lookup(
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            true,
            fx,
        )
    }

    /// Pin the receiver stage to a temp when it reads a mutable operand,
    /// carries no setup of its own, and a later argument (or the spread)
    /// contains a call — so the receiver is captured before those args can
    /// mutate it. A receiver that is itself a call already evaluates eagerly.
    fn pin_receiver_if_mutated(
        &mut self,
        stage: &mut StagedExpression,
        receiver: &Expression,
        rest_has_call: bool,
    ) {
        if !matches!(receiver.unwrap_parens(), Expression::Call { .. })
            && reads_mutable_operand(receiver)
            && stage.setup.is_empty()
            && rest_has_call
        {
            self.pin_staged(stage, "recv");
        }
    }

    fn stage_native_dot_access_call(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String, Vec<String>) {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        let mut all_stages: Vec<StagedExpression> =
            Vec::with_capacity(1 + ctx.args.len() + ctx.spread.is_some() as usize);
        let mut receiver_stage = self.stage_operand(expression, ExpressionContext::value(), fx);
        let rest_has_call =
            ctx.args.iter().any(contains_call) || ctx.spread.is_some_and(contains_call);
        self.pin_receiver_if_mutated(&mut receiver_stage, expression, rest_has_call);
        if expression.get_type().is_ref() {
            receiver_stage.value = format!("*{}", receiver_stage.value);
        }
        all_stages.push(receiver_stage);
        all_stages.extend(self.stage_native_method_args(ctx.function, ctx.args, fx));

        let combine = plan_variadic_spread(ctx.function, ctx.spread).map(|p| p.combine(1));
        let (setup, all_values) = self
            .sequence_with_spread_structured(all_stages, ctx.spread, false, "_arg", combine, fx);

        let receiver = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();
        (setup, receiver, emitted_args)
    }

    pub(super) fn lower_native_method_identifier(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        if matches!(ctx.native_type, NativeGoType::String)
            && ctx.method == "substring"
            && ctx.args.len() >= 2
        {
            return self.lower_string_substring(&ctx.args[0], &ctx.args[1..], fx);
        }

        if ctx.method == "equals"
            && matches!(ctx.native_type, NativeGoType::Slice | NativeGoType::Map)
            && let Some(receiver_expr) = ctx.args.first()
        {
            let receiver_ty = self
                .facts
                .peel_alias(&receiver_expr.get_type().strip_refs());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let (setup, emitted_args) = self.stage_native_identifier_args(ctx, fx);
                if emitted_args.len() >= 2 {
                    let body = self.render_container_equality(
                        &emitted_args[0],
                        &emitted_args[1],
                        &receiver_ty,
                        fx,
                    );
                    return (setup, body);
                }
            }
        }

        let (setup, emitted_args) = self.stage_native_identifier_args(ctx, fx);

        if let Some(inlined) = apply_inline_identifier_lookup(ctx, &emitted_args, false, fx) {
            return (setup, inlined);
        }

        fx.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = self.format_type_args_from_annotations(ctx.type_args, fx);
        (
            setup,
            format!(
                "{}{}({})",
                fn_name,
                type_args_string,
                emitted_args.join(", ")
            ),
        )
    }

    /// Negated counterpart for identifier-form native method calls.
    pub(super) fn try_emit_negated_native_method_identifier(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let receiver_arity = ctx.args.len().saturating_sub(1);
        if !has_inline_negation(ctx.native_type, ctx.method, receiver_arity) {
            return None;
        }
        let (setup, emitted_args) = self.stage_native_identifier_args(ctx, fx);
        output.push_str(&Renderer.render_setup(&setup));
        apply_inline_identifier_lookup(ctx, &emitted_args, true, fx)
    }

    fn stage_native_identifier_args(
        &mut self,
        ctx: &NativeCallContext,
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, Vec<String>) {
        let mut stages = self.stage_native_method_args(ctx.function, ctx.args, fx);
        if let Some(receiver) = ctx.args.first() {
            let rest_has_call =
                ctx.args[1..].iter().any(contains_call) || ctx.spread.is_some_and(contains_call);
            self.pin_receiver_if_mutated(&mut stages[0], receiver, rest_has_call);
        }
        let combine = plan_variadic_spread(ctx.function, ctx.spread).map(|p| p.combine(0));
        self.sequence_with_spread_structured(stages, ctx.spread, false, "_arg", combine, fx)
    }

    fn lower_string_substring(
        &mut self,
        receiver_expr: &Expression,
        args: &[Expression],
        fx: &mut EmitEffects,
    ) -> (Vec<LoweredStatement>, String) {
        fx.require_stdlib();
        let arg = &args[0];
        let is_ref_receiver = receiver_expr.get_type().is_ref();
        let deref = |raw: &str| -> String {
            if is_ref_receiver {
                format!("*{}", raw)
            } else {
                raw.to_string()
            }
        };

        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = arg
        {
            let mut stages =
                vec![self.stage_operand(receiver_expr, ExpressionContext::value(), fx)];
            if let Some(s) = start.as_deref() {
                stages.push(self.stage_operand(s, ExpressionContext::value(), fx));
            }
            if let Some(e) = end.as_deref() {
                stages.push(self.stage_operand(e, ExpressionContext::value(), fx));
            }
            let (setup, values) = self.sequence_structured(stages, "_arg");
            let mut bounds = values.iter().skip(1);
            let start_bound = start.is_some().then(|| bounds.next().unwrap().clone());
            let end_bound = end.is_some().then(|| {
                let e = bounds.next().unwrap();
                if *inclusive {
                    format!("{}+1", e)
                } else {
                    e.clone()
                }
            });
            return (
                setup,
                format_substring_call(
                    &deref(&values[0]),
                    start_bound.as_deref(),
                    end_bound.as_deref(),
                ),
            );
        }

        let arg_ty = arg.get_type();
        let range_kind = peel_to_range_type(&arg_ty)
            .and_then(|t| t.get_name())
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.stage_operand(receiver_expr, ExpressionContext::value(), fx);
        let range_staged = self.stage_or_capture(arg, "range", fx);
        let (setup, values) = self.sequence_structured(vec![receiver_staged, range_staged], "_arg");
        let (start, end) = range_var_bounds(&values[1], range_kind);
        (
            setup,
            format_substring_call(&deref(&values[0]), start.as_deref(), end.as_deref()),
        )
    }

    fn render_container_equality(
        &mut self,
        lhs: &str,
        rhs: &str,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> String {
        let peeled = self.facts.peel_alias(&ty.strip_refs());
        if peeled.is_slice() {
            fx.require_slices();
            return match peeled.inner() {
                Some(elem) if self.is_container(&elem) => {
                    let eq = self.container_equality_closure(&elem, fx);
                    format!("slices.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("slices.Equal({lhs}, {rhs})"),
            };
        }
        if peeled.is_map() {
            fx.require_maps();
            let value = peeled
                .as_compound()
                .and_then(|(_, args)| args.get(1).cloned());
            return match value {
                Some(value) if self.is_container(&value) => {
                    let eq = self.container_equality_closure(&value, fx);
                    format!("maps.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("maps.Equal({lhs}, {rhs})"),
            };
        }
        unreachable!("`.equals()` gate guarantees a slice or map receiver")
    }

    fn container_equality_closure(&mut self, ty: &Type, fx: &mut EmitEffects) -> String {
        let go_ty = self.go_type_string(ty, fx);
        let a = self.fresh_var(Some("a"));
        let b = self.fresh_var(Some("b"));
        let body = self.render_container_equality(&a, &b, ty, fx);
        format!("func({a} {go_ty}, {b} {go_ty}) bool {{ return {body} }}")
    }

    fn is_container(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(&ty.strip_refs());
        peeled.is_slice() || peeled.is_map()
    }
}

fn format_substring_call(receiver: &str, start: Option<&str>, end: Option<&str>) -> String {
    let pkg = go_name::GO_STDLIB_PKG;
    match (start, end) {
        (Some(s), Some(e)) => format!("{}.Substring({}, {}, {})", pkg, receiver, s, e),
        (Some(s), None) => format!("{}.SubstringFrom({}, {})", pkg, receiver, s),
        (None, Some(e)) => format!("{}.SubstringTo({}, {})", pkg, receiver, e),
        (None, None) => unreachable!("`s.substring(..)` is rejected upstream"),
    }
}

pub(super) fn apply_inline_import(import: InlineImport, fx: &mut EmitEffects) {
    match import {
        InlineImport::Slices => fx.require_slices(),
        InlineImport::Strings => fx.require_strings(),
        InlineImport::Maps => fx.require_maps(),
        InlineImport::Stdlib => fx.require_stdlib(),
        InlineImport::None => {}
    }
}
