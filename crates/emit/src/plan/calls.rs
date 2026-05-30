use crate::GoCallStrategy;
use crate::Planner;
use crate::abi::AbiShape;
use crate::calls::go_interop::go_qualified_name;
use crate::calls::go_interop::is_go_receiver;
use crate::expressions::staging::VariadicCombine;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;
use syntax::program::{CallKind, NativeTypeKind};
use syntax::types::Type;

#[derive(Debug)]
pub(crate) struct CallPlan {
    pub(crate) callee: CalleePlan,
    pub(crate) return_shape: CallReturnShape,
    /// Call-level wrapping (variadic spread, array-return wrapper).
    pub(crate) wrapper: WrapperPlan,
}

/// AST-level `CallKind` plus emit-side classification.
#[derive(Debug)]
pub(crate) enum CalleePlan {
    /// Regular Lisette function or method call.
    Regular,
    /// Go interop call. `strategy` describes argument/return handling.
    GoInterop(GoCallStrategy),
    /// UFCS method call: `receiver.method()` where `method` is a free function.
    UfcsMethod,
    /// Native type constructor: `Channel.new(...)`, `Map.new(...)`.
    NativeConstructor(NativeTypeKind),
    /// Native instance method via dot access: `slice.append(x)`.
    NativeMethod(NativeTypeKind),
    /// Native method via identifier: `Slice.contains(s, x)`.
    NativeMethodIdentifier(NativeTypeKind),
    /// Receiver method via UFCS syntax: `Type.method(receiver, args)`.
    ReceiverMethodUfcs { is_public: bool },
    /// Tuple struct constructor: `Point(1, 2)`.
    TupleStructConstructor,
    /// Type assertion: `assert_type<T>(x)`.
    AssertType,
}

/// Return-shape adaptation at the call boundary (Go interop is encoded in
/// `CalleePlan`).
#[derive(Debug, Clone)]
pub(crate) enum CallReturnShape {
    /// No adaptation: rendered call result is used as-is.
    Direct,
    /// Lisette callee returns a lowered ABI shape.
    Lowered(AbiShape),
}

/// Per-argument adaptation; first applicable wins.
#[derive(Debug, Clone)]
pub(crate) enum ArgumentPlan {
    /// No special adaptation beyond the final type coercion.
    Direct,
    /// Wrap a function value in a Go callback adapter (Go calls only).
    GoCallbackAdapter(CallbackWrapperKind),
    /// Adapt a lowered-return fn-value arg to the callee's expected shape.
    LoweredFnShapeAdapter,
    /// Nullable coercion: `Option<Ref<T>>` → `*T` or bare `T`.
    NullableCoercion(NullableCoerceKind),
    /// Unwrap a Go pointer parameter at the call site (`&x` or `*x`).
    GoPointerUnwrap,
    /// `&` wrap for recursive enum constructor arguments.
    RecursiveEnumPointer,
    /// Lower a tagged Go-function value (prelude-dispatch arg).
    TaggedGoLowering,
}

/// Sub-variants of `ArgumentPlan::NullableCoercion`. The detection path picks
/// which branch fires; the emitter applies it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NullableCoerceKind {
    /// Source `Option<T>` argument flowing into an `unknown`-typed parameter:
    /// emit `nil` (for `None`) or coerce the option value.
    OptionToUnknown,
    /// Nullable `Option` argument flowing into a Go-imported interface: unwrap
    /// the option and apply the Go-boundary coercion.
    NullableInterface,
}

/// Sub-variants of `ArgumentPlan::GoCallbackAdapter`. `Identity` is the
/// no-op case where source and target callback ABIs already agree.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CallbackWrapperKind {
    /// Both arg and param callback returns are already lowered; pass through.
    Identity,
    /// Wrap the Lisette callback so its tagged returns marshal into the
    /// callee's expected lowered ABI.
    Wrap,
}

