use super::NativeCallContext;
use crate::Planner;
use crate::calls::dispatch::extract_native_method_name;
use crate::context::expression::ExpressionContext;
use crate::expressions::access::index_access::range_var_bounds;
use crate::names::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::calls::plan_variadic_spread;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, ValuePlan};
use crate::statements::assignments::lvalues_match;
use crate::types::native::NativeGoType;
use crate::utils::reads_mutable_operand;
use std::iter;
use syntax::ast::{Expression, Generic, Literal, UnaryOperator};
use syntax::program::{CallKind, DotAccessKind, NativeTypeKind};
use syntax::types::{CompoundKind, Type, peel_to_range_type};

pub(super) struct NativeCallResult {
    pub setup: Vec<LoweredStatement>,
    pub value: String,
    pub argument_effect: EvaluationEffect,
    pub arguments_contain_deferred_evaluation: bool,
}

impl NativeCallResult {
    pub(super) fn new(
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
    arity: RuleArity,
    template: &'static str,
    /// Direct Go form of the negated method. Set when the positive template
    /// emits a comparison, so `!method(...)` can flip the operator instead
    /// of prepending `!` (Go's `!` binds tighter than `==`).
    negated_template: Option<&'static str>,
    import: InlineImport,
}

#[derive(Clone, Copy)]
enum RuleArity {
    Exact(usize),
    Variadic,
}

impl RuleArity {
    fn accepts(self, actual: usize) -> bool {
        match self {
            Self::Exact(expected) => actual == expected,
            Self::Variadic => true,
        }
    }
}

impl InlineRule {
    fn matches(&self, native_type: NativeGoType, method: &str, arity: usize) -> bool {
        self.method == method && self.types.contains(&native_type) && self.arity.accepts(arity)
    }
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
        arity: RuleArity::Exact(0),
        template: "len({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice, N::Channel, N::Sender, N::Receiver],
        method: "capacity",
        arity: RuleArity::Exact(0),
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
        arity: RuleArity::Exact(0),
        template: "len({r}) == 0",
        negated_template: Some("len({r}) != 0"),
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "enumerate",
        arity: RuleArity::Exact(0),
        template: "{r}",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "bytes",
        arity: RuleArity::Exact(0),
        template: "[]byte({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "runes",
        arity: RuleArity::Exact(0),
        template: "[]rune({r})",
        negated_template: None,
        import: InlineImport::None,
    },
    // Single-arg methods
    InlineRule {
        types: &[N::Map],
        method: "delete",
        arity: RuleArity::Exact(1),
        template: "delete({r}, {0})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "copy_from",
        arity: RuleArity::Exact(1),
        template: "copy({r}, {0})",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "contains",
        arity: RuleArity::Exact(1),
        template: "slices.Contains({r}, {0})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::String],
        method: "contains",
        arity: RuleArity::Exact(1),
        template: "strings.Contains({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "split",
        arity: RuleArity::Exact(1),
        template: "strings.Split({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "starts_with",
        arity: RuleArity::Exact(1),
        template: "strings.HasPrefix({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "ends_with",
        arity: RuleArity::Exact(1),
        template: "strings.HasSuffix({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "byte_at",
        arity: RuleArity::Exact(1),
        template: "{r}[{0}]",
        negated_template: None,
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "rune_at",
        arity: RuleArity::Exact(1),
        template: "lisette.RuneAt({r}, {0})",
        negated_template: None,
        import: InlineImport::Stdlib,
    },
    InlineRule {
        types: &[N::Slice],
        method: "join",
        arity: RuleArity::Exact(1),
        template: "strings.Join({r}, {0})",
        negated_template: None,
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::Slice],
        method: "any",
        arity: RuleArity::Exact(1),
        template: "slices.ContainsFunc({r}, {0})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::Slice],
        method: "reserve",
        arity: RuleArity::Exact(1),
        template: "slices.Grow({r}, {0})",
        negated_template: None,
        import: InlineImport::Slices,
    },
    // Variadic methods
    InlineRule {
        types: &[N::Slice],
        method: "append",
        arity: RuleArity::Variadic,
        template: "append({r+args})",
        negated_template: None,
        import: InlineImport::None,
    },
];

