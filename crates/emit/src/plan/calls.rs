use crate::Planner;
use crate::abi::callable::{AbiTransition, CallableAbi, CallableParamAbi, CallableReturnAbi};
use crate::abi::coercion::LayoutBridge;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::definitions::interface_adapter::with_comma_ok_hint;
use crate::patterns::matching::prelude_constructor;
use syntax::ast::{Expression, IdentifierResolution};
use syntax::program::NativeTypeKind;
use syntax::program::{
    CallKind, Definition, DotAccessResolution, Method, Visibility, resolved_definition,
};
use syntax::types::{CompoundKind, FunctionParameter, Type};

#[derive(Debug)]
pub(crate) struct CallPlan<'a> {
    pub(crate) resolved: ResolvedCallee<'a>,
    pub(crate) arguments: Vec<ArgumentPlan>,
    pub(crate) result_transition: AbiTransition,
}

#[derive(Debug)]
pub(crate) struct ResolvedCallee<'a> {
    pub(crate) origin: CallableOrigin,
    pub(crate) declaration: Option<CallableDeclaration<'a>>,
    pub(crate) instantiated: Type,
    pub(crate) receiver_offset: usize,
    pub(crate) abi: CallableAbi,
    pub(crate) is_prelude_dispatch: bool,
}

impl ResolvedCallee<'_> {
    pub(crate) fn declared_type(&self) -> Option<&Type> {
        self.declaration.map(CallableDeclaration::ty)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallableDeclaration<'a> {
    Definition(&'a Definition),
    Method(&'a Method),
}

impl<'a> CallableDeclaration<'a> {
    pub(crate) fn ty(self) -> &'a Type {
        match self {
            Self::Definition(definition) => &definition.ty,
            Self::Method(method) => &method.ty,
        }
    }

    pub(crate) fn visibility(self) -> &'a Visibility {
        match self {
            Self::Definition(definition) => &definition.visibility,
            Self::Method(method) => &method.visibility,
        }
    }

    pub(crate) fn is_type_definition(self) -> bool {
        matches!(self, Self::Definition(definition) if definition.is_type_definition())
    }

    pub(crate) fn go_type_param_recipe(self) -> Option<&'a str> {
        match self {
            Self::Definition(definition) => definition.go_type_param_recipe(),
            Self::Method(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallableOrigin {
    /// Go interop call; `ResolvedCallee::abi` describes its physical boundary.
    GoInterop,
    /// Lisette call. Local bindings and unresolved callees are `CallKind::Regular`.
    Source(CallKind),
}

#[derive(Debug, Clone)]
pub(crate) enum ArgumentPlan {
    Direct,
    GoCallbackAdapter {
        source: CallableReturnAbi,
        target: CallableReturnAbi,
        transition: AbiTransition,
    },
    LoweredFnShapeAdapter(Box<FunctionArgumentAdapter>),
    GoSlotBridge(Box<ArgumentSlotBridge>),
    TaggedGoLowering,
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionArgumentAdapter {
    pub(crate) source_function: Type,
    pub(crate) source_abi: CallableReturnAbi,
    pub(crate) target_abi: CallableReturnAbi,
    pub(crate) target_origin: SlotOrigin,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentValueSource {
    Lisette,
    GoPhysical,
}

#[derive(Debug, Clone)]
pub(crate) struct ArgumentSlotBridge {
    pub(crate) target: ValueLayout,
    pub(crate) bridge: LayoutBridge,
    pub(crate) source: ArgumentValueSource,
}

impl<'a> Planner<'a> {
    /// Build a `CallPlan` for the given expression. Returns `None` for
    /// non-Call expressions.
    pub(crate) fn plan_call(&self, expression: &Expression) -> Option<CallPlan<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            call_kind,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let function = callee.unwrap_parens();
        let go_return = self.resolve_go_call_abi(expression);

        let origin = if self.is_go_callable(function) {
            CallableOrigin::GoInterop
        } else if self.is_local_binding(function) || *call_kind == CallKind::Unresolved {
            CallableOrigin::Source(CallKind::Regular)
        } else {
            CallableOrigin::Source(*call_kind)
        };

        let resolved = self.resolve_callee(function, origin, go_return.as_ref(), args.len());
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
        })
    }

    fn resolve_callee(
        &self,
        function: &Expression,
        origin: CallableOrigin,
        go_return: Option<&CallableReturnAbi>,
        arg_count: usize,
    ) -> ResolvedCallee<'a> {
        let (id, declaration) = self.resolve_callee_definition(function);
        let declared_type = declaration.map(CallableDeclaration::ty);
        let instantiated = self
            .facts
            .resolve_to_function_type(function.get_type().unwrap_forall())
            .unwrap_or_else(|| function.get_type().unwrap_forall().clone());
        let declared_params = declared_type.and_then(|ty| ty.unwrap_forall().get_function_params());
        let receiver_offset = receiver_offset(declared_params, &instantiated, arg_count);
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
                .classify_callee_abi(function, id.as_deref(), declaration)
                .unwrap_or_else(|| {
                    instantiated
                        .get_function_ret()
                        .map(|return_ty| self.value_return_abi(return_ty))
                        .unwrap_or(CallableReturnAbi::Direct)
                }),
        };
        let return_type = instantiated.get_function_ret().unwrap_or(&Type::Never);
        let declared_return = declared_type.and_then(|ty| ty.unwrap_forall().get_function_ret());
        let catalog_return = matches!(origin, CallableOrigin::GoInterop)
            .then(|| {
                id.as_deref()
                    .and_then(|id| self.facts.go_callable_return_slot(id))
            })
            .flatten();
        let return_origin = if matches!(origin, CallableOrigin::GoInterop) {
            catalog_return.map_or_else(
                || SlotOrigin::go_return(self.facts.resolves_to_unknown(return_type)),
                |slot| slot.origin,
            )
        } else {
            SlotOrigin::Lisette
        };
        let return_declaration = catalog_return
            .map(|slot| &slot.declared_type)
            .or(declared_return);
        let return_layout = return_declaration.map_or_else(
            || self.value_layout(return_type, return_origin),
            |declaration| {
                self.value_layout_with_declaration(return_type, return_origin, declaration)
            },
        );
        let is_prelude_dispatch = id
            .as_deref()
            .is_some_and(|definition| definition.starts_with("prelude."))
            || matches!(
                origin,
                CallableOrigin::Source(
                    CallKind::NativeConstructor(_)
                        | CallKind::NativeMethod(_)
                        | CallKind::NativeMethodIdentifier(_)
                )
            );

        ResolvedCallee {
            origin,
            declaration,
            instantiated,
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
        let origin = if self.is_go_callable(expression) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Source(CallKind::Regular)
        };
        Some(self.resolve_callee(expression, origin, go_return.as_ref(), params.len()))
    }

    pub(crate) fn resolve_callee_definition(
        &self,
        function: &Expression,
    ) -> (Option<String>, Option<CallableDeclaration<'a>>) {
        let id = resolved_definition(function).map(str::to_string);
        let declaration = id.as_deref().and_then(|id| self.callable_declaration(id));
        (id, declaration)
    }

    pub(crate) fn callable_declaration(&self, id: &str) -> Option<CallableDeclaration<'a>> {
        self.facts
            .definition(id)
            .map(CallableDeclaration::Definition)
            .or_else(|| {
                let (owner, name) = id.rsplit_once('.')?;
                self.facts
                    .method(owner, name)
                    .map(CallableDeclaration::Method)
            })
    }

    /// Lowered shape of a callee. Type-driven, so it fires regardless of
    /// whether the callee is a direct ref, local, parameter, or field.
    fn classify_callee_abi(
        &self,
        callee: &Expression,
        id: Option<&str>,
        declaration: Option<CallableDeclaration<'a>>,
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
        let callee_definition = resolved_definition(callee);
        if callee_definition.is_some_and(|definition| definition.starts_with("go:")) {
            return None;
        }
        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
        {
            let receiver_type = receiver.get_type();
            if NativeTypeKind::from_type(&self.facts.strip_and_peel(&receiver_type)).is_some()
                || receiver_is_prelude_type(&receiver_type)
                || matches!(
                    &**receiver,
                    Expression::Identifier {
                        resolution: IdentifierResolution::Definition { name: definition, .. },
                        ..
                    }
                        if definition.starts_with("prelude.")
                )
            {
                return None;
            }
        } else if callee_definition.is_some_and(|definition| definition.starts_with("prelude.")) {
            return None;
        }
        // Tagged-type constructors compile to `lisette.MakeX(...)`,
        // not multi-return Go calls.
        if prelude_constructor(inner).is_some() {
            return None;
        }
        if self.callee_uses_tagged_method_return(callee) {
            return None;
        }
        let declared_return =
            declaration.and_then(|declaration| declaration.ty().unwrap_forall().get_function_ret());
        let classify_ty = declared_return.unwrap_or(f.return_type.as_ref());
        let origin = self.function_type_origin(&callee_ty, SlotOrigin::Lisette);
        let abi = self.classify_slot_emission(classify_ty, origin)?;

        // Interface methods carry `#[go(...)]` hints the call must read.
        if let Some(CallableDeclaration::Method(method)) = declaration
            && id
                .and_then(|id| id.rsplit_once('.'))
                .and_then(|(owner, _)| self.facts.definition(owner))
                .is_some_and(Definition::is_interface)
        {
            return Some(with_comma_ok_hint(abi, method));
        }
        Some(abi)
    }

    pub(crate) fn callee_uses_tagged_method_return(&self, callee: &Expression) -> bool {
        let callee_definition = resolved_definition(callee);
        if callee_definition.is_some_and(|id| {
            id.starts_with("go:")
                || id.starts_with("prelude.")
                || id
                    .rsplit_once('.')
                    .is_some_and(|(owner, name)| self.facts.is_ufcs_method(owner, name))
        }) {
            return false;
        }
        let method_name = match callee.unwrap_parens() {
            Expression::DotAccess {
                expression: receiver,
                member,
                resolution,
                ..
            } if matches!(
                resolution,
                DotAccessResolution::InstanceMethod { .. }
                    | DotAccessResolution::InstanceMethodValue { .. }
            ) && NativeTypeKind::from_type(
                &self.facts.strip_and_peel(&receiver.get_type()),
            )
            .is_none()
                && !receiver_is_prelude_type(&receiver.get_type()) =>
            {
                Some(member.as_str())
            }
            Expression::Identifier { .. } => callee_definition.and_then(|id| {
                let (owner, name) = id.rsplit_once('.')?;
                self.facts.method(owner, name)?;
                Some(name)
            }),
            _ => None,
        };
        method_name.is_some_and(|name| self.facts.method_uses_tagged_return(name))
    }

    /// Resolve a Go-interop call's strategy.
    fn resolve_go_call_abi(&self, expression: &Expression) -> Option<CallableReturnAbi> {
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
        let qualified_name = resolved_definition(callee)?;
        if !qualified_name.starts_with("go:") {
            return None;
        }
        if self.facts.go_callable_return_slot(qualified_name).is_some() {
            return self.facts.go_callable_return(qualified_name).cloned();
        }
        let go_hints = self
            .facts
            .definition(qualified_name)
            .map(Definition::go_hints)
            .or_else(|| {
                let (owner, name) = qualified_name.rsplit_once('.')?;
                self.facts
                    .method(owner, name)
                    .map(|method| method.go_hints.as_slice())
            })
            .unwrap_or_default();
        self.facts.classify_go_return_type(return_ty, go_hints)
    }

    pub(crate) fn is_go_callable(&self, expression: &Expression) -> bool {
        resolved_definition(expression).is_some_and(|definition| definition.starts_with("go:"))
    }

    pub(crate) fn call_target_is_go(&self, expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Call { expression: callee, .. }
                if self.is_go_callable(callee.unwrap_parens())
        )
    }
}