/// Call-level wrapping (variadic spread, Go-array return).
#[derive(Debug, Clone, Default)]
pub(crate) struct WrapperPlan {
    /// Variadic spread combine: present when the callee accepts a variadic
    /// parameter and the call supplies a trailing spread argument.
    pub(crate) variadic: Option<VariadicSpreadPlan>,
    /// True when the callee is a Go function whose declared return is a
    /// slice/array. Renderers still gate on `ctx.keeps_raw_go_array_return()`
    /// before applying the slice wrap.
    pub(crate) go_array_return: bool,
}

/// Variadic spread combine: a trailing spread argument must be combined
/// with fixed args via the variadic boundary helper.
#[derive(Debug, Clone)]
pub(crate) struct VariadicSpreadPlan {
    /// Element type of the variadic parameter.
    pub(crate) element_ty: Type,
    /// Count of fixed parameters in the callee's signature (excluding the
    /// trailing variadic). Callers add their own `extra_leading` to derive
    /// the per-call fixed count.
    pub(crate) fixed_in_signature: usize,
}

impl VariadicSpreadPlan {
    /// Derive a `VariadicCombine`, given the caller's `extra_leading` argument
    /// count (UFCS adds 1 for the implicit receiver).
    pub(crate) fn combine(&self, extra_leading: usize) -> VariadicCombine {
        VariadicCombine {
            element_ty: self.element_ty.clone(),
            fixed_count: self.fixed_in_signature + extra_leading,
        }
    }
}

impl CallPlan {
    /// Derive a `VariadicCombine` from this plan, given the caller's
    /// `extra_leading` argument count (UFCS adds 1 for the implicit receiver).
    pub(crate) fn variadic_combine(&self, extra_leading: usize) -> Option<VariadicCombine> {
        self.wrapper
            .variadic
            .as_ref()
            .map(|spread| spread.combine(extra_leading))
    }

    /// True when the callee is a Go function returning a slice/array; the
    /// renderer still gates on its expression-context before wrapping.
    pub(crate) fn has_go_array_return(&self) -> bool {
        self.wrapper.go_array_return
    }
}