pub(crate) fn clip_shared_capacity(receiver: &str) -> String {
    format!("{r}[:len({r}):len({r})]", r = receiver)
}

fn grows_into_capacity(method: &str, appends_anything: bool) -> bool {
    method == "reserve" || (method == "append" && appends_anything)
}

pub(crate) fn is_clip_safe_path(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn growth_clip_applies(ctx: &NativeCallContext, receiver: &Expression) -> bool {
    let appends_anything = if matches!(ctx.function, Expression::DotAccess { .. }) {
        !ctx.args.is_empty() || ctx.spread.is_some()
    } else {
        ctx.args.len() > 1 || ctx.spread.is_some()
    };
    grows_into_capacity(ctx.method, appends_anything)
        && !is_fresh_slice_value(receiver)
        && !ctx
            .retired_receiver
            .is_some_and(|target| retired_covers_receiver(target, receiver))
}

fn is_fresh_slice_value(receiver: &Expression) -> bool {
    match receiver.unwrap_parens() {
        Expression::Literal {
            literal: Literal::Slice(_),
            ..
        } => true,
        Expression::Call {
            expression: callee,
            args,
            spread,
            call_kind,
            ..
        } => match call_kind {
            CallKind::NativeConstructor(NativeTypeKind::Slice) => true,
            CallKind::NativeMethod(NativeTypeKind::Slice)
            | CallKind::NativeMethodIdentifier(NativeTypeKind::Slice) => {
                match extract_native_method_name(callee.unwrap_parens()) {
                    "append" => {
                        let receiver_argument_count =
                            matches!(call_kind, CallKind::NativeMethodIdentifier(_)) as usize;
                        args.len() > receiver_argument_count || spread.is_some()
                    }
                    "reserve" | "clone" | "filter" | "map" => true,
                    _ => false,
                }
            }
            _ => false,
        },
        _ => false,
    }
}

fn retired_covers_receiver(target: &Expression, receiver: &Expression) -> bool {
    let mut current = receiver.unwrap_parens();
    loop {
        if lvalues_match(target, current) {
            return true;
        }
        match current {
            Expression::DotAccess { expression, .. } => current = expression.unwrap_parens(),
            _ => return false,
        }
    }
}

fn render_inline(template: &str, receiver: &str, args: &[String]) -> String {
    let mut result = template.replace("{r}", receiver);
    for (i, arg) in args.iter().enumerate() {
        result = result.replace(&format!("{{{}}}", i), arg);
    }
    if result.contains("{args}") {
        result = result.replace("{args}", &args.join(", "));
    }
    if result.contains("{r+args}") {
        let all = iter::once(receiver.to_string())
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
    INLINE_METHODS
        .iter()
        .find(|rule| rule.matches(*native_type, method, arity))
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
fn has_inline_negation(native_type: &NativeGoType, method: &str, arity: usize) -> bool {
    lookup_inline_rule(native_type, method, arity)
        .and_then(|r| r.negated_template)
        .is_some()
}

/// Resolve the inline rule for a dot-access form, applying the static-receiver
/// fallback when the standard receiver shape does not match.
fn apply_inline_lookup(
    planner: &mut Planner,
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

#[derive(Clone, Copy)]
enum NativeMethodForm {
    Dot,
    Identifier,
}

struct StagedNativeMethod {
    setup: Vec<LoweredStatement>,
    receiver: String,
    arguments: Vec<String>,
    effect: EvaluationEffect,
    contains_deferred_evaluation: bool,
}

impl StagedNativeMethod {
    fn finish(self, value: String) -> NativeCallResult {
        NativeCallResult::new(
            self.setup,
            value,
            self.effect,
            self.contains_deferred_evaluation,
        )
    }
}

impl Planner<'_> {
    pub(super) fn lower_native_method(&mut self, ctx: &NativeCallContext) -> NativeCallResult {
        let (form, receiver_expression, arguments) = match ctx.function {
            Expression::DotAccess { expression, .. } => {
                (NativeMethodForm::Dot, expression.as_ref(), ctx.args)
            }
            _ => {
                let (receiver, arguments) = ctx
                    .args
                    .split_first()
                    .expect("native method identifier has a receiver argument");
                (NativeMethodForm::Identifier, receiver, arguments)
            }
        };

        if matches!(ctx.native_type, NativeGoType::String)
            && ctx.method == "substring"
            && !arguments.is_empty()
        {
            return self.lower_string_substring(
                receiver_expression,
                arguments,
                ctx.capture_boundary,
            );
        }

        if ctx.method == "equals"
            && matches!(ctx.native_type, NativeGoType::Slice | NativeGoType::Map)
        {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if receiver_ty.is_slice() || receiver_ty.is_map() {
                let staged = self.stage_native_method(ctx, form);
                let body =
                    self.render_equality(&staged.receiver, &staged.arguments[0], &receiver_ty, &[]);
                return staged.finish(body);
            }
        }

        if ctx.method == "contains" && matches!(ctx.native_type, NativeGoType::Slice) {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if receiver_ty.is_slice()
                && let Some(element) = receiver_ty.inner()
                && self.needs_custom_equality(&element, &[])
            {
                let mut staged = self.stage_native_method(ctx, form);
                self.require_slices();
                let searched = staged.arguments[0].clone();
                let target = self.hoist_tmp_value_statement(&mut staged.setup, "want", &searched);
                let predicate = self.contains_predicate(&element, &target, &[]);
                let body = format!("slices.ContainsFunc({}, {predicate})", staged.receiver);
                return staged.finish(body);
            }
        }

        if matches!(ctx.native_type, NativeGoType::Array)
            && is_native_array_method(ctx.method)
            && matches!(
                self.facts.strip_and_peel(&receiver_expression.get_type()),
                Type::Array { .. }
            )
        {
            let mut staged = self.stage_native_method(ctx, form);
            let index = staged.arguments.first();
            let body = self.lower_array_method_body(
                ctx.method,
                receiver_expression,
                staged.receiver.clone(),
                index,
                &mut staged.setup,
            );
            return staged.finish(body);
        }

        if matches!(ctx.native_type, NativeGoType::Slice)
            && let Some(result) = self.try_lower_slice_loop(ctx, receiver_expression, arguments)
        {
            return result;
        }

        if ctx.method == "clone" {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if is_cloneable_container(&receiver_ty) {
                let staged = self.stage_native_method(ctx, form);
                let body = self.render_clone(&staged.receiver, &receiver_ty);
                return staged.finish(body);
            }
        }

        let mut staged = self.stage_native_method(ctx, form);
        if growth_clip_applies(ctx, receiver_expression) {
            staged.receiver = clip_shared_capacity(&staged.receiver);
        }

        let inlined = match form {
            NativeMethodForm::Dot => apply_inline_lookup(
                self,
                ctx.native_type,
                ctx.method,
                &staged.receiver,
                &staged.arguments,
                false,
            ),
            NativeMethodForm::Identifier => try_inline_native_method(
                ctx.native_type,
                ctx.method,
                &staged.receiver,
                &staged.arguments,
                false,
            )
            .map(|(value, import)| {
                apply_inline_import(self, import);
                value
            }),
        };
        if let Some(inlined) = inlined {
            return staged.finish(inlined);
        }

        let mut emitted_args = vec![staged.receiver.clone()];
        emitted_args.extend(staged.arguments.iter().cloned());
        self.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args = match form {
            NativeMethodForm::Dot
                if !ctx.resolved_type_args.is_empty() && ctx.call_ty.is_some() =>
            {
                self.format_type_args_with_receiver(
                    &receiver_expression.get_type(),
                    ctx.resolved_type_args,
                )
            }
            NativeMethodForm::Dot | NativeMethodForm::Identifier => {
                self.format_resolved_type_args(ctx.resolved_type_args)
            }
        };
        staged.finish(format!("{fn_name}{type_args}({})", emitted_args.join(", ")))
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
                resolution,
                ..
            } => {
                if matches!(
                    resolution.kind(),
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

    /// Returns `None` when the rule has no direct negated form, without staging.
    pub(super) fn try_emit_negated_native_method(
        &mut self,
        setup: &mut Vec<LoweredStatement>,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        let (form, arity) = if matches!(ctx.function, Expression::DotAccess { .. }) {
            (NativeMethodForm::Dot, ctx.args.len())
        } else {
            (
                NativeMethodForm::Identifier,
                ctx.args.len().saturating_sub(1),
            )
        };
        if !has_inline_negation(ctx.native_type, ctx.method, arity) {
            return None;
        }
        let staged = self.stage_native_method(ctx, form);
        let (inlined, import) = match form {
            NativeMethodForm::Dot => {
                let value = apply_inline_lookup(
                    self,
                    ctx.native_type,
                    ctx.method,
                    &staged.receiver,
                    &staged.arguments,
                    true,
                )?;
                (value, InlineImport::None)
            }
            NativeMethodForm::Identifier => try_inline_native_method(
                ctx.native_type,
                ctx.method,
                &staged.receiver,
                &staged.arguments,
                true,
            )?,
        };
        setup.extend(staged.setup);
        apply_inline_import(self, import);
        Some(inlined)
    }

    /// Pin the receiver stage to a temp when it reads a mutable operand,
    /// carries no setup of its own, and a later argument (or the spread)
    /// contains a call, so the receiver is captured before those args can
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

    fn stage_native_method(
        &mut self,
        ctx: &NativeCallContext,
        form: NativeMethodForm,
    ) -> StagedNativeMethod {
        let (receiver, mut stages) = match (form, ctx.function) {
            (NativeMethodForm::Dot, Expression::DotAccess { expression, .. }) => {
                let mut stages = vec![self.stage_operand(expression, ExpressionContext::value())];
                stages.extend(self.stage_native_method_args(ctx.function, ctx.args));
                (expression.as_ref(), stages)
            }
            (NativeMethodForm::Identifier, _) => (
                &ctx.args[0],
                self.stage_native_method_args(ctx.function, ctx.args),
            ),
            (NativeMethodForm::Dot, _) => unreachable!("dot form requires dot access"),
        };
        let spread_stage = ctx
            .spread
            .map(|spread| self.stage_operand(spread, ExpressionContext::value()));
        let rest_has_call = stages[1..]
            .iter()
            .chain(spread_stage.iter())
            .any(|stage| stage.evaluation.effect.has_call());
        self.pin_receiver_if_mutated(&mut stages[0], receiver, rest_has_call);
        if matches!(form, NativeMethodForm::Dot) && receiver.get_type().is_ref() {
            let receiver = stages.remove(0).unary("*");
            stages.insert(0, receiver);
        }
        if growth_clip_applies(ctx, receiver)
            && !is_clip_safe_path(&stages[0].expression.rendered())
        {
            self.pin_staged(&mut stages[0], "recv");
        }
        let spread_index = spread_stage.map(|stage| {
            stages.push(stage);
            stages.len() - 1
        });

        let receiver_offset = matches!(form, NativeMethodForm::Dot) as usize;
        let combine = plan_variadic_spread(&self.facts, ctx.function, ctx.spread)
            .map(|plan| plan.combine(receiver_offset));
        let mut sequenced = self.sequence_values(stages, ctx.capture_boundary, "arg");
        if let Some(spread_index) = spread_index {
            self.finalize_spread_stage(&mut sequenced.values, spread_index, false, combine);
        }
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, mut values) = sequenced.into_rendered();
        let receiver = values.remove(0);
        StagedNativeMethod {
            setup,
            receiver,
            arguments: values,
            effect,
            contains_deferred_evaluation,
        }
    }

    /// Lower `m.get(k)` to the native comma-ok index expression `m[k]`.
    pub(super) fn lower_map_index_pair(
        &mut self,
        expression: &Expression,
    ) -> (Vec<LoweredStatement>, String) {
        let Expression::Call {
            expression: function,
            args,
            type_arguments,
            ..
        } = expression
        else {
            unreachable!("lower_map_index_pair requires a Call expression");
        };
        let resolved_type_args = type_arguments
            .resolved_types()
            .expect("emission requires checked call type arguments");
        let native_type = NativeGoType::Map;
        let ctx = NativeCallContext {
            function,
            args,
            spread: None,
            resolved_type_args,
            call_ty: None,
            native_type: &native_type,
            method: "get",
            capture_boundary: CaptureBoundary::SiblingSequence,
            retired_receiver: None,
        };
        let staged = self.stage_native_method(&ctx, NativeMethodForm::Dot);
        let receiver = super::comma_ok::parenthesize_prefixed(staged.receiver);
        (
            staged.setup,
            format!("{}[{}]", receiver, staged.arguments[0]),
        )
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
            let sequenced = self.sequence_values(stages, capture_boundary, "arg");
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
        let range_kind = peel_to_range_type(&arg_ty, |id| self.facts.definition(id))
            .and_then(|ty| ty.get_name().map(str::to_owned))
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.stage_operand(receiver_expr, ExpressionContext::value());
        let range_staged = self.stage_or_capture(arg, "range");
        let sequenced =
            self.sequence_values(vec![receiver_staged, range_staged], capture_boundary, "arg");
        let effect = sequenced.effect;
        let contains_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (setup, values) = sequenced.into_rendered();
        let (start, end) = range_var_bounds(&values[1], &range_kind);
        NativeCallResult::new(
            setup,
            format_substring_call(&deref(&values[0]), start.as_deref(), end.as_deref()),
            effect,
            contains_deferred_evaluation,
        )
    }

    pub(crate) fn render_equality(
        &mut self,
        lhs: &str,
        rhs: &str,
        ty: &Type,
        generics: &[Generic],
    ) -> String {
        self.render_equality_test(lhs, rhs, ty, generics, false)
    }

    /// `!=` where equality is an operator, a `!` prefix where it is a call.
    pub(crate) fn render_inequality(
        &mut self,
        lhs: &str,
        rhs: &str,
        ty: &Type,
        generics: &[Generic],
    ) -> String {
        self.render_equality_test(lhs, rhs, ty, generics, true)
    }

    fn render_equality_test(
        &mut self,
        lhs: &str,
        rhs: &str,
        ty: &Type,
        generics: &[Generic],
        negated: bool,
    ) -> String {
        let (operator, prefix) = if negated { ("!=", "!") } else { ("==", "") };
        let peeled = self.facts.peel_alias(ty);
        if peeled.is_ref() {
            return format!("{lhs} {operator} {rhs}");
        }
        if peeled.is_slice() {
            self.require_slices();
            return match peeled.inner() {
                Some(elem) if self.needs_custom_equality(&elem, generics) => {
                    let eq = self.equality_closure(&elem, generics);
                    format!("{prefix}slices.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("{prefix}slices.Equal({lhs}, {rhs})"),
            };
        }
        if peeled.is_map() {
            self.require_maps();
            let value = peeled
                .as_compound()
                .and_then(|(_, args)| args.get(1).cloned());
            return match value {
                Some(value) if self.needs_custom_equality(&value, generics) => {
                    let eq = self.equality_closure(&value, generics);
                    format!("{prefix}maps.EqualFunc({lhs}, {rhs}, {eq})")
                }
                _ => format!("{prefix}maps.Equal({lhs}, {rhs})"),
            };
        }
        if self.type_has_equals(&peeled, generics) {
            return format!("{prefix}{lhs}.{}({rhs})", self.equals_method_go_name());
        }
        format!("{lhs} {operator} {rhs}")
    }

    fn equality_closure(&mut self, ty: &Type, generics: &[Generic]) -> String {
        let go_ty = self.use_go_type(ty);
        let a = self.fresh_var(Some("a"));
        let b = self.fresh_var(Some("b"));
        let body = self.render_equality(&a, &b, ty, generics);
        format!("func({a} {go_ty}, {b} {go_ty}) bool {{ return {body} }}")
    }

    fn contains_predicate(&mut self, ty: &Type, target: &str, generics: &[Generic]) -> String {
        let go_ty = self.use_go_type(ty);
        let element = self.fresh_var(Some("e"));
        let body = self.render_equality(&element, target, ty, generics);
        format!("func({element} {go_ty}) bool {{ return {body} }}")
    }

    fn needs_custom_equality(&self, ty: &Type, generics: &[Generic]) -> bool {
        self.is_container(ty) || self.type_has_equals(ty, generics)
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

pub(super) fn apply_inline_import(planner: &mut Planner, import: InlineImport) {
    match import {
        InlineImport::Slices => planner.require_slices(),
        InlineImport::Strings => planner.require_strings(),
        InlineImport::Stdlib => planner.require_stdlib(),
        InlineImport::None => {}
    }
}
