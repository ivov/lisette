use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableAbi, CallableParamAbi, CallableReturnAbi};
use crate::abi::layout::SlotOrigin;
use crate::calls::go_interop::go_qualified_name;
use crate::calls::go_interop::is_go_receiver;
use crate::expressions::staging::VariadicCombine;
use crate::types::native::NativeGoType;
use syntax::ast::Expression;
use syntax::program::{CallKind, Definition, DotAccessKind, NativeTypeKind};
use syntax::types::Type;

#[derive(Debug)]
pub(crate) struct CallPlan<'a> {
    pub(crate) resolved: ResolvedCallee<'a>,
    pub(crate) arguments: Vec<ArgumentPlan>,
    pub(crate) result_transition: AbiTransition,
    /// Variadic spread combine: present when the callee accepts a variadic
    /// parameter and the call supplies a trailing spread argument.
    pub(crate) variadic: Option<VariadicSpreadPlan>,
}

/// Canonical identity, signatures, and physical ABI for one callable.
#[derive(Debug)]
pub(crate) struct ResolvedCallee<'a> {
    pub(crate) id: Option<String>,
    pub(crate) origin: CallableOrigin,
    pub(crate) definition: Option<&'a Definition>,
    pub(crate) instantiated: Type,
    pub(crate) declared: Option<Type>,
    pub(crate) receiver_offset: usize,
    pub(crate) abi: CallableAbi,
    pub(crate) is_prelude_dispatch: bool,
}

/// AST-level `CallKind` plus emit-side classification.
#[derive(Debug, Clone)]
pub(crate) enum CallableOrigin {
    /// Regular Lisette function or method call.
    Regular,
    /// Go interop call; `ResolvedCallee::abi` describes its physical boundary.
    GoInterop,
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

/// Per-argument adaptation; first applicable wins.
#[derive(Debug, Clone)]
pub(crate) enum ArgumentPlan {
    /// No special adaptation beyond the final type coercion.
    Direct,
    /// Wrap a function value in a Go callback adapter (Go calls only).
    GoCallbackAdapter {
        source: CallableReturnAbi,
        target: CallableReturnAbi,
        transition: AbiTransition,
    },
    /// Adapt a lowered-return fn-value arg to the callee's expected shape.
    LoweredFnShapeAdapter,
    /// Bridge the Lisette value layout to the parameter slot's physical layout.
    GoSlotBridge,
    /// Lower a tagged Go-function value (prelude-dispatch arg).
    TaggedGoLowering,
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

impl CallPlan<'_> {
    /// Derive a `VariadicCombine` from this plan, given the caller's
    /// `extra_leading` argument count (UFCS adds 1 for the implicit receiver).
    pub(crate) fn variadic_combine(&self, extra_leading: usize) -> Option<VariadicCombine> {
        self.variadic
            .as_ref()
            .map(|spread| spread.combine(extra_leading))
    }
}

impl<'a> Planner<'a> {
    /// Build a `CallPlan` for the given expression. Returns `None` for
    /// non-Call expressions.
    pub(crate) fn plan_call(&self, expression: &Expression) -> Option<CallPlan<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            call_kind,
            spread,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let function = callee.unwrap_parens();
        let variadic = plan_variadic_spread(function, (**spread).as_ref());

        let go_return = self.resolve_go_call_abi(expression);

        let kind = call_kind.filter(|_| !self.is_local_binding(function));

        let callee_plan = if is_go_callable(function) {
            CallableOrigin::GoInterop
        } else {
            match kind {
                Some(CallKind::TupleStructConstructor) => CallableOrigin::TupleStructConstructor,
                Some(CallKind::AssertType) => CallableOrigin::AssertType,
                Some(CallKind::UfcsMethod) => CallableOrigin::UfcsMethod,
                Some(CallKind::NativeConstructor(kind)) => CallableOrigin::NativeConstructor(kind),
                Some(CallKind::NativeMethod(kind)) => CallableOrigin::NativeMethod(kind),
                Some(CallKind::NativeMethodIdentifier(kind)) => {
                    CallableOrigin::NativeMethodIdentifier(kind)
                }
                Some(CallKind::ReceiverMethodUfcs { is_public }) => {
                    CallableOrigin::ReceiverMethodUfcs { is_public }
                }
                None | Some(CallKind::Regular) => CallableOrigin::Regular,
            }
        };

