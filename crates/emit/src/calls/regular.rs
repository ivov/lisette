use rustc_hash::FxHashSet as HashSet;

use crate::Emitter;
use crate::names::go_name;
use crate::types::coercion::Coercion;
use crate::utils::Staged;
use crate::write_line;
use syntax::ast::{Annotation, Expression, UnaryOperator};
use syntax::types::Type;

struct CallArgsContext<'a> {
    fn_param_types: &'a [Type],
    pointer_indices: &'a HashSet<usize>,
    is_go_call: bool,
    /// Suppresses the Go-fn identity short-circuit on fn-typed params
    /// dispatching into prelude generic helpers (e.g. `OptionAndThen`).
    is_prelude_dispatch: bool,
    spread: Option<&'a Expression>,
    wrap_spread_to_any: bool,
}

/// Collapse redundant fmt wrappers:
/// - `fmt.Print{ln}(fmt.Sprintf(...))` → `fmt.Printf(..., "\n")`
/// - `fmt.Print{ln}(fmt.Sprint(x))` → `fmt.Print{ln}(x)`
fn collapse_fmt_print(function_string: &str, args_strings: &[String], call_str: String) -> String {
    if function_string != "fmt.Print" && function_string != "fmt.Println" {
        return call_str;
    }
    if args_strings.len() != 1 {
        return call_str;
    }
    let arg = &args_strings[0];

    if let Some(inner) = arg
        .strip_prefix("fmt.Sprintf(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let suffix = if function_string == "fmt.Println" {
            "\\n"
        } else {
            ""
        };
        if suffix.is_empty() {
            return format!("fmt.Printf({})", inner);
        }
        if let Some(close_quote) = inner.find("\", ") {
            let format_str = &inner[..close_quote];
            let rest = &inner[close_quote + 1..];
            return format!("fmt.Printf({}{}\"{})", format_str, suffix, rest);
        }
        if inner.starts_with('"') && inner.ends_with('"') {
            let format_str = &inner[..inner.len() - 1];
            return format!("fmt.Printf({}{}\")", format_str, suffix);
        }
        return call_str;
    }

    if let Some(inner) = arg
        .strip_prefix("fmt.Sprint(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return format!("{}({})", function_string, inner);
    }

    call_str
}

