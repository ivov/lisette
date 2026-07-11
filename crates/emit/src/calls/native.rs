use super::NativeCallContext;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::expressions::access::index_access::range_var_bounds;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::plan_variadic_spread;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, ValuePlan};
use crate::types::native::NativeGoType;
use crate::utils::reads_mutable_operand;
use syntax::ast::{Expression, UnaryOperator};
use syntax::program::DotAccessKind;
use syntax::types::{CompoundKind, Type, peel_to_range_type};

pub(super) struct NativeCallResult {
    pub setup: Vec<LoweredStatement>,
    pub value: String,
    pub argument_effect: EvaluationEffect,
    pub arguments_contain_deferred_evaluation: bool,
}

impl NativeCallResult {
    fn new(
        setup: Vec<LoweredStatement>,
        value: String,
        argument_effect: EvaluationEffect,
        arguments_contain_deferred_evaluation: bool,
    ) -> Self {
        Self {
            setup,
            value,
            argument_effect,
            arguments_contain_deferred_evaluation,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum InlineImport {
    None,
    Slices,
    Strings,
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
            N::Array,
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

fn is_native_array_method(method: &str) -> bool {
    matches!(method, "to_slice" | "get")
}

pub(super) fn native_method_lowers_to_plain_call(
    native_type: &NativeGoType,
    method: &str,
    receiver_arity: usize,
) -> bool {
    if matches!(method, "substring" | "equals" | "clone") || is_native_array_method(method) {
        return true;
    }
    let Some(rule) = lookup_inline_rule(native_type, method, receiver_arity) else {
        return true;
    };
    matches!(
        rule.method,
        "delete" | "contains" | "split" | "starts_with" | "ends_with" | "rune_at" | "join" | "any"
    )
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
    planner: &Planner,
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    emitted_args: &[String],
    negated: bool,
) -> Option<String> {
    if let Some((inlined, import)) =
        try_inline_native_method(native_type, method, receiver, emitted_args, negated)
    {
        apply_inline_import(planner, import);
        return Some(inlined);
    }
    if let Some((static_receiver, remaining)) = emitted_args.split_first()
        && let Some((inlined, import)) =
            try_inline_native_method(native_type, method, static_receiver, remaining, negated)
    {
        apply_inline_import(planner, import);
        return Some(inlined);
    }
    None
}

/// Resolve the inline rule for an identifier-form call (args[0] is the receiver).
fn apply_inline_identifier_lookup(
    planner: &Planner,
    ctx: &NativeCallContext,
    emitted_args: &[String],
    negated: bool,
) -> Option<String> {
    let (receiver, remaining) = emitted_args.split_first()?;
    let (inlined, import) =
        try_inline_native_method(ctx.native_type, ctx.method, receiver, remaining, negated)?;
    apply_inline_import(planner, import);
    Some(inlined)
}

impl Planner<'_> {
    pub(super) fn lower_native_method_dot_access(
        &mut self,
        ctx: &NativeCallContext,
    ) -> NativeCallResult {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        if matches!(ctx.native_type, NativeGoType::String) && ctx.method == "substring" {
            return self.lower_string_substring(expression, ctx.args, ctx.capture_boundary);
        }

        if ctx.method == "equals"
            && matches!(ctx.native_type, NativeGoType::Slice | NativeGoType::Map)
        {
            let receiver_ty = self.facts.strip_and_peel(&expression.get_type());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let (setup, receiver, emitted_args, effect, contains_deferred_evaluation) =
                    self.stage_native_dot_access_call(ctx);
                let body = self.render_equality(&receiver, &emitted_args[0], &receiver_ty);
                return NativeCallResult::new(setup, body, effect, contains_deferred_evaluation);
            }
        }

        if matches!(ctx.native_type, NativeGoType::Array)
            && let Some(result) = self.lower_native_array_method(ctx, expression)
        {
            return result;
        }

        if ctx.method == "clone" {
            let receiver_ty = self.facts.strip_and_peel(&expression.get_type());
            if is_cloneable_container(&receiver_ty) {
                let (setup, receiver, _, effect, contains_deferred_evaluation) =
                    self.stage_native_dot_access_call(ctx);
                let body = self.render_clone(&receiver, &receiver_ty);
                return NativeCallResult::new(setup, body, effect, contains_deferred_evaluation);
            }
        }

        let (setup, receiver, emitted_args, effect, contains_deferred_evaluation) =
            self.stage_native_dot_access_call(ctx);

        if let Some(inlined) = apply_inline_lookup(
            self,
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            false,
        ) {
            return NativeCallResult::new(setup, inlined, effect, contains_deferred_evaluation);
        }

        let mut new_args = vec![receiver];
        new_args.extend(emitted_args);
        self.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = if !ctx.resolved_type_args.is_empty() && ctx.call_ty.is_some() {
            let receiver_ty = expression.get_type();
            self.format_type_args_with_receiver(&receiver_ty, ctx.resolved_type_args)
        } else {
            self.format_type_args(ctx.resolved_type_args)
        };
        NativeCallResult::new(
            setup,
            format!("{}{}({})", fn_name, type_args_string, new_args.join(", ")),
            effect,
            contains_deferred_evaluation,
        )
    }

    fn lower_native_array_method(
        &mut self,
        ctx: &NativeCallContext,
        expression: &Expression,
    ) -> Option<NativeCallResult> {
        if !is_native_array_method(ctx.method)
            || !matches!(
                self.facts.strip_and_peel(&expression.get_type()),
                Type::Array { .. }
            )
        {
            return None;
        }
        let (mut setup, receiver, emitted_args, effect, contains_deferred_evaluation) =
            self.stage_native_dot_access_call(ctx);
        let index = emitted_args.first();
        let body =
            self.lower_array_method_body(ctx.method, expression, receiver, index, &mut setup);
        Some(NativeCallResult::new(
            setup,
            body,
            effect,
            contains_deferred_evaluation,
        ))
    }

    fn lower_array_method_body(
        &mut self,
        method: &str,
        receiver_expr: &Expression,
        receiver: String,
        index: Option<&String>,
        setup: &mut Vec<LoweredStatement>,
    ) -> String {
        match method {
            "to_slice" => {
                self.require_slices();
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                format!("slices.Clone({view})")
            }
            "get" => {
                self.require_stdlib();
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                let pkg = go_name::GO_STDLIB_PKG;
                format!(
                    "{pkg}.SliceGet({view}, {})",
                    index.expect("get needs an index")
                )
            }
            other => unreachable!("not a native array method: {other}"),
        }
    }

    fn sliceable_receiver(
        &mut self,
        expression: &Expression,
        receiver: String,
        setup: &mut Vec<LoweredStatement>,
    ) -> String {
        let base = if self.receiver_is_addressable(expression) {
            if receiver.starts_with('*') {
                format!("({receiver})")
            } else {
                receiver
            }
        } else {
            self.hoist_tmp_value_statement(setup, "arr", &receiver)
        };
        format!("{base}[:]")
    }

    fn receiver_is_addressable(&self, expression: &Expression) -> bool {
        if expression.get_type().is_ref() {
            return true;
        }
        match expression.unwrap_parens() {
            Expression::Identifier { .. } => true,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            } => true,
            Expression::DotAccess {
                expression: base,
                dot_access_kind,
                ..
            } => {
                if matches!(
                    dot_access_kind,
                    Some(DotAccessKind::TupleStructField { is_newtype: true })
                ) {
                    return false;
                }
                let origin = base.unwrap_parens();
                let fresh_value = matches!(origin, Expression::StructCall { .. })
                    || (matches!(origin, Expression::Call { .. }) && !base.get_type().is_ref());
                !fresh_value && self.receiver_is_addressable(base)
            }
            Expression::IndexedAccess {
                expression: base, ..
            } => match self.facts.strip_and_peel(&base.get_type()).get_name() {
                Some("Map") => false,
                Some("Slice") => true,
                _ => self.receiver_is_addressable(base),
            },
            _ => false,
        }
    }

    /// Negated counterpart for dot-access native method calls. Returns
    /// `None` when the rule has no `negated_template`, so the unary-not
    /// caller can fall back to `!expr` without having staged anything.
    pub(super) fn try_emit_negated_native_method_dot_access(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        if !has_inline_negation(ctx.native_type, ctx.method, ctx.args.len()) {
            return None;
        }
        let (stage_setup, receiver, emitted_args, _, _) = self.stage_native_dot_access_call(ctx);
        setup.extend(stage_setup);
        apply_inline_lookup(
            self,
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            true,
        )
    }

    /// Pin the receiver stage to a temp when it reads a mutable operand,
    /// carries no setup of its own, and a later argument (or the spread)
    /// contains a call — so the receiver is captured before those args can
    /// mutate it. A receiver that is itself a call already evaluates eagerly.
    fn pin_receiver_if_mutated(
        &mut self,
        stage: &mut ValuePlan,
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
    ) -> (
        Vec<LoweredStatement>,
        String,
        Vec<String>,
        EvaluationEffect,
        bool,
    ) {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        let mut all_stages: Vec<ValuePlan> =
            Vec::with_capacity(1 + ctx.args.len() + ctx.spread.is_some() as usize);
        let mut receiver_stage = self.stage_operand(expression, ExpressionContext::value());
        let argument_stages = self.stage_native_method_args(ctx.function, ctx.args);
        let spread_stage = ctx
            .spread
            .map(|spread| self.stage_operand(spread, ExpressionContext::value()));
        let rest_has_call = argument_stages
            .iter()
            .chain(spread_stage.iter())
            .any(|stage| stage.evaluation.effect.has_call());
        self.pin_receiver_if_mutated(&mut receiver_stage, expression, rest_has_call);
        if expression.get_type().is_ref() {
            receiver_stage = receiver_stage.unary("*");
        }
        all_stages.push(receiver_stage);
        all_stages.extend(argument_stages);
        let spread_index = spread_stage.map(|stage| {
            all_stages.push(stage);
            all_stages.len() - 1
        });

        let combine = plan_variadic_spread(ctx.function, ctx.spread).map(|p| p.combine(1));
        let mut sequenced = self.sequence_values(all_stages, ctx.capture_boundary, "_arg");
        if let Some(spread_index) = spread_index {
            self.finalize_spread_stage(&mut sequenced.values, spread_index, false, combine);
        }
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, all_values) = sequenced.into_rendered();

        let receiver = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();
        (
            setup,
            receiver,
            emitted_args,
            effect,
            contains_deferred_evaluation,
        )
    }