        let resolved = self.resolve_callee(
            function,
            callee_plan.clone(),
            go_return.as_ref(),
            args.len(),
        );
        let callee_diverges = resolved
            .instantiated
            .get_function_ret()
            .is_some_and(Type::is_never);
        let result_transition = if callee_diverges {
            AbiTransition::Identity
        } else {
            resolved
                .abi
                .result
                .transition_to(&self.value_return_abi(ty))
        };
        debug_assert_ne!(
            result_transition,
            AbiTransition::Incompatible,
            "a typed call must preserve its logical result type"
        );
        let arguments = args
            .iter()
            .enumerate()
            .map(|(index, argument)| {
                let param = resolved.abi.param(index);
                self.plan_argument(argument, &resolved, param)
            })
            .collect();

        Some(CallPlan {
            resolved,
            arguments,
            result_transition,
            variadic,
        })
    }

    fn resolve_callee(
        &self,
        function: &Expression,
        origin: CallableOrigin,
        go_return: Option<&CallableReturnAbi>,
        arg_count: usize,
    ) -> ResolvedCallee<'a> {
        let (id, definition) = self.resolve_callee_definition(function);
        let declared = definition.map(|definition| definition.ty().clone());
        let instantiated = self
            .facts
            .resolve_to_function_type(function.get_type().unwrap_forall())
            .unwrap_or_else(|| function.get_type().unwrap_forall().clone());
        let declared_params = declared
            .as_ref()
            .and_then(|ty| ty.unwrap_forall().get_function_params());
        let receiver_offset =
            declared_params.map_or(0, |params| params.len().saturating_sub(arg_count));
        let params = build_param_abi(
            self,
            &instantiated,
            declared_params,
            receiver_offset,
            id.as_deref(),
            &origin,
        );
        let result = match go_return {
            Some(result) => result.clone(),
            None => self
                .classify_callee_abi(function, definition)
                .unwrap_or_else(|| {
                    instantiated
                        .get_function_ret()
                        .map(|return_ty| self.value_return_abi(return_ty))
                        .unwrap_or(CallableReturnAbi::Direct)
                }),
        };
        let return_type = instantiated.get_function_ret().unwrap_or(&Type::Never);
        let return_layout = if matches!(origin, CallableOrigin::GoInterop) {
            id.as_deref()
                .and_then(|id| self.facts.go_callable_return_slot(id))
                .map(|slot| {
                    self.value_layout_with_declaration(
                        return_type,
                        slot.origin,
                        &slot.declared_type,
                    )
                })
                .unwrap_or_else(|| {
                    self.value_layout(return_type, SlotOrigin::go_return(return_type))
                })
        } else {
            self.value_layout(return_type, SlotOrigin::Lisette)
        };
        let is_prelude_dispatch = match function.unwrap_parens() {
            Expression::DotAccess { expression, .. } => {
                let receiver_ty = self.facts.strip_and_peel(&expression.get_type());
                matches!(
                    &receiver_ty,
                    Type::Nominal { id, .. } if id.starts_with("prelude.")
                ) || NativeGoType::from_type(&receiver_ty).is_some()
            }
            Expression::Identifier {
                qualified: Some(qualified),
                ..
            } => qualified.starts_with("prelude."),
            _ => false,
        };