impl Emitter<'_> {
    pub(super) fn emit_regular_call(
        &mut self,
        output: &mut String,
        function: &Expression,
        args: &[Expression],
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        spread: Option<&Expression>,
    ) -> String {
        if let Some(go_name) = self.get_callee_go_name(function).map(str::to_string) {
            let stages: Vec<Staged> = args.iter().map(|a| self.stage_operand(a)).collect();
            let wrap_to_any = Self::spread_needs_any_wrap(function, spread);
            let args_strings =
                self.sequence_with_spread(output, stages, spread, wrap_to_any, "_arg");
            return format!("{}({})", go_name, args_strings.join(", "));
        }

        let saved = self.emitting_call_callee;
        self.emitting_call_callee = true;
        let mut function_string = self.emit_operand(output, function);
        self.emitting_call_callee = saved;

        if matches!(
            function,
            Expression::Unary {
                operator: UnaryOperator::Deref,
                ..
            }
        ) {
            function_string = format!("({})", function_string);
        }

        let type_args_string =
            self.resolve_call_type_args(function, type_args, call_ty, &mut function_string);

        let pointer_indices = self.get_recursive_enum_pointer_indices(function);

        let fn_param_types: Vec<Type> = match function.get_type().unwrap_forall() {
            Type::Function { params, .. } => params.clone(),
            _ => vec![],
        };

        let (is_go_call, is_prelude_dispatch) = match function.unwrap_parens() {
            Expression::DotAccess { expression, .. } => {
                let is_prelude = matches!(
                    expression.get_type().strip_refs().unwrap_forall(),
                    Type::Nominal { id, .. } if id.starts_with("prelude.")
                );
                (Self::is_go_receiver(expression), is_prelude)
            }
            _ => (false, false),
        };

        let ctx = CallArgsContext {
            fn_param_types: &fn_param_types,
            pointer_indices: &pointer_indices,
            is_go_call,
            is_prelude_dispatch,
            spread,
            wrap_spread_to_any: Self::spread_needs_any_wrap(function, spread),
        };
        let args_strings = self.emit_call_args(output, args, &ctx);

        let call_str = format!(
            "{}{}({})",
            function_string,
            type_args_string,
            args_strings.join(", ")
        );
        let call_str = collapse_fmt_print(&function_string, &args_strings, call_str);

        if let Some(wrapped) = self.wrap_go_array_return(output, function, &call_str) {
            return wrapped;
        }
        call_str
    }

    /// Materialize a Go array-returning call into a variable and reslice it,
    /// so the caller sees a `[]T` slice instead of a fixed-size array.
    /// Skipped in discarded-call contexts via `skip_array_return_wrap`.
    fn wrap_go_array_return(
        &mut self,
        output: &mut String,
        function: &Expression,
        call_str: &str,
    ) -> Option<String> {
        if self.skip_array_return_wrap {
            return None;
        }
        let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = function.unwrap_parens()
        else {
            return None;
        };
        if !Self::is_go_receiver(receiver_expression)
            || !self.has_go_array_return(receiver_expression, member)
        {
            return None;
        }
        let temp = self.fresh_var(Some("arr"));
        self.declare(&temp);
        write_line!(output, "{} := {}", temp, call_str);
        Some(format!("{}[:]", temp))
    }

    fn resolve_call_type_args(
        &mut self,
        function: &Expression,
        type_args: &[Annotation],
        call_ty: Option<&Type>,
        function_string: &mut String,
    ) -> String {
        let mut type_args_string = self.format_type_args_from_annotations(type_args);

        let slot_ty = self.current_slot_expected_ty.clone();

        if type_args_string.is_empty()
            && let Some(inferred) = self.infer_return_only_type_args(function)
        {
            type_args_string = slot_ty
                .as_ref()
                .and_then(|t| self.prelude_container_type_args(t))
                .unwrap_or(inferred);
        }

        if type_args_string.is_empty() && Self::is_prelude_variant_constructor(function) {
            let candidate = call_ty
                .and_then(|t| self.prelude_container_type_args(t))
                .or_else(|| {
                    slot_ty
                        .as_ref()
                        .and_then(|t| self.prelude_container_type_args(t))
                });
            type_args_string = candidate.unwrap_or_default();
        }

        if !type_args_string.is_empty()
            && let Some(bracket_start) = function_string.find('[')
        {
            function_string.truncate(bracket_start);
        }

        type_args_string
    }

    fn emit_call_args(
        &mut self,
        output: &mut String,
        args: &[Expression],
        ctx: &CallArgsContext<'_>,
    ) -> Vec<String> {
        let stages: Vec<Staged> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let mut setup = String::new();
                let value = self.emit_call_arg(&mut setup, arg, i, ctx);
                Staged::new(setup, value)
            })
            .collect();
        self.sequence_with_spread(output, stages, ctx.spread, ctx.wrap_spread_to_any, "_arg")
    }

    fn spread_needs_any_wrap(function: &Expression, spread: Option<&Expression>) -> bool {
        let Some(spread_expr) = spread else {
            return false;
        };
        let Some(variadic_elem) = function.get_type().unwrap_forall().is_variadic() else {
            return false;
        };
        if !variadic_elem.is_unknown() {
            return false;
        }
        spread_expr
            .get_type()
            .inner()
            .is_some_and(|t| !t.is_unknown())
    }

    /// Classify and emit a single call argument.
    fn emit_call_arg(
        &mut self,
        output: &mut String,
        arg: &Expression,
        index: usize,
        ctx: &CallArgsContext<'_>,
    ) -> String {
        let effective_param_ty = self.effective_param_type(index, ctx.fn_param_types);

        if ctx.is_go_call
            && let Some(result) = self.try_emit_callback_wrapper(output, arg, effective_param_ty)
        {
            return result;
        }

        if let Some(result) = self.try_emit_nullable_coercion(output, arg, effective_param_ty) {
            return result;
        }

        if ctx.pointer_indices.contains(&index) {
            let value = self.emit_value(output, arg);
            if matches!(arg, Expression::Reference { .. }) || arg.get_type().is_ref() {
                return value;
            }
            let temp = self.fresh_var(Some("ptr"));
            self.declare(&temp);
            write_line!(output, "{} := {}", temp, value);
            return format!("&{}", temp);
        }

        let suppress = ctx.is_prelude_dispatch
            && effective_param_ty
                .is_some_and(|p| matches!(p.unwrap_forall(), Type::Function { .. }));
        let saved = std::mem::replace(&mut self.suppress_go_fn_short_circuit, suppress);
        let value = self.emit_composite_value(output, arg);
        self.suppress_go_fn_short_circuit = saved;
        match effective_param_ty {
            Some(target) => {
                let coercion = Coercion::resolve(self, &arg.get_type(), target);
                coercion.apply(self, output, value)
            }
            None => value,
        }
    }

    fn effective_param_type<'a>(
        &self,
        index: usize,
        fn_param_types: &'a [Type],
    ) -> Option<&'a Type> {
        fn_param_types.get(index).or_else(|| {
            fn_param_types
                .last()
                .filter(|t| t.get_name() == Some("VarArgs"))
        })
    }

    fn try_emit_callback_wrapper(
        &mut self,
        output: &mut String,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<String> {
        let param_fn_ty = effective_param_ty
            .and_then(|param_ty| self.resolve_to_function_type(param_ty.unwrap_forall()))
            .filter(|fn_ty| {
                let Type::Function { return_type, .. } = fn_ty else {
                    return false;
                };
                return_type.is_result()
                    || return_type.is_option()
                    || return_type.tuple_arity().is_some_and(|a| a >= 2)
            })?;

        let arg_ty = arg.get_type();
        let arg_fn_ty = self.resolve_to_function_type(arg_ty.unwrap_forall());
        if let Some(Type::Function {
            return_type: arg_ret,
            ..
        }) = arg_fn_ty.as_ref()
            && let Type::Function {
                return_type: param_ret,
                ..
            } = &param_fn_ty
            && self.classify_direct_emission(arg_ret).is_some()
            && self.classify_direct_emission(param_ret).is_some()
        {
            return Some(self.emit_value(output, arg));
        }

        let value = self.emit_value(output, arg);
        Some(self.emit_lisette_callback_wrapper(output, &value, &param_fn_ty))
    }

    fn try_emit_nullable_coercion(
        &mut self,
        output: &mut String,
        arg: &Expression,
        effective_param_ty: Option<&Type>,
    ) -> Option<String> {
        let param_ty = effective_param_ty?;
        let arg_ty = arg.get_type();
        if !self.is_nullable_option(&arg_ty) {
            return None;
        }
        let check_ty = if param_ty.get_name() == Some("VarArgs") {
            param_ty.inner().unwrap_or_else(|| param_ty.clone())
        } else {
            param_ty.clone()
        };
        let needs_coercion = self
            .as_interface(&check_ty)
            .is_some_and(|id| go_name::is_go_import(&id))
            || (check_ty.has_name("Unknown") && {
                let inner = arg_ty.ok_type();
                self.as_interface(&inner)
                    .is_some_and(|id| go_name::is_go_import(&id))
            });

        if !needs_coercion {
            return None;
        }

        if matches!(arg, Expression::Identifier { value, .. } if value == "None") {
            return Some("nil".to_string());
        }
        let value = self.emit_value(output, arg);
        let coercion = Coercion::resolve_unwrap_go_nullable(self, &arg.get_type());
        Some(coercion.apply(self, output, value))
    }
}
