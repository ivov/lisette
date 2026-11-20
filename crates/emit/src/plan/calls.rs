use crate::EmitFacts;
use crate::Planner;
use crate::abi::ReturnSlot;
use crate::abi::callable::{CallableAbi, CallableParamAbi, CallableReturnAbi, LoweredReturnAbi};
use crate::abi::coercion::LayoutBridge;
use crate::abi::is_prelude_container_type;
use crate::abi::layout::{FunctionLayout, ValueLayout};
use crate::names::go_name;
use crate::patterns::matching::prelude_constructor;
use syntax::ast::{Expression, IdentifierResolution};
use syntax::program::GoTypeRecipe;
use syntax::program::NativeTypeKind;
use syntax::program::SlotOrigin;
use syntax::program::{
    CallOperation, CheckedCallee, Definition, Method, Visibility, resolved_definition,
};
use syntax::types::{CompoundKind, FunctionParameter, Type};

#[derive(Debug)]
pub(crate) struct CallPlan<'a> {
    pub(crate) resolved: ResolvedCallee<'a>,
    pub(crate) arguments: Vec<ArgumentPlan>,
}

impl CallPlan<'_> {
    pub(crate) fn wraps_result(&self) -> bool {
        self.resolved.abi.result.is_lowered()
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedCallee<'a> {
    pub(crate) origin: CallableOrigin<'a>,
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

    pub(crate) fn go_type_param_recipe(self) -> Option<&'a [GoTypeRecipe]> {
        match self {
            Self::Definition(definition) => definition.go_type_param_recipe(),
            Self::Method(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CallableOrigin<'a> {
    GoInterop,
    Source(&'a CallOperation),
}

#[derive(Debug, Clone)]
pub(crate) enum ArgumentPlan {
    Direct,
    Bridge(Box<ArgumentSlotBridge>),
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionBridge {
    pub(crate) source: FunctionLayout,
    pub(crate) target: FunctionLayout,
}

impl FunctionBridge {
    pub(crate) fn into_argument(self, function_type: Type) -> ArgumentSlotBridge {
        ArgumentSlotBridge {
            target: ValueLayout::Function {
                function_type,
                layout: self.target.clone(),
            },
            bridge: LayoutBridge::Function {
                source: Box::new(self.source),
                target: Box::new(self.target),
            },
            source: ArgumentValueSource::Callable,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ArgumentValueSource {
    Lisette,
    GoPhysical,
    GoSlot,

    Callable,
}

#[derive(Debug, Clone)]
pub(crate) struct ArgumentSlotBridge {
    pub(crate) target: ValueLayout,
    pub(crate) bridge: LayoutBridge,
    pub(crate) source: ArgumentValueSource,
}

impl<'a> Planner<'a> {
    pub(crate) fn plan_call(&self, expression: &Expression) -> Option<CallPlan<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            ty,
            ..
        } = expression
        else {
            return None;
        };

        let function = callee.unwrap_parens();
        let go_return = self.resolve_go_call_abi(expression);

        let checked = self.facts.checked(function).callee();
        let origin = if self.is_go_callable(function) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Source(&checked.operation)
        };
        let definition = checked.definition.as_ref().map(ToString::to_string);

        let resolved = self.resolve_callee(
            function,
            origin,
            definition,
            go_return.as_ref(),
            checked.receiver_offset(),
        );
        debug_assert!(
            !resolved.abi.result.is_lowered() || {
                let peeled = self.facts.peel_alias(ty);
                is_prelude_container_type(&peeled)
                    || peeled.tuple_arity().is_some_and(|arity| arity >= 2)
            },
            "a lowered call result wraps into its tagged Lisette value"
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
        })
    }

    fn resolve_callee(
        &self,
        function: &Expression,
        origin: CallableOrigin<'a>,
        id: Option<String>,
        go_return: Option<&LoweredReturnAbi>,
        receiver_offset: usize,
    ) -> ResolvedCallee<'a> {
        let bound_layout = self.bound_dispatch_layout(function);
        let declaration = id
            .as_deref()
            .and_then(|id| self.facts.callable_declaration(id));
        let alias_declaration = self.facts.function_alias_declaration(&function.get_type());
        let declared_type = declaration
            .map(CallableDeclaration::ty)
            .or(alias_declaration.as_ref());
        let instantiated = self
            .facts
            .resolve_to_function_type(function.get_type().unwrap_forall())
            .unwrap_or_else(|| function.get_type().unwrap_forall().clone());
        let declared_params = declared_type.and_then(|ty| ty.unwrap_forall().get_function_params());
        let declared_return = declared_type.and_then(|ty| ty.unwrap_forall().get_function_ret());
        let mut params = self.build_param_abi(
            &instantiated,
            declared_params,
            receiver_offset,
            id.as_deref(),
            &origin,
        );
        let return_type = instantiated.get_function_ret().unwrap_or(&Type::Never);
        let callee_ty = function.get_type();
        if declaration.is_some_and(CallableDeclaration::is_type_definition)
            && matches!(callee_ty.unwrap_forall(), Type::Nominal { id: type_id, .. } if id.as_deref() == Some(type_id.as_str()))
            && self.facts.is_go_named_function_type(&callee_ty)
        {
            params = vec![CallableParamAbi {
                instantiated: callee_ty.clone(),
                declared: None,
                origin: SlotOrigin::Lisette,
                layout: self
                    .value_layout(&callee_ty, SlotOrigin::Lisette)
                    .underlying()
                    .clone(),
                variadic: None,
            }];
        }
        let result = if let Some(layout) = bound_layout {
            layout.return_abi
        } else {
            match go_return {
                Some(result) => CallableReturnAbi::Lowered(result.clone()),
                None if matches!(
                    origin,
                    CallableOrigin::Source(CallOperation::NativeConstructor {
                        kind: NativeTypeKind::Array,
                        ..
                    })
                ) =>
                {
                    self.facts.return_abi(return_type, ReturnSlot::Function)
                }
                None => self
                    .classify_callee_abi(function, id.as_deref(), declaration, declared_return)
                    .unwrap_or_else(|| {
                        CallableReturnAbi::unlowered(declared_return.unwrap_or(return_type))
                    }),
            }
        };
        let (return_origin, return_declaration) =
            self.facts
                .callee_return_slot(&origin, id.as_deref(), declared_return, return_type);
        let return_layout =
            self.callable_result_layout(return_type, return_origin, return_declaration, &result);
        let is_prelude_dispatch = id.as_deref().is_some_and(go_name::is_prelude_id)
            || matches!(
                origin,
                CallableOrigin::Source(
                    CallOperation::NativeConstructor { .. } | CallOperation::Native { .. }
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
                result_declaration: return_declaration.cloned(),
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
        let return_ty = instantiated.get_function_ret()?;
        let go_return = self.resolve_go_callee_abi(expression, return_ty);
        let origin = if self.is_go_callable(expression) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Source(&CallOperation::Function)
        };
        let (id, _) = self.resolve_callee_definition(expression);
        let receiver_offset = self
            .facts
            .checked(expression)
            .function_value()
            .map_or(0, CheckedCallee::receiver_offset);
        Some(self.resolve_callee(expression, origin, id, go_return.as_ref(), receiver_offset))
    }

    pub(crate) fn resolve_callee_definition(
        &self,
        function: &Expression,
    ) -> (Option<String>, Option<CallableDeclaration<'a>>) {
        let id = match function.unwrap_parens() {
            Expression::Identifier { .. } | Expression::DotAccess { .. } => self
                .facts
                .checked(function)
                .callee()
                .definition
                .as_ref()
                .map(ToString::to_string),
            _ => None,
        };
        let declaration = id
            .as_deref()
            .and_then(|id| self.facts.callable_declaration(id));
        (id, declaration)
    }

    fn classify_callee_abi(
        &self,
        callee: &Expression,
        id: Option<&str>,
        declaration: Option<CallableDeclaration<'a>>,
        declared_return: Option<&Type>,
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
        let callee_definition = id;
        if callee_definition.is_some_and(go_name::is_go_import) {
            return None;
        }
        if let Expression::DotAccess {
            expression: receiver,
            ..
        } = inner
        {
            let receiver_type = receiver.get_type();
            if self.facts.native_kind(&receiver_type).is_some()
                || self.facts.is_prelude_nominal(&receiver_type)
                || matches!(
                    &**receiver,
                    Expression::Identifier {
                        resolution: IdentifierResolution::Definition { name: definition, .. },
                        ..
                    }
                        if go_name::is_prelude_id(definition)
                )
            {
                return None;
            }
        } else if callee_definition.is_some_and(go_name::is_prelude_id) {
            return None;
        }
        if prelude_constructor(inner).is_some() {
            return None;
        }
        if let Expression::DotAccess {
            expression: receiver,
            member,
            ..
        } = inner
            && let Some(method) = self
                .facts
                .interface_method(&self.facts.strip_and_peel(&receiver.get_type()), member)
        {
            return Some(self.facts.return_abi(
                method.ty.get_function_ret().unwrap_or(&Type::Never),
                ReturnSlot::InterfaceMethod {
                    name: &method.source_name,
                    method: &method,
                },
            ));
        }
        if self.selects_tagged_twin(callee) {
            return Some(CallableReturnAbi::Value);
        }
        let classify_ty = declared_return.unwrap_or(f.return_type.as_ref());
        if let Some(CallableDeclaration::Method(_)) = declaration {
            let go_method = id
                .and_then(|id| id.rsplit_once('.'))
                .is_none_or(|(owner, name)| self.facts.emits_go_method(owner, name));
            return Some(if go_method {
                self.facts.return_abi(classify_ty, ReturnSlot::Method)
            } else {
                self.facts.return_abi(classify_ty, ReturnSlot::Function)
            });
        }
        if declaration.is_some_and(|declaration| !declaration.is_type_definition()) {
            return Some(self.facts.return_abi(classify_ty, ReturnSlot::Function));
        }
        let origin = self.function_type_origin(&callee_ty, SlotOrigin::Lisette);
        Some(
            self.facts
                .return_abi(classify_ty, ReturnSlot::FunctionValue(origin)),
        )
    }

    fn resolve_go_call_abi(&self, expression: &Expression) -> Option<LoweredReturnAbi> {
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
    ) -> Option<LoweredReturnAbi> {
        if self.bound_dispatch_requirement(callee).is_some() {
            return None;
        }
        if let Some(requirement) = self.go_bound_requirement(callee) {
            return self
                .facts
                .return_abi(
                    return_ty,
                    ReturnSlot::Go {
                        hints: &requirement.method.go_hints,
                    },
                )
                .lowered()
                .cloned();
        }
        let qualified_name = self
            .facts
            .checked(callee)
            .function_value()?
            .definition
            .as_deref()?;
        if !go_name::is_go_import(qualified_name) {
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
        self.facts
            .return_abi(return_ty, ReturnSlot::Go { hints: go_hints })
            .lowered()
            .cloned()
    }

    pub(crate) fn is_go_callable(&self, expression: &Expression) -> bool {
        if self.bound_dispatch_requirement(expression).is_some() {
            return false;
        }
        resolved_definition(expression).is_some_and(go_name::is_go_import)
            || self.go_bound_requirement(expression).is_some()
            || self
                .function_type_origin(&expression.get_type(), SlotOrigin::Lisette)
                .declared_by_go()
    }

    pub(crate) fn call_target_is_go(&self, expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Call { expression: callee, .. }
                if self.is_go_callable(callee.unwrap_parens())
        )
    }

    fn build_param_abi(
        &self,
        instantiated: &Type,
        declared: Option<&[FunctionParameter]>,
        receiver_offset: usize,
        callee_id: Option<&str>,
        callable_origin: &CallableOrigin<'_>,
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
                        self.facts
                            .go_callable_parameter(id, receiver_offset + index)
                    })
                } else {
                    None
                };
                let origin =
                    catalog_slot.map_or_else(
                        || {
                            if matches!(callable_origin, CallableOrigin::GoInterop) {
                                SlotOrigin::go_parameter(self.facts.resolves_to_unknown(
                                    declared.as_ref().unwrap_or(&instantiated.ty),
                                ))
                            } else {
                                SlotOrigin::Lisette
                            }
                        },
                        |slot| slot.origin,
                    );
                let layout = catalog_slot
                    .map(|slot| {
                        self.value_layout_with_declaration(
                            &instantiated.ty,
                            origin,
                            &slot.declared_type,
                        )
                    })
                    .or_else(|| {
                        declared.as_ref().map(|declared| {
                            self.value_layout_with_declaration(&instantiated.ty, origin, declared)
                        })
                    })
                    .unwrap_or_else(|| self.value_layout(&instantiated.ty, origin));
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
                    || self.value_layout(&element, origin),
                    |declared| self.value_layout_with_declaration(&element, origin, declared),
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
}

impl<'a> EmitFacts<'a> {
    fn callee_return_slot<'r>(
        &'r self,
        origin: &CallableOrigin<'_>,
        id: Option<&str>,
        declared_return: Option<&'r Type>,
        return_type: &Type,
    ) -> (SlotOrigin, Option<&'r Type>) {
        if !matches!(origin, CallableOrigin::GoInterop) {
            return (SlotOrigin::Lisette, declared_return);
        }
        match id.and_then(|id| self.go_callable_return_slot(id)) {
            Some(slot) => (slot.origin, Some(&slot.declared_type)),
            None => (
                SlotOrigin::go_return(
                    self.resolves_to_unknown(declared_return.unwrap_or(return_type)),
                ),
                declared_return,
            ),
        }
    }

    pub(crate) fn callable_declaration(&self, id: &str) -> Option<CallableDeclaration<'a>> {
        self.definition(id)
            .map(CallableDeclaration::Definition)
            .or_else(|| {
                let (owner, name) = id.rsplit_once('.')?;
                self.method(owner, name).map(CallableDeclaration::Method)
            })
    }
}
