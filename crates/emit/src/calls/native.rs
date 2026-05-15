use super::NativeCallContext;
use crate::Emitter;
use crate::expressions::access::index_access::range_var_bounds;
use crate::expressions::context::ExpressionContext;
use crate::expressions::emission::EmittedExpression;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;
use syntax::types::peel_to_range_type;

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

/// Try to inline a native type method call to raw Go.
///
/// `negated=true` selects each rule's `negated_template`, returning `None`
/// when the rule does not define one. Many native type methods are thin
/// wrappers around Go builtins; inlining them avoids function call overhead.
pub(super) fn try_inline_native_method(
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    args: &[String],
    negated: bool,
) -> Option<(String, InlineImport)> {
    // Special case: append with 0 args returns receiver unchanged
    // (Go's append requires at least 2 args).
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

/// Whether some rule for this (type, method, arity) defines a negated template.
/// Used as a cheap pre-check before staging arguments.
pub(super) fn has_inline_negation(native_type: &NativeGoType, method: &str, arity: usize) -> bool {
    lookup_inline_rule(native_type, method, arity)
        .and_then(|r| r.negated_template)
        .is_some()
}

/// Resolve the inline rule for a dot-access form, applying the static-receiver
/// fallback when the standard receiver shape does not match.
fn apply_inline_lookup(
    emitter: &mut Emitter,
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    emitted_args: &[String],
    negated: bool,
) -> Option<String> {
    if let Some((inlined, import)) =
        try_inline_native_method(native_type, method, receiver, emitted_args, negated)
    {
        emitter.apply_inline_import(import);
        return Some(inlined);
    }
    if let Some((static_receiver, remaining)) = emitted_args.split_first()
        && let Some((inlined, import)) =
            try_inline_native_method(native_type, method, static_receiver, remaining, negated)
    {
        emitter.apply_inline_import(import);
        return Some(inlined);
    }
    None
}

/// Resolve the inline rule for an identifier-form call (args[0] is the receiver).
fn apply_inline_identifier_lookup(
    emitter: &mut Emitter,
    ctx: &NativeCallContext,
    emitted_args: &[String],
    negated: bool,
) -> Option<String> {
    let (receiver, remaining) = emitted_args.split_first()?;
    let (inlined, import) =
        try_inline_native_method(ctx.native_type, ctx.method, receiver, remaining, negated)?;
    emitter.apply_inline_import(import);
    Some(inlined)
}

impl Emitter<'_> {
    pub(super) fn apply_inline_import(&mut self, import: InlineImport) {
        match import {
            InlineImport::Slices => self.requirements.require_slices(),
            InlineImport::Strings => self.requirements.require_strings(),
            InlineImport::Maps => self.requirements.require_maps(),
            InlineImport::Stdlib => self.requirements.require_stdlib(),
            InlineImport::None => {}
        }
    }

    pub(super) fn emit_native_method_dot_access(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> String {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        if matches!(ctx.native_type, NativeGoType::String) && ctx.method == "substring" {
            return self.emit_string_substring(output, expression, ctx.args);
        }

        let (receiver, emitted_args) = self.stage_native_dot_access_call(output, ctx);

        if let Some(inlined) = apply_inline_lookup(
            self,
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            false,
        ) {
            return inlined;
        }

        let mut new_args = vec![receiver];
        new_args.extend(emitted_args);
        self.requirements.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = if !ctx.type_args.is_empty() && ctx.call_ty.is_some() {
            let receiver_ty = expression.get_type();
            self.format_type_args_with_receiver(&receiver_ty, ctx.type_args)
        } else {
            self.format_type_args_from_annotations(ctx.type_args)
        };
        format!("{}{}({})", fn_name, type_args_string, new_args.join(", "))
    }

    /// Negated counterpart for dot-access native method calls. Returns
    /// `None` when the rule has no `negated_template`, so the unary-not
    /// caller can fall back to `!expr` without having staged anything.
    pub(super) fn try_emit_negated_native_method_dot_access(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        if !has_inline_negation(ctx.native_type, ctx.method, ctx.args.len()) {
            return None;
        }
        let (receiver, emitted_args) = self.stage_native_dot_access_call(output, ctx);
        apply_inline_lookup(
            self,
            ctx.native_type,
            ctx.method,
            &receiver,
            &emitted_args,
            true,
        )
    }

    fn stage_native_dot_access_call(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> (String, Vec<String>) {
        let Expression::DotAccess { expression, .. } = ctx.function else {
            unreachable!("expected DotAccess for native method call")
        };

        let mut all_stages: Vec<EmittedExpression> =
            Vec::with_capacity(1 + ctx.args.len() + ctx.spread.is_some() as usize);
        all_stages.push(self.stage_operand(expression, ExpressionContext::value()));
        all_stages.extend(self.stage_native_method_args(ctx.function, ctx.args));

        let combine = Self::variadic_combine_for(ctx.function, ctx.spread, 1);
        let all_values =
            self.sequence_with_spread(output, all_stages, ctx.spread, false, "_arg", combine);

        let raw_receiver = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();

        let receiver = if expression.get_type().is_ref() {
            format!("*{}", raw_receiver)
        } else {
            raw_receiver
        };
        (receiver, emitted_args)
    }

    pub(super) fn emit_native_method_identifier(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> String {
        if matches!(ctx.native_type, NativeGoType::String)
            && ctx.method == "substring"
            && ctx.args.len() >= 2
        {
            return self.emit_string_substring(output, &ctx.args[0], &ctx.args[1..]);
        }

        let emitted_args = self.stage_native_identifier_args(output, ctx);

        if let Some(inlined) = apply_inline_identifier_lookup(self, ctx, &emitted_args, false) {
            return inlined;
        }

        self.requirements.require_stdlib();
        let fn_name = format!(
            "{}.{}{}",
            go_name::GO_STDLIB_PKG,
            ctx.native_type.method_prefix(),
            go_name::snake_to_camel(ctx.method)
        );
        let type_args_string = self.format_type_args_from_annotations(ctx.type_args);
        format!(
            "{}{}({})",
            fn_name,
            type_args_string,
            emitted_args.join(", ")
        )
    }

    /// Negated counterpart for identifier-form native method calls.
    pub(super) fn try_emit_negated_native_method_identifier(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> Option<String> {
        let receiver_arity = ctx.args.len().saturating_sub(1);
        if !has_inline_negation(ctx.native_type, ctx.method, receiver_arity) {
            return None;
        }
        let emitted_args = self.stage_native_identifier_args(output, ctx);
        apply_inline_identifier_lookup(self, ctx, &emitted_args, true)
    }

    fn stage_native_identifier_args(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> Vec<String> {
        let stages = self.stage_native_method_args(ctx.function, ctx.args);
        let combine = Self::variadic_combine_for(ctx.function, ctx.spread, 0);
        self.sequence_with_spread(output, stages, ctx.spread, false, "_arg", combine)
    }

    fn emit_string_substring(
        &mut self,
        output: &mut String,
        receiver_expr: &Expression,
        args: &[Expression],
    ) -> String {
        self.requirements.require_stdlib();
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
            let values = self.sequence(output, stages, "_arg");
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
            return format_substring_call(
                &deref(&values[0]),
                start_bound.as_deref(),
                end_bound.as_deref(),
            );
        }

        let arg_ty = arg.get_type();
        let range_kind = peel_to_range_type(&arg_ty)
            .and_then(|t| t.get_name())
            .expect("substring arg should resolve to a known range type");
        let receiver_staged = self.stage_operand(receiver_expr, ExpressionContext::value());
        let range_staged = self.stage_or_capture(arg, "range");
        let values = self.sequence(output, vec![receiver_staged, range_staged], "_arg");
        let (start, end) = range_var_bounds(&values[1], range_kind);
        format_substring_call(&deref(&values[0]), start.as_deref(), end.as_deref())
    }
}

fn format_substring_call(recv: &str, start: Option<&str>, end: Option<&str>) -> String {
    let pkg = go_name::GO_STDLIB_PKG;
    match (start, end) {
        (Some(s), Some(e)) => format!("{}.Substring({}, {}, {})", pkg, recv, s, e),
        (Some(s), None) => format!("{}.SubstringFrom({}, {})", pkg, recv, s),
        (None, Some(e)) => format!("{}.SubstringTo({}, {})", pkg, recv, e),
        (None, None) => unreachable!("`s.substring(..)` is rejected upstream"),
    }
}