        ResolvedCallee {
            id,
            origin,
            definition,
            instantiated,
            declared,
            receiver_offset,
            abi: CallableAbi {
                params,
                result,
                return_layout,
            },
            is_prelude_dispatch,
        }
    }

    pub(crate) fn resolve_callable_value(
        &self,
        expression: &Expression,
    ) -> Option<ResolvedCallee<'a>> {
        let instantiated = self
            .facts
            .resolve_to_function_type(expression.get_type().unwrap_forall())?;
        let params = instantiated.get_function_params()?;
        let return_ty = instantiated.get_function_ret()?;
        let go_return = self.resolve_go_callee_abi(expression, return_ty);
        let origin = if is_go_callable(expression) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Regular
        };
        Some(self.resolve_callee(expression, origin, go_return.as_ref(), params.len()))
    }

    pub(crate) fn resolve_callee_definition(
        &self,
        function: &Expression,
    ) -> (Option<String>, Option<&'a Definition>) {
        let primary = self.resolve_callee_id(function);
        if let Some(id) = primary.as_deref()
            && let Some(definition) = self.facts.definition(id)
        {
            return (primary, Some(definition));
        }

        match function.unwrap_parens() {
            Expression::Identifier {
                value, binding_id, ..
            } if binding_id.is_none() => {
                let qualified = self.facts.qualified_current(value);
                if let Some(definition) = self.facts.definition(&qualified) {
                    return (Some(qualified), Some(definition));
                }
                if let Some(definition) = self.facts.definition(value) {
                    return (Some(value.to_string()), Some(definition));
                }
            }
            Expression::DotAccess {
                dot_access_kind:
                    Some(
                        DotAccessKind::StructField { .. }
                        | DotAccessKind::TupleStructField { .. }
                        | DotAccessKind::TupleElement,
                    ),
                ..
            } => {}
            Expression::DotAccess {
                expression, member, ..
            } => {
                if let Expression::Identifier { value, .. } = expression.as_ref() {
                    let module = self.module.module_for_alias(value).unwrap_or(value);
                    let qualified = format!("{module}.{member}");
                    if let Some(definition) = self.facts.definition(&qualified) {
                        return (Some(qualified), Some(definition));
                    }
                    let local = self.facts.qualified_current_member(value, member);
                    if let Some(definition) = self.facts.definition(&local) {
                        return (Some(local), Some(definition));
                    }
                }
                if let Expression::DotAccess {
                    expression: inner,
                    member: type_name,
                    ..
                } = expression.as_ref()
                    && let Expression::Identifier { value: module, .. } = inner.as_ref()
                {
                    let module = self.module.module_for_alias(module).unwrap_or(module);
                    let qualified = format!("{module}.{type_name}.{member}");
                    if let Some(definition) = self.facts.definition(&qualified) {
                        return (Some(qualified), Some(definition));
                    }
                }

                let receiver = self.facts.strip_and_peel(&expression.get_type());
                if let Some(native) = NativeGoType::from_type(&receiver) {
                    let qualified = format!("prelude.{}.{}", native.lisette_name(), member);
                    if let Some(definition) = self.facts.definition(&qualified) {
                        return (Some(qualified), Some(definition));
                    }
                }
            }
            _ => {}
        }

        (primary, None)
    }

    pub(crate) fn resolve_callable_params(
        &self,
        function: &Expression,
        arg_count: usize,
    ) -> Vec<CallableParamAbi> {
        let (id, definition) = self.resolve_callee_definition(function);
        let declared = definition.map(Definition::ty);
        let declared_params = declared.and_then(|ty| ty.unwrap_forall().get_function_params());
        let receiver_offset =
            declared_params.map_or(0, |params| params.len().saturating_sub(arg_count));
        let instantiated = self
            .facts
            .resolve_to_function_type(function.get_type().unwrap_forall())
            .unwrap_or_else(|| function.get_type().unwrap_forall().clone());
        let origin = if is_go_callable(function) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Regular
        };
        build_param_abi(
            self,
            &instantiated,
            declared_params,
            receiver_offset,
            id.as_deref(),
            &origin,
        )
    }

    fn resolve_callee_id(&self, function: &Expression) -> Option<String> {
        match function.unwrap_parens() {
            Expression::Identifier {
                value,
                qualified,
                binding_id,
                ..
            } => qualified.as_deref().map(str::to_string).or_else(|| {
                binding_id
                    .is_none()
                    .then(|| self.facts.qualified_current(value))
            }),
            Expression::DotAccess {
                dot_access_kind:
                    Some(
                        DotAccessKind::StructField { .. }
                        | DotAccessKind::TupleStructField { .. }
                        | DotAccessKind::TupleElement,
                    ),
                ..
            } => None,
            Expression::DotAccess {
                expression, member, ..
            } => go_qualified_name(expression, member).or_else(|| {
                let receiver = self.facts.strip_and_peel(&expression.get_type());
                match receiver {
                    Type::Nominal { id, .. } => Some(format!("{id}.{member}")),
                    _ => None,
                }
            }),
            _ => None,
        }
    }

    /// Lowered shape of a callee. Type-driven, so it fires regardless of
    /// whether the callee is a direct ref, local, parameter, or field.
    fn classify_callee_abi(
        &self,
        callee: &Expression,
        definition: Option<&Definition>,
    ) -> Option<CallableReturnAbi> {
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
            // Peel so an alias over a native type stays native, else its methods
            // get a second tagged-return wrap at the call site.
            if NativeGoType::from_type(&self.facts.strip_and_peel(&receiver_ty)).is_some()
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
        let declared_return =
            definition.and_then(|definition| definition.ty().unwrap_forall().get_function_ret());
        let classify_ty = declared_return.unwrap_or(f.return_type.as_ref());

        self.classify_direct_emission(classify_ty)
    }

    /// Resolve a Go-interop call's strategy.
    pub(crate) fn resolve_go_call_abi(&self, expression: &Expression) -> Option<CallableReturnAbi> {
        let Expression::Call {
            expression: callee,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        self.resolve_go_callee_abi(callee, ty)
    }

    fn resolve_go_callee_abi(
        &self,
        callee: &Expression,
        return_ty: &Type,
    ) -> Option<CallableReturnAbi> {
        let inner = callee.unwrap_parens();
        if let Expression::DotAccess {
            expression: receiver_expression,
            member,
            ..
        } = inner
            && is_go_receiver(receiver_expression)
        {
            if let Some(qualified_name) = go_qualified_name(receiver_expression, member)
                && self
                    .facts
                    .go_callable_return_slot(&qualified_name)
                    .is_some()
            {
                return self.facts.go_callable_return(&qualified_name).cloned();
            }
            let go_hints = go_qualified_name(receiver_expression, member)
                .and_then(|name| self.facts.definition(name.as_str()))
                .map(|d| d.go_hints())
                .unwrap_or_default();
            return self.facts.classify_go_return_type(return_ty, go_hints);
        }

        None
    }
}

