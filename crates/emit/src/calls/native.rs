use super::NativeCallContext;
use crate::Planner;
use crate::calls::dispatch::extract_native_method_name;
use crate::context::expression::ExpressionContext;
use crate::control_flow::propagation::plain_return;
use crate::expressions::access::index_access::range_var_bounds;
use crate::names::go_name;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{LoweredBlock, LoweredStatement};
use crate::plan::calls::plan_variadic_spread;
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use crate::statements::assignments::lvalues_match;
use crate::types::native::NativeGoType;
use syntax::ast::{Expression, Generic, Literal, UnaryOperator};
use syntax::program::{CallKind, DotAccessKind, NativeTypeKind};
use syntax::types::{CompoundKind, Type, peel_to_range_type};

pub(super) struct NativeCallResult {
    pub setup: Vec<LoweredStatement>,
    pub value: GoExpression,
    pub argument_effect: EvaluationEffect,
}

impl NativeCallResult {
    pub(super) fn new(
        setup: Vec<LoweredStatement>,
        value: GoExpression,
        argument_effect: EvaluationEffect,
    ) -> Self {
        Self {
            setup,
            value,
            argument_effect,
        }
    }
}

#[derive(Clone, Copy)]
enum InlinePackage {
    None,
    Slices,
    Strings,
    Prelude,
}

/// The Go shape a native method inlines to, given the receiver and arguments.
#[derive(Clone, Copy)]
enum InlineForm {
    /// `callee(receiver, arguments...)`.
    Call(&'static str),
    /// `len(receiver) == 0`. The negated method flips the operator instead of
    /// prepending `!`, which binds tighter than `==` in Go.
    IsEmpty,
    Receiver,
    /// `T(receiver)`.
    Conversion(&'static str),
    /// `receiver[argument]`.
    Index,
}

struct InlineRule {
    types: &'static [NativeGoType],
    method: &'static str,
    arity: RuleArity,
    form: InlineForm,
    package: InlinePackage,
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
        form: InlineForm::Call("len"),
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::Slice, N::Channel, N::Sender, N::Receiver],
        method: "capacity",
        arity: RuleArity::Exact(0),
        form: InlineForm::Call("cap"),
        package: InlinePackage::None,
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
        form: InlineForm::IsEmpty,
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "enumerate",
        arity: RuleArity::Exact(0),
        form: InlineForm::Receiver,
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::String],
        method: "bytes",
        arity: RuleArity::Exact(0),
        form: InlineForm::Conversion("[]byte"),
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::String],
        method: "runes",
        arity: RuleArity::Exact(0),
        form: InlineForm::Conversion("[]rune"),
        package: InlinePackage::None,
    },
    // Single-arg methods
    InlineRule {
        types: &[N::Map],
        method: "delete",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("delete"),
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "copy_from",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("copy"),
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "contains",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("Contains"),
        package: InlinePackage::Slices,
    },
    InlineRule {
        types: &[N::String],
        method: "contains",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("Contains"),
        package: InlinePackage::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "split",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("Split"),
        package: InlinePackage::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "starts_with",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("HasPrefix"),
        package: InlinePackage::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "ends_with",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("HasSuffix"),
        package: InlinePackage::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "byte_at",
        arity: RuleArity::Exact(1),
        form: InlineForm::Index,
        package: InlinePackage::None,
    },
    InlineRule {
        types: &[N::String],
        method: "rune_at",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("RuneAt"),
        package: InlinePackage::Prelude,
    },
    InlineRule {
        types: &[N::Slice],
        method: "join",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("Join"),
        package: InlinePackage::Strings,
    },
    InlineRule {
        types: &[N::Slice],
        method: "any",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("ContainsFunc"),
        package: InlinePackage::Slices,
    },
    InlineRule {
        types: &[N::Slice],
        method: "reserve",
        arity: RuleArity::Exact(1),
        form: InlineForm::Call("Grow"),
        package: InlinePackage::Slices,
    },
    // Variadic methods
    InlineRule {
        types: &[N::Slice],
        method: "append",
        arity: RuleArity::Variadic,
        form: InlineForm::Call("append"),
        package: InlinePackage::None,
    },
];