fn receiver_is_prelude_type(ty: &Type) -> bool {
    matches!(
        ty.strip_refs().unwrap_forall(),
        Type::Nominal { id, .. } if id.starts_with("prelude.")
    )
}

/// Declared parameters the receiver takes up, a variadic parameter counted once.
fn receiver_offset(
    declared_params: Option<&[FunctionParameter]>,
    instantiated: &Type,
    arg_count: usize,
) -> usize {
    let Some(declared) = declared_params else {
        return 0;
    };
    let supplied = instantiated
        .get_function_params()
        .map_or(arg_count, <[FunctionParameter]>::len);
    declared.len().saturating_sub(supplied)
}

fn build_param_abi(
    planner: &Planner<'_>,
    instantiated: &Type,
    declared: Option<&[FunctionParameter]>,
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
                .map(|param| param.ty.clone());
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
                        SlotOrigin::go_parameter(
                            planner
                                .facts
                                .resolves_to_unknown(declared.as_ref().unwrap_or(&instantiated.ty)),
                        )
                    } else {
                        SlotOrigin::Lisette
                    }
                },
                |slot| slot.origin,
            );
            let layout = catalog_slot
                .map(|slot| {
                    planner.value_layout_with_declaration(
                        &instantiated.ty,
                        origin,
                        &slot.declared_type,
                    )
                })
                .or_else(|| {
                    declared.as_ref().map(|declared| {
                        planner.value_layout_with_declaration(&instantiated.ty, origin, declared)
                    })
                })
                .unwrap_or_else(|| planner.value_layout(&instantiated.ty, origin));
            let element = instantiated
                .ty
                .is_native(CompoundKind::VarArgs)
                .then(|| instantiated.ty.inner())
                .flatten();
            let Some(element) = element else {
                return CallableParamAbi {
                    instantiated: instantiated.ty.clone(),
                    declared,
                    origin,
                    layout,
                    variadic: None,
                };
            };
            let declared_element = declared.map(|declared| {
                if declared.is_native(CompoundKind::VarArgs) {
                    declared.inner().unwrap_or(declared)
                } else {
                    declared
                }
            });
            let element_layout = declared_element.as_ref().map_or_else(
                || planner.value_layout(&element, origin),
                |declared| planner.value_layout_with_declaration(&element, origin, declared),
            );
            CallableParamAbi {
                instantiated: element,
                declared: declared_element,
                origin,
                layout: element_layout,
                variadic: Some(layout),
            }
        })
        .collect()
}