fn build_param_abi(
    planner: &Planner<'_>,
    instantiated: &Type,
    declared: Option<&[Type]>,
    receiver_offset: usize,
    callee_id: Option<&str>,
    callable_origin: &CallableOrigin,
) -> Vec<CallableParamAbi> {
    instantiated
        .get_function_params()
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .map(|(index, instantiated)| {
            let declared = declared
                .and_then(|params| params.get(receiver_offset + index))
                .cloned();
            let catalog_slot = if matches!(callable_origin, CallableOrigin::GoInterop) {
                callee_id.and_then(|id| {
                    planner
                        .facts
                        .go_callable_parameter(id, receiver_offset + index)
                })
            } else {
                None
            };
            let origin = catalog_slot.map_or_else(
                || {
                    if matches!(callable_origin, CallableOrigin::GoInterop) {
                        SlotOrigin::go_parameter(declared.as_ref().unwrap_or(instantiated))
                    } else {
                        SlotOrigin::Lisette
                    }
                },
                |slot| slot.origin,
            );
            let layout = catalog_slot
                .map(|slot| {
                    planner.value_layout_with_declaration(instantiated, origin, &slot.declared_type)
                })
                .or_else(|| {
                    declared.as_ref().map(|declared| {
                        planner.value_layout_with_declaration(instantiated, origin, declared)
                    })
                })
                .unwrap_or_else(|| planner.value_layout(instantiated, origin));
            CallableParamAbi {
                instantiated: instantiated.clone(),
                declared,
                origin,
                layout,
            }
        })
        .collect()
}

fn receiver_is_prelude_type(ty: &Type) -> bool {
    matches!(
        ty.strip_refs().unwrap_forall(),
        Type::Nominal { id, .. } if id.starts_with("prelude.")
    )
}

fn is_go_callable(expression: &Expression) -> bool {
    matches!(
        expression.unwrap_parens(),
        Expression::DotAccess { expression, .. } if is_go_receiver(expression)
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