/// `receiver[:len(receiver):len(receiver)]`, so a later `append` cannot write through an alias.
pub(crate) fn clip_shared_capacity(receiver: GoExpression) -> GoExpression {
    let length = GoExpression::call(
        GoExpression::name("len".to_string()),
        vec![receiver.clone()],
    );
    GoExpression::slice(receiver, None, Some(&length), Some(&length))
}

fn grows_into_capacity(method: &str, appends_anything: bool) -> bool {
    method == "reserve" || (method == "append" && appends_anything)
}

/// Natives that write no memory a sibling operand can read and run no caller code.
pub(super) fn native_method_is_pure(native_type: &NativeGoType, method: &str) -> bool {
    match native_type {
        NativeGoType::String => matches!(
            method,
            "length"
                | "is_empty"
                | "contains"
                | "split"
                | "starts_with"
                | "ends_with"
                | "byte_at"
                | "rune_at"
                | "bytes"
                | "runes"
                | "substring"
        ),
        NativeGoType::Array => matches!(method, "length" | "get" | "to_slice"),
        NativeGoType::Slice => matches!(
            method,
            "length"
                | "is_empty"
                | "capacity"
                | "get"
                | "append"
                | "contains"
                | "enumerate"
                | "clone"
                | "join"
        ),
        NativeGoType::Map => matches!(method, "length" | "is_empty" | "get" | "clone"),
        NativeGoType::Channel | NativeGoType::Sender | NativeGoType::Receiver => {
            matches!(method, "length" | "is_empty" | "capacity")
        }
        NativeGoType::EnumeratedSlice => method == "clone",
    }
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
            CallKind::NativeMethod(NativeTypeKind::Array)
            | CallKind::NativeMethodIdentifier(NativeTypeKind::Array) => {
                extract_native_method_name(callee.unwrap_parens()) == "to_slice"
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

fn build_inline(
    form: InlineForm,
    package: InlinePackage,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    negated: bool,
) -> GoExpression {
    match form {
        InlineForm::Call(callee) => {
            let callee = match package {
                InlinePackage::None => GoExpression::name(callee.to_string()),
                InlinePackage::Slices => GoExpression::generated(GeneratedPackage::Slices, callee),
                InlinePackage::Strings => {
                    GoExpression::generated(GeneratedPackage::Strings, callee)
                }
                InlinePackage::Prelude => {
                    GoExpression::generated(GeneratedPackage::Prelude, callee)
                }
            };
            let mut all = vec![receiver.clone()];
            all.extend(arguments.iter().cloned());
            GoExpression::call(callee, all)
        }
        InlineForm::IsEmpty => {
            let operator = if negated { "!=" } else { "==" };
            GoExpression::binary(
                GoExpression::call(
                    GoExpression::name("len".to_string()),
                    vec![receiver.clone()],
                ),
                operator,
                GoExpression::literal("0".to_string()),
            )
        }
        InlineForm::Receiver => receiver.clone(),
        // A slice type needs no parentheses as a callee, unlike `Conversion`'s general form.
        InlineForm::Conversion(go_type) => GoExpression::call(
            GoExpression::type_name(go_type.to_string()),
            vec![receiver.clone()],
        ),
        InlineForm::Index => {
            let base = receiver.clone();
            let index = arguments
                .first()
                .expect("an index rule takes one argument")
                .clone();
            GoExpression::index(base, index)
        }
    }
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

/// Try to inline a native-type method call. `negated` asks for the rule's
/// negated form (`None` when the rule has none).
pub(super) fn try_inline_native_method(
    native_type: &NativeGoType,
    method: &str,
    receiver: &GoExpression,
    arguments: &[GoExpression],
    negated: bool,
) -> Option<GoExpression> {
    // Go's `append` requires at least 2 args, so zero-arg `append` returns
    // the receiver unchanged.
    if !negated && method == "append" && arguments.is_empty() {
        return Some(receiver.clone());
    }
    let rule = lookup_inline_rule(native_type, method, arguments.len())?;
    if negated && !matches!(rule.form, InlineForm::IsEmpty) {
        return None;
    }
    Some(build_inline(
        rule.form,
        rule.package,
        receiver,
        arguments,
        negated,
    ))
}

fn is_native_array_method(method: &str) -> bool {
    matches!(method, "to_slice" | "get")
}

/// Reads the receiver, runs no user code, and returns no view of it.
fn slice_method_reads_elements_only(method: &str, arity: usize) -> bool {
    matches!(
        (method, arity),
        ("length", 0)
            | ("is_empty", 0)
            | ("clone", 0)
            | ("get", 1)
            | ("contains", 1)
            | ("equals", 1)
            | ("join", 1)
    )
}

fn contains_call_expression(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. } | Expression::Task { .. }
    ) || expression
        .children()
        .into_iter()
        .any(contains_call_expression)
}

fn array_to_slice_receiver(expression: &Expression) -> Option<&Expression> {
    let Expression::Call {
        expression: callee,
        args,
        spread,
        call_kind,
        ..
    } = expression.unwrap_parens()
    else {
        return None;
    };
    if spread.is_some() || extract_native_method_name(callee.unwrap_parens()) != "to_slice" {
        return None;
    }
    match call_kind {
        CallKind::NativeMethod(NativeTypeKind::Array) if args.is_empty() => {
            match callee.unwrap_parens() {
                Expression::DotAccess { expression, .. } => Some(expression),
                _ => None,
            }
        }
        CallKind::NativeMethodIdentifier(NativeTypeKind::Array) if args.len() == 1 => args.first(),
        _ => None,
    }
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

/// Whether a rule for `(type, method, arity)` has a negated form.
fn has_inline_negation(native_type: &NativeGoType, method: &str, arity: usize) -> bool {
    lookup_inline_rule(native_type, method, arity)
        .is_some_and(|rule| matches!(rule.form, InlineForm::IsEmpty))
}

/// Resolve the inline rule for a dot-access form, applying the static-receiver
/// fallback when the standard receiver shape does not match.
fn apply_inline_lookup(
    native_type: &NativeGoType,
    method: &str,
    receiver: &GoExpression,
    emitted_args: &[GoExpression],
    negated: bool,
) -> Option<GoExpression> {
    if let Some(inlined) =
        try_inline_native_method(native_type, method, receiver, emitted_args, negated)
    {
        return Some(inlined);
    }
    if let Some((static_receiver, remaining)) = emitted_args.split_first()
        && let Some(inlined) =
            try_inline_native_method(native_type, method, static_receiver, remaining, negated)
    {
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
    receiver: GoExpression,
    arguments: Vec<GoExpression>,
    effect: EvaluationEffect,
}

impl StagedNativeMethod {
    fn finish(self, value: GoExpression) -> NativeCallResult {
        NativeCallResult::new(self.setup, value, self.effect)
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
                let body = self.equality_test(
                    staged.receiver.clone(),
                    staged.arguments[0].clone(),
                    &receiver_ty,
                    &[],
                    false,
                );
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
                let searched = staged.arguments[0].clone();
                let target = self.hoist_tmp_value_statement(&mut staged.setup, "want", searched);
                let predicate = self.contains_predicate(&element, GoExpression::name(target), &[]);
                let body = GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "ContainsFunc"),
                    vec![staged.receiver.clone(), predicate],
                );
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
            let index = staged.arguments.first().cloned();
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
            && let Some(result) =
                self.try_lower_slice_loop(ctx, receiver_expression, arguments, None)
        {
            return result;
        }

        if ctx.method == "clone" {
            let receiver_ty = self.facts.strip_and_peel(&receiver_expression.get_type());
            if is_cloneable_container(&receiver_ty) {
                let staged = self.stage_native_method(ctx, form);
                let body = self.clone_expression(staged.receiver.clone(), &receiver_ty);
                return staged.finish(body);
            }
        }

        let mut staged = self.stage_native_method(ctx, form);
        if growth_clip_applies(ctx, receiver_expression) {
            staged.receiver = clip_shared_capacity(staged.receiver);
        }

        let inlined = match form {
            NativeMethodForm::Dot => apply_inline_lookup(
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
            ),
        };
        if let Some(inlined) = inlined {
            return staged.finish(inlined);
        }

        let fn_name = GoExpression::generated(
            GeneratedPackage::Prelude,
            format!(
                "{}{}",
                ctx.native_type.method_prefix(),
                go_name::snake_to_camel(ctx.method)
            ),
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
        let mut emitted_args = vec![staged.receiver.clone()];
        emitted_args.extend(staged.arguments.iter().cloned());
        staged.finish(GoExpression::call(
            GoExpression::instantiation(fn_name, type_args),
            emitted_args,
        ))
    }

    fn lower_array_method_body(
        &mut self,
        method: &str,
        receiver_expr: &Expression,
        receiver: GoExpression,
        index: Option<GoExpression>,
        setup: &mut Vec<LoweredStatement>,
    ) -> GoExpression {
        match method {
            "to_slice" => {
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Clone"),
                    vec![view],
                )
            }
            "get" => {
                let view = self.sliceable_receiver(receiver_expr, receiver, setup);
                GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Prelude, "SliceGet"),
                    vec![view, index.expect("get needs an index")],
                )
            }
            other => unreachable!("not a native array method: {other}"),
        }
    }

    fn sliceable_receiver(
        &mut self,
        expression: &Expression,
        receiver: GoExpression,
        setup: &mut Vec<LoweredStatement>,
    ) -> GoExpression {
        let base = if self.receiver_is_addressable(expression) {
            receiver
        } else {
            GoExpression::name(self.hoist_tmp_value_statement(setup, "arr", receiver))
        };
        GoExpression::slice(base, None, None, None)
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
    ) -> Option<GoExpression> {
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
        let inlined = match form {
            NativeMethodForm::Dot => apply_inline_lookup(
                ctx.native_type,
                ctx.method,
                &staged.receiver,
                &staged.arguments,
                true,
            )?,
            NativeMethodForm::Identifier => try_inline_native_method(
                ctx.native_type,
                ctx.method,
                &staged.receiver,
                &staged.arguments,
                true,
            )?,
        };
        setup.extend(staged.setup);
        Some(inlined)
    }

    /// Stage `to_slice()` as `arr[:]` when the consumer cannot observe the skipped clone.
    fn try_stage_to_slice_view(
        &mut self,
        ctx: &NativeCallContext,
        receiver_expression: &Expression,
        arguments: &[Expression],
    ) -> Option<ValuePlan> {
        if !matches!(ctx.native_type, NativeGoType::Slice) {
            return None;
        }
        if !matches!(ctx.capture_boundary, CaptureBoundary::SiblingSequence) {
            return None;
        }
        if ctx.spread.is_some()
            || !slice_method_reads_elements_only(ctx.method, arguments.len())
            || arguments.iter().any(contains_call_expression)
        {
            return None;
        }
        let array = array_to_slice_receiver(receiver_expression)?;
        if !matches!(
            self.facts.strip_and_peel(&array.get_type()),
            Type::Array { .. }
        ) {
            return None;
        }
        if matches!(ctx.method, "contains" | "equals") {
            let element = self
                .facts
                .strip_and_peel(&receiver_expression.get_type())
                .inner()?;
            if self.needs_custom_equality(&element, &[]) {
                return None;
            }
        }
        Some(self.stage_array_view(array))
    }

    fn stage_array_view(&mut self, array: &Expression) -> ValuePlan {
        let mut staged = self.plan_operand(array, ExpressionContext::value());
        if array.get_type().is_ref() {
            staged = staged.unary("*");
        }
        if !self.receiver_is_addressable(array) {
            self.pin_staged(&mut staged, "arr");
        }
        staged.map_expression_as_observable_computed(|_setup, array| {
            let base = array;
            GoExpression::slice(base, None, None, None)
        })
    }

    fn stage_native_method(
        &mut self,
        ctx: &NativeCallContext,
        form: NativeMethodForm,
    ) -> StagedNativeMethod {
        let (receiver, mut stages) = match (form, ctx.function) {
            (NativeMethodForm::Dot, Expression::DotAccess { expression, .. }) => {
                let receiver_stage = self
                    .try_stage_to_slice_view(ctx, expression, ctx.args)
                    .unwrap_or_else(|| self.plan_operand(expression, ExpressionContext::value()));
                let mut stages = vec![receiver_stage];
                stages.extend(self.stage_native_method_args_from(ctx.function, ctx.args, 0));
                (expression.as_ref(), stages)
            }
            (NativeMethodForm::Identifier, _) => {
                let receiver = &ctx.args[0];
                let stages = match self.try_stage_to_slice_view(ctx, receiver, &ctx.args[1..]) {
                    Some(view) => {
                        let mut stages = vec![view];
                        stages.extend(self.stage_native_method_args_from(
                            ctx.function,
                            ctx.args,
                            1,
                        ));
                        stages
                    }
                    None => self.stage_native_method_args_from(ctx.function, ctx.args, 0),
                };
                (receiver, stages)
            }
            (NativeMethodForm::Dot, _) => unreachable!("dot form requires dot access"),
        };
        let spread_stage = ctx
            .spread
            .map(|spread| self.plan_operand(spread, ExpressionContext::value()));
        if matches!(form, NativeMethodForm::Dot) && receiver.get_type().is_ref() {
            let receiver = stages.remove(0).unary("*");
            stages.insert(0, receiver);
        }
        if growth_clip_applies(ctx, receiver) && !is_clip_safe_path(stages[0].expression.as_str()) {
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
        let mut values = sequenced.values;
        let receiver = values.remove(0);
        StagedNativeMethod {
            setup: sequenced.setup,
            receiver,
            arguments: values,
            effect,
        }
    }

    /// Lower `m.get(k)` to the native comma-ok index expression `m[k]`.
    pub(super) fn lower_map_index_pair(
        &mut self,
        expression: &Expression,
    ) -> (Vec<LoweredStatement>, GoExpression) {
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
            result_name: None,
        };
        let mut staged = self.stage_native_method(&ctx, NativeMethodForm::Dot);
        let receiver = staged.receiver;
        let key = staged.arguments.remove(0);
        (staged.setup, GoExpression::index(receiver, key))
    }

    fn lower_string_substring(
        &mut self,
        receiver_expr: &Expression,
        args: &[Expression],
        capture_boundary: CaptureBoundary,
    ) -> NativeCallResult {
        let arg = &args[0];
        let is_ref_receiver = receiver_expr.get_type().is_ref();
        let deref = |raw: GoExpression| -> GoExpression {
            if is_ref_receiver {
                GoExpression::dereference(raw)
            } else {
                raw
            }
        };

        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = arg
        {
            let mut stages = vec![self.plan_operand(receiver_expr, ExpressionContext::value())];
            if let Some(s) = start.as_deref() {
                stages.push(self.plan_operand(s, ExpressionContext::value()));
            }
            if let Some(e) = end.as_deref() {
                stages.push(self.plan_operand(e, ExpressionContext::value()));
            }
            let sequenced = self.sequence_values(stages, capture_boundary, "arg");
            let effect = sequenced.effect;
            let mut values = sequenced.values.into_iter();
            let receiver = values.next().expect("substring has a receiver");
            let start_bound = start
                .is_some()
                .then(|| values.next().expect("range has a start"));
            let end_bound = end.is_some().then(|| {
                let end = values.next().expect("range has an end");
                if *inclusive {
                    GoExpression::binary(end, "+", GoExpression::literal("1".to_string()))
                } else {
                    end
                }
            });
            return NativeCallResult::new(
                sequenced.setup,
                substring_call(deref(receiver), start_bound, end_bound),
                effect,
            );
        }

        let arg_ty = arg.get_type();
        let range_kind = peel_to_range_type(&arg_ty, |id| self.facts.definition(id))
            .and_then(|ty| ty.get_name().map(str::to_owned))
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.plan_operand(receiver_expr, ExpressionContext::value());
        let range_staged = self.stage_or_capture(arg, "range");
        let sequenced =
            self.sequence_values(vec![receiver_staged, range_staged], capture_boundary, "arg");
        let effect = sequenced.effect;
        let mut values = sequenced.values.into_iter();
        let receiver = values.next().expect("substring has a receiver");
        let range = values.next().expect("substring has a range");
        let (start, end) = range_var_bounds(&range, &range_kind);
        NativeCallResult::new(
            sequenced.setup,
            substring_call(deref(receiver), start, end),
            effect,
        )
    }

    pub(crate) fn equality_expression(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        ty: &Type,
        generics: &[Generic],
    ) -> GoExpression {
        self.equality_test(lhs, rhs, ty, generics, false)
    }

    /// `!=` where equality is an operator, a `!` prefix where it is a call.
    pub(crate) fn inequality_expression(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        ty: &Type,
        generics: &[Generic],
    ) -> GoExpression {
        self.equality_test(lhs, rhs, ty, generics, true)
    }

    fn equality_test(
        &mut self,
        lhs: GoExpression,
        rhs: GoExpression,
        ty: &Type,
        generics: &[Generic],
        negated: bool,
    ) -> GoExpression {
        let operator = if negated { "!=" } else { "==" };
        let negate = |call: GoExpression| {
            if negated {
                GoExpression::unary("!", call)
            } else {
                call
            }
        };
        let peeled = self.facts.peel_alias(ty);
        if peeled.is_ref() {
            return GoExpression::binary(lhs, operator, rhs);
        }
        if peeled.is_slice() {
            return match peeled.inner() {
                Some(elem) if self.needs_custom_equality(&elem, generics) => {
                    let eq = self.equality_closure(&elem, generics);
                    negate(GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Slices, "EqualFunc"),
                        vec![lhs, rhs, eq],
                    ))
                }
                _ => negate(GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Slices, "Equal"),
                    vec![lhs, rhs],
                )),
            };
        }
        if peeled.is_map() {
            let value = peeled
                .as_compound()
                .and_then(|(_, args)| args.get(1).cloned());
            return match value {
                Some(value) if self.needs_custom_equality(&value, generics) => {
                    let eq = self.equality_closure(&value, generics);
                    negate(GoExpression::call(
                        GoExpression::generated(GeneratedPackage::Maps, "EqualFunc"),
                        vec![lhs, rhs, eq],
                    ))
                }
                _ => negate(GoExpression::call(
                    GoExpression::generated(GeneratedPackage::Maps, "Equal"),
                    vec![lhs, rhs],
                )),
            };
        }
        if self.type_has_equals(&peeled, generics) {
            let method = self.equals_method_go_name();
            return negate(GoExpression::call(
                GoExpression::selector(lhs, method.to_string()),
                vec![rhs],
            ));
        }
        GoExpression::binary(lhs, operator, rhs)
    }

    fn equality_closure(&mut self, ty: &Type, generics: &[Generic]) -> GoExpression {
        let go_ty = self.use_go_type(ty);
        let a = self.fresh_var(Some("a"));
        let b = self.fresh_var(Some("b"));
        let body = self.equality_expression(
            GoExpression::name(a.clone()),
            GoExpression::name(b.clone()),
            ty,
            generics,
        );
        predicate_literal(format!("{a} {go_ty}, {b} {go_ty}"), body)
    }

    fn contains_predicate(
        &mut self,
        ty: &Type,
        target: GoExpression,
        generics: &[Generic],
    ) -> GoExpression {
        let go_ty = self.use_go_type(ty);
        let element = self.fresh_var(Some("e"));
        let body =
            self.equality_expression(GoExpression::name(element.clone()), target, ty, generics);
        predicate_literal(format!("{element} {go_ty}"), body)
    }

    fn needs_custom_equality(&self, ty: &Type, generics: &[Generic]) -> bool {
        self.is_container(ty) || self.type_has_equals(ty, generics)
    }

    fn is_container(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        peeled.is_slice() || peeled.is_map()
    }
}

fn predicate_literal(parameters: String, body: GoExpression) -> GoExpression {
    GoExpression::function_literal(
        parameters,
        "bool".to_string(),
        LoweredBlock {
            statements: vec![plain_return(body)],
        },
        FunctionLiteralLayout::Inline,
    )
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

fn substring_call(
    receiver: GoExpression,
    start: Option<GoExpression>,
    end: Option<GoExpression>,
) -> GoExpression {
    let (function, bounds) = match (start, end) {
        (Some(start), Some(end)) => ("Substring", vec![start, end]),
        (Some(start), None) => ("SubstringFrom", vec![start]),
        (None, Some(end)) => ("SubstringTo", vec![end]),
        (None, None) => unreachable!("`s.substring(..)` is rejected upstream"),
    };
    let mut arguments = vec![receiver];
    arguments.extend(bounds);
    GoExpression::call(
        GoExpression::generated(GeneratedPackage::Prelude, function),
        arguments,
    )
}