    pub(super) fn lower_native_method_identifier(
        &mut self,
        ctx: &NativeCallContext,
    ) -> NativeCallResult {
        if matches!(ctx.native_type, NativeGoType::String)
            && ctx.method == "substring"
            && ctx.args.len() >= 2
        {
            return self.lower_string_substring(&ctx.args[0], &ctx.args[1..], ctx.capture_boundary);
        }

        if ctx.method == "equals"
            && matches!(ctx.native_type, NativeGoType::Slice | NativeGoType::Map)
            && let Some(receiver_expr) = ctx.args.first()
        {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expr.get_type());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let (setup, emitted_args, effect, contains_deferred_evaluation) =
                    self.stage_native_identifier_args(ctx);
                if emitted_args.len() >= 2 {
                    let body =
                        self.render_equality(&emitted_args[0], &emitted_args[1], &receiver_ty);
                    return NativeCallResult::new(
                        setup,
                        body,
                        effect,
                        contains_deferred_evaluation,
                    );
                }
            }
        }

        if ctx.method == "clone"
            && let Some(receiver_expr) = ctx.args.first()
        {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expr.get_type());
            if is_cloneable_container(&receiver_ty) {
                let (setup, emitted_args, effect, contains_deferred_evaluation) =
                    self.stage_native_identifier_args(ctx);
                if let Some(receiver) = emitted_args.first() {
                    let body = self.render_clone(receiver, &receiver_ty);
                    return NativeCallResult::new(
                        setup,
                        body,
                        effect,
                        contains_deferred_evaluation,
                    );
                }
            }
        }

        if is_native_array_method(ctx.method)
            && let Some(receiver_expr) = ctx.args.first()
            && matches!(
                self.facts.strip_and_peel(&receiver_expr.get_type()),
                Type::Array { .. }
            )
        {
            let (mut setup, emitted_args, effect, contains_deferred_evaluation) =
                self.stage_native_identifier_args(ctx);
            let receiver = emitted_args[0].clone();
            let index = emitted_args.get(1);
            let body = self.lower_array_method_body(
                ctx.method,
                receiver_expr,
                receiver,
                index,
                &mut setup,
            );
            return NativeCallResult::new(setup, body, effect, contains_deferred_evaluation);
        }

        let (setup, emitted_args, effect, contains_deferred_evaluation) =
            self.stage_native_identifier_args(ctx);

        if let Some(inlined) = apply_inline_identifier_lookup(self, ctx, &emitted_args, false) {
            return NativeCallResult::new(setup, inlined, effect, contains_deferred_evaluation);
        }

        self.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = self.format_type_args(ctx.resolved_type_args);
        NativeCallResult::new(
            setup,
            format!(
                "{}{}({})",
                fn_name,
                type_args_string,
                emitted_args.join(", ")
            ),
            effect,
            contains_deferred_evaluation,
        )
    }

    /// Negated counterpart for identifier-form native method calls.
    pub(super) fn try_emit_negated_native_method_identifier(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        let receiver_arity = ctx.args.len().saturating_sub(1);
        if !has_inline_negation(ctx.native_type, ctx.method, receiver_arity) {
            return None;
        }
        let (stage_setup, emitted_args, _, _) = self.stage_native_identifier_args(ctx);
        setup.extend(stage_setup);
        apply_inline_identifier_lookup(self, ctx, &emitted_args, true)
    }

    fn stage_native_identifier_args(
        &mut self,
        ctx: &NativeCallContext,
    ) -> (Vec<LoweredStatement>, Vec<String>, EvaluationEffect, bool) {
        let mut stages = self.stage_native_method_args(ctx.function, ctx.args);
        let spread_stage = ctx
            .spread
            .map(|spread| self.stage_operand(spread, ExpressionContext::value()));
        if let Some(receiver) = ctx.args.first() {
            let rest_has_call = stages[1..]
                .iter()
                .chain(spread_stage.iter())
                .any(|stage| stage.evaluation.effect.has_call());
            self.pin_receiver_if_mutated(&mut stages[0], receiver, rest_has_call);
        }
        let spread_index = spread_stage.map(|stage| {
            stages.push(stage);
            stages.len() - 1
        });
        let combine = plan_variadic_spread(ctx.function, ctx.spread).map(|p| p.combine(0));
        let mut sequenced = self.sequence_values(stages, ctx.capture_boundary, "_arg");
        if let Some(spread_index) = spread_index {
            self.finalize_spread_stage(&mut sequenced.values, spread_index, false, combine);
        }
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, values) = sequenced.into_rendered();
        (setup, values, effect, contains_deferred_evaluation)
    }

    fn lower_string_substring(
        &mut self,
        receiver_expr: &Expression,
        args: &[Expression],
        capture_boundary: CaptureBoundary,
    ) -> NativeCallResult {
        self.require_stdlib();
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
            let mut stages = vec![self.stage_operand(receiver_expr, ExpressionContext::value())];
            if let Some(s) = start.as_deref() {
                stages.push(self.stage_operand(s, ExpressionContext::value()));
            }
            if let Some(e) = end.as_deref() {
                stages.push(self.stage_operand(e, ExpressionContext::value()));
            }
            let sequenced = self.sequence_values(stages, capture_boundary, "_arg");
            let effect = sequenced.effect;
            let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
            let (setup, values) = sequenced.into_rendered();
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
            return NativeCallResult::new(
                setup,
                format_substring_call(
                    &deref(&values[0]),
                    start_bound.as_deref(),
                    end_bound.as_deref(),
                ),
                effect,
                contains_deferred_evaluation,
            );
        }

        let arg_ty = arg.get_type();
        let range_kind = peel_to_range_type(&arg_ty)
            .and_then(|t| t.get_name())
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.stage_operand(receiver_expr, ExpressionContext::value());
        let range_staged = self.stage_or_capture(arg, "range");
        let sequenced = self.sequence_values(
            vec![receiver_staged, range_staged],
            capture_boundary,
            "_arg",
        );
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, values) = sequenced.into_rendered();
        let (start, end) = range_var_bounds(&values[1], range_kind);
        NativeCallResult::new(
            setup,
            format_substring_call(&deref(&values[0]), start.as_deref(), end.as_deref()),
            effect,
            contains_deferred_evaluation,
        )
    }

    pub(crate) fn render_equality(&mut self, lhs: &str, rhs: &str, ty: &Type) -> String {
        let peeled = self.facts.peel_alias(ty);
        if peeled.is_ref() {
            return format!("{lhs} == {rhs}");
        }
        if peeled.is_slice() {
            self.require_slices();
            return match peeled.inner() {
                Some(elem) if self.needs_custom_equality(&elem) => {
                    let eq = self.equality_closure(&elem);
                    format!("slices.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("slices.Equal({lhs}, {rhs})"),
            };
        }
        if peeled.is_map() {
            self.require_maps();
            let value = peeled
                .as_compound()
                .and_then(|(_, args)| args.get(1).cloned());
            return match value {
                Some(value) if self.needs_custom_equality(&value) => {
                    let eq = self.equality_closure(&value);
                    format!("maps.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("maps.Equal({lhs}, {rhs})"),
            };
        }
        if self.type_has_equals(&peeled) {
            return format!("{lhs}.{}({rhs})", self.equals_method_go_name());
        }
        format!("{lhs} == {rhs}")
    }

    fn equality_closure(&mut self, ty: &Type) -> String {
        let go_ty = self.go_type_string(ty);
        let a = self.fresh_var(Some("a"));
        let b = self.fresh_var(Some("b"));
        let body = self.render_equality(&a, &b, ty);
        format!("func({a} {go_ty}, {b} {go_ty}) bool {{ return {body} }}")
    }

    fn needs_custom_equality(&self, ty: &Type) -> bool {
        self.is_container(ty) || self.type_has_equals(ty)
    }

    fn is_container(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        peeled.is_slice() || peeled.is_map()
    }
}

fn is_cloneable_container(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Compound {
            kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
            ..
        }
    )
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

pub(super) fn apply_inline_import(planner: &Planner, import: InlineImport) {
    match import {
        InlineImport::Slices => planner.require_slices(),
        InlineImport::Strings => planner.require_strings(),
        InlineImport::Stdlib => planner.require_stdlib(),
        InlineImport::None => {}
    }
}