impl Planner<'_> {
    /// Build a `CallPlan` for the given expression. Returns `None` for
    /// non-Call expressions.
    pub(crate) fn plan_call(&self, expression: &Expression) -> Option<CallPlan> {
        let Expression::Call {
            expression: callee,
            call_kind,
            spread,
            ..
        } = expression
        else {
            return None;
        };

        let function = callee.unwrap_parens();
        let wrapper = self.plan_call_wrapper(function, (**spread).as_ref());

        if let Some(strategy) = self.resolve_go_call_strategy(expression) {
            return Some(CallPlan {
                callee: CalleePlan::GoInterop(strategy),
                return_shape: CallReturnShape::Direct,
                wrapper,
            });
        }

        let kind = call_kind.filter(|_| !self.is_local_binding(function));

        let callee_plan = match kind {
            Some(CallKind::TupleStructConstructor) => CalleePlan::TupleStructConstructor,
            Some(CallKind::AssertType) => CalleePlan::AssertType,
            Some(CallKind::UfcsMethod) => CalleePlan::UfcsMethod,
            Some(CallKind::NativeConstructor(kind)) => CalleePlan::NativeConstructor(kind),
            Some(CallKind::NativeMethod(kind)) => CalleePlan::NativeMethod(kind),
            Some(CallKind::NativeMethodIdentifier(kind)) => {
                CalleePlan::NativeMethodIdentifier(kind)
            }
            Some(CallKind::ReceiverMethodUfcs { is_public }) => {
                CalleePlan::ReceiverMethodUfcs { is_public }
            }
            None | Some(CallKind::Regular) => CalleePlan::Regular,
        };

        let return_shape = match self.classify_callee_abi(callee) {
            Some(shape) => CallReturnShape::Lowered(shape),
            None => CallReturnShape::Direct,
        };

        Some(CallPlan {
            callee: callee_plan,
            return_shape,
            wrapper,
        })
    }

    /// Plan call-level wrapping. Detects variadic spread (if a trailing
    /// spread argument is supplied) and the Go-array-return hint (the
    /// renderer still gates on its expression context before wrapping).
    fn plan_call_wrapper(&self, function: &Expression, spread: Option<&Expression>) -> WrapperPlan {
        let variadic = plan_variadic_spread(function, spread);
        let mut go_array_return = false;
        if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = function.unwrap_parens()
            && is_go_receiver(receiver_expression)
            && self.has_go_array_return(receiver_expression, member)
        {
            go_array_return = true;
        }
        WrapperPlan {
            variadic,
            go_array_return,
        }
    }

    /// Lowered shape of a callee. Type-driven, so it fires regardless of
    /// whether the callee is a direct ref, local, parameter, or field.
    pub(crate) fn classify_callee_abi(&self, callee: &Expression) -> Option<AbiShape> {
        let callee_ty = callee.get_type();
        let unwrapped = callee_ty.unwrap_forall();
        let resolved = self
            .facts
            .resolve_to_function_type(unwrapped)
            .unwrap_or_else(|| unwrapped.clone());
        let Type::Function(f) = resolved else {
            return None;
        };
        let inner = callee.unwrap_parens();
        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
        {
            if is_go_receiver(receiver) {
                return None;
            }
            // Methods on native types (`xs.find(f)`) and prelude types
            // (`r.map(f)`, `opt.map(f)`) dispatch to Lisette-prelude
            // helpers whose Go signatures keep the tagged return — no
            // lowering at the call site.
            let receiver_ty = receiver.get_type();
            if NativeGoType::from_type(&receiver_ty).is_some()
                || receiver_is_prelude_type(&receiver_ty)
            {
                return None;
            }
            // Type-namespace dispatch like `Option.map(opt, f)` — prelude helper, tagged return.
            if matches!(
                &**receiver,
                Expression::Identifier { qualified: Some(q), .. } if q.starts_with("prelude.")
            ) {
                return None;
            }
        }
        // Tagged-type constructors compile to `lisette.MakeX(...)`,
        // not multi-return Go calls.
        if inner.as_result_constructor().is_some()
            || inner.as_option_constructor().is_some()
            || inner.as_partial_constructor().is_some()
        {
            return None;
        }
        // Prelude function refs (`assert_type(x)`) — prelude helper, tagged return.
        if let Expression::Identifier {
            qualified: Some(q), ..
        } = inner
            && q.starts_with("prelude.")
        {
            return None;
        }
        let declared_return = self
            .callee_definition(callee)
            .and_then(|definition| definition.ty().unwrap_forall().get_function_ret());
        let classify_ty = declared_return.unwrap_or(f.return_type.as_ref());

        self.classify_direct_emission(classify_ty)
    }

    /// Resolve a Go-interop call's strategy.
    pub(crate) fn resolve_go_call_strategy(
        &self,
        expression: &Expression,
    ) -> Option<GoCallStrategy> {
        let Expression::Call {
            expression: callee,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let inner = callee.unwrap_parens();

        if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = inner
            && is_go_receiver(receiver_expression)
        {
            if let Some(qualified_name) = go_qualified_name(receiver_expression, member)
                && let Some(strategy) = self.facts.go_call_strategy(&qualified_name)
            {
                return Some(strategy.clone());
            }
            let go_hints = go_qualified_name(receiver_expression, member)
                .and_then(|name| self.facts.definition(name.as_str()))
                .map(|d| d.go_hints())
                .unwrap_or_default();
            return self.facts.classify_go_return_type(ty, go_hints);
        }

        None
    }
}

fn receiver_is_prelude_type(ty: &Type) -> bool {
    matches!(
        ty.strip_refs().unwrap_forall(),
        Type::Nominal { id, .. } if id.starts_with("prelude.")
    )
}

/// Plan a variadic spread: present when the callee accepts a variadic
/// parameter and the call supplies a trailing spread argument.
pub(crate) fn plan_variadic_spread(
    function: &Expression,
    spread: Option<&Expression>,
) -> Option<VariadicSpreadPlan> {
    spread?;
    let fn_ty = function.get_type();
    let unwrapped = fn_ty.unwrap_forall();
    let element_ty = unwrapped.is_variadic()?;
    let fixed_in_signature = unwrapped.get_function_params()?.len().saturating_sub(1);
    Some(VariadicSpreadPlan {
        element_ty,
        fixed_in_signature,
    })
}
