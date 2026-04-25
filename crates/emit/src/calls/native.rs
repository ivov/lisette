use super::NativeCallContext;
use crate::Emitter;
use crate::names::go_name;
use crate::types::native::NativeGoType;
use crate::utils::Staged;
use syntax::ast::Expression;

#[derive(Clone, Copy)]
pub(super) enum InlineImport {
    None,
    Slices,
    Strings,
    Maps,
}

struct InlineRule {
    types: &'static [NativeGoType],
    method: &'static str,
    arity: i8,
    template: &'static str,
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
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice, N::Channel, N::Sender, N::Receiver],
        method: "capacity",
        arity: 0,
        template: "cap({r})",
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
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "enumerate",
        arity: 0,
        template: "{r}",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "clone",
        arity: 0,
        template: "slices.Clone({r})",
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::Map],
        method: "clone",
        arity: 0,
        template: "maps.Clone({r})",
        import: InlineImport::Maps,
    },
    // Single-arg methods
    InlineRule {
        types: &[N::Map],
        method: "delete",
        arity: 1,
        template: "delete({r}, {0})",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "extend",
        arity: 1,
        template: "append({r}, {0}...)",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "copy_from",
        arity: 1,
        template: "copy({r}, {0})",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "contains",
        arity: 1,
        template: "slices.Contains({r}, {0})",
        import: InlineImport::Slices,
    },
    InlineRule {
        types: &[N::String],
        method: "contains",
        arity: 1,
        template: "strings.Contains({r}, {0})",
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "split",
        arity: 1,
        template: "strings.Split({r}, {0})",
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "starts_with",
        arity: 1,
        template: "strings.HasPrefix({r}, {0})",
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "ends_with",
        arity: 1,
        template: "strings.HasSuffix({r}, {0})",
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::String],
        method: "byte_at",
        arity: 1,
        template: "{r}[{0}]",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::String],
        method: "rune_at",
        arity: 1,
        template: "[]rune({r})[{0}]",
        import: InlineImport::None,
    },
    InlineRule {
        types: &[N::Slice],
        method: "join",
        arity: 1,
        template: "strings.Join({r}, {0})",
        import: InlineImport::Strings,
    },
    InlineRule {
        types: &[N::Slice],
        method: "any",
        arity: 1,
        template: "slices.ContainsFunc({r}, {0})",
        import: InlineImport::Slices,
    },
    // Variadic methods
    InlineRule {
        types: &[N::Slice],
        method: "append",
        arity: -1,
        template: "append({r+args})",
        import: InlineImport::None,
    },
];

fn render_inline(rule: &InlineRule, receiver: &str, args: &[String]) -> String {
    let mut result = rule.template.to_string();
    result = result.replace("{r}", receiver);
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

/// Try to inline a native type method call to raw Go.
///
/// Many native type methods are thin wrappers around Go builtins.
/// Inlining them produces cleaner output and avoids function call overhead.
///
/// Returns `Some((code, extra_import))` if the method can be inlined.
pub(super) fn try_inline_native_method(
    native_type: &NativeGoType,
    method: &str,
    receiver: &str,
    args: &[String],
) -> Option<(String, InlineImport)> {
    // Special case: append with 0 args returns receiver unchanged
    // (Go's append requires at least 2 args)
    if method == "append" && args.is_empty() {
        return Some((receiver.to_string(), InlineImport::None));
    }

    let rule = INLINE_METHODS.iter().find(|s| {
        s.method == method
            && s.types.contains(native_type)
            && (s.arity < 0 || s.arity as usize == args.len())
    })?;

    Some((render_inline(rule, receiver, args), rule.import))
}

impl Emitter<'_> {
    pub(super) fn apply_inline_import(&mut self, import: InlineImport) {
        match import {
            InlineImport::Slices => self.flags.needs_slices = true,
            InlineImport::Strings => self.flags.needs_strings = true,
            InlineImport::Maps => self.flags.needs_maps = true,
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

        let mut all_stages: Vec<Staged> =
            Vec::with_capacity(1 + ctx.args.len() + ctx.spread.is_some() as usize);
        all_stages.push(self.stage_operand(expression));
        all_stages.extend(self.stage_native_method_args(ctx.function, ctx.args));
        let all_values = self.sequence_with_spread(output, all_stages, ctx.spread, false, "_arg");
        let raw_receiver = all_values[0].clone();
        let emitted_args: Vec<String> = all_values[1..].to_vec();

        let is_ref_receiver = expression.get_type().is_ref();
        let receiver = if is_ref_receiver {
            format!("*{}", raw_receiver)
        } else {
            raw_receiver.clone()
        };

        if let Some((inlined, extra_import)) =
            try_inline_native_method(ctx.native_type, ctx.method, &receiver, &emitted_args)
        {
            self.apply_inline_import(extra_import);

            return inlined;
        }

        if !emitted_args.is_empty() {
            let static_receiver = &emitted_args[0];
            let remaining_args = &emitted_args[1..];
            if let Some((inlined, extra_import)) = try_inline_native_method(
                ctx.native_type,
                ctx.method,
                static_receiver,
                remaining_args,
            ) {
                self.apply_inline_import(extra_import);
                return inlined;
            }
        }

        let mut new_args = vec![receiver];
        new_args.extend(emitted_args);
        self.flags.needs_stdlib = true;
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

    pub(super) fn emit_native_method_identifier(
        &mut self,
        output: &mut String,
        ctx: &NativeCallContext,
    ) -> String {
        let stages = self.stage_native_method_args(ctx.function, ctx.args);
        let emitted_args = self.sequence_with_spread(output, stages, ctx.spread, false, "_arg");
        if !emitted_args.is_empty() {
            let receiver = &emitted_args[0];
            let remaining_args = &emitted_args[1..];
            if let Some((inlined, extra_import)) =
                try_inline_native_method(ctx.native_type, ctx.method, receiver, remaining_args)
            {
                self.apply_inline_import(extra_import);
                return inlined;
            }
        }

        self.flags.needs_stdlib = true;
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
}
