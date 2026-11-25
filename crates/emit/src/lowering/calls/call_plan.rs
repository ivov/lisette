use crate::Planner;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lowering::abi::CalleeSignature;
use crate::lowering::abi::callable::{CallableAbi, CallableParamAbi};
use crate::lowering::abi::coercion_plan::LayoutChange;
use crate::lowering::abi::layout::{FunctionLayout, ResultProducer, ValueLayout};
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::program::{
    CallOperation, CheckedCallee, Definition, Method, Visibility, resolved_definition,
};
use syntax::program::{GoTypeRecipe, is_go_import, is_prelude_symbol};
use syntax::types::{CompoundKind, FunctionParameter, Type};

#[derive(Debug)]
pub(in crate::lowering) struct CallPlan<'a> {
    pub(in crate::lowering) resolved: ResolvedCallee<'a>,
    pub(in crate::lowering) arguments: Vec<ArgumentPlan>,
}

impl CallPlan<'_> {
    pub(in crate::lowering) fn wraps_result(&self) -> bool {
        self.resolved.abi.result.is_lowered()
    }
}

#[derive(Debug)]
pub(in crate::lowering) struct ResolvedCallee<'a> {
    pub(in crate::lowering) origin: CallableOrigin<'a>,
    pub(in crate::lowering) declaration: Option<CallableDeclaration<'a>>,
    pub(in crate::lowering) instantiated: Type,
    pub(in crate::lowering) receiver_offset: usize,
    pub(in crate::lowering) abi: CallableAbi,
    pub(in crate::lowering) is_prelude_dispatch: bool,
}

impl ResolvedCallee<'_> {
    pub(in crate::lowering) fn declared_type(&self) -> Option<&Type> {
        self.declaration.map(CallableDeclaration::ty)
    }

    pub(in crate::lowering) fn function_layout(&self, planner: &Planner<'_>) -> FunctionLayout {
        FunctionLayout {
            producer: self.origin.producer(),
            ..self.abi.function_layout(planner)
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) enum CallableDeclaration<'a> {
    Definition(&'a Definition),
    Method(&'a Method),
}

impl<'a> CallableDeclaration<'a> {
    pub(in crate::lowering) fn ty(self) -> &'a Type {
        match self {
            Self::Definition(definition) => &definition.ty,
            Self::Method(method) => &method.ty,
        }
    }

    pub(in crate::lowering) fn visibility(self) -> &'a Visibility {
        match self {
            Self::Definition(definition) => &definition.visibility,
            Self::Method(method) => &method.visibility,
        }
    }

    pub(in crate::lowering) fn is_type_definition(self) -> bool {
        matches!(self, Self::Definition(definition) if definition.is_type_definition())
    }

    pub(in crate::lowering) fn go_type_param_recipe(self) -> Option<&'a [GoTypeRecipe]> {
        match self {
            Self::Definition(definition) => definition.go_type_param_recipe(),
            Self::Method(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) enum CallableOrigin<'a> {
    GoInterop,
    Source(&'a CallOperation),
}

impl CallableOrigin<'_> {
    pub(in crate::lowering) fn producer(self) -> ResultProducer {
        match self {
            CallableOrigin::GoInterop => ResultProducer::Go,
            CallableOrigin::Source(_) => ResultProducer::Lisette,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::lowering) enum ArgumentPlan {
    Direct,
    LayoutChange(Box<ArgumentSlotLayoutChange>),
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct FunctionLayoutChange {
    pub(in crate::lowering) source: FunctionLayout,
    pub(in crate::lowering) target: FunctionLayout,
}

impl FunctionLayoutChange {
    pub(in crate::lowering) fn into_argument(
        self,
        function_type: Type,
    ) -> ArgumentSlotLayoutChange {
        ArgumentSlotLayoutChange {
            target: ValueLayout::Function {
                function_type,
                layout: self.target.clone(),
            },
            change: LayoutChange::Function {
                source: Box::new(self.source),
                target: Box::new(self.target),
            },
            source: ArgumentValueSource::Callable,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) enum ArgumentValueSource {
    Lisette,
    GoPhysical,
    GoSlot,
    Callable,
}

#[derive(Debug, Clone)]
pub(in crate::lowering) struct ArgumentSlotLayoutChange {
    pub(in crate::lowering) target: ValueLayout,
    pub(in crate::lowering) change: LayoutChange,
    pub(in crate::lowering) source: ArgumentValueSource,
}

impl<'a> Planner<'a> {
    pub(in crate::lowering) fn plan_call(&self, expression: &Expression) -> Option<CallPlan<'a>> {
        let Expression::Call {
            expression: callee,
            args,
            ..
        } = expression
        else {
            return None;
        };

        let checked = self.facts.checked(callee).callee();
        let origin = if self.is_go_callable(callee) {
            CallableOrigin::GoInterop
        } else {
            CallableOrigin::Source(&checked.operation)
        };
        let definition = checked.definition.as_ref().map(ToString::to_string);

        let resolved = self.resolve_callee(callee, origin, definition, checked.receiver_offset());
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
        receiver_offset: usize,
    ) -> ResolvedCallee<'a> {
        let declaration = id
            .as_deref()
            .and_then(|id| callable_declaration(&self.facts, id));
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
        let result = self.callee_return_abi(CalleeSignature {
            callee: function,
            origin: &origin,
            id: id.as_deref(),
            declaration,
            declared_return,
            return_type,
        });

        let (return_origin, return_declaration) = callee_return_slot(
            &self.facts,
            &origin,
            id.as_deref(),
            declared_return,
            return_type,
        );

        let return_layout =
            self.callable_result_layout(return_type, return_origin, return_declaration, &result);
        let is_prelude_dispatch = is_prelude_dispatch(id.as_deref(), &origin);

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

    pub(in crate::lowering) fn resolve_callable_value(
        &self,
        expression: &Expression,
    ) -> Option<ResolvedCallee<'a>> {
        let instantiated = self
            .facts
            .resolve_to_function_type(expression.get_type().unwrap_forall())?;
        instantiated.get_function_ret()?;
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
        Some(self.resolve_callee(expression, origin, id, receiver_offset))
    }

    pub(in crate::lowering) fn resolve_callee_definition(
        &self,
        function: &Expression,
    ) -> (Option<String>, Option<CallableDeclaration<'a>>) {
        let id = match function {
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
            .and_then(|id| callable_declaration(&self.facts, id));
        (id, declaration)
    }

    pub(in crate::lowering) fn is_go_callable(&self, expression: &Expression) -> bool {
        if self.bound_dispatch_requirement(expression).is_some() {
            return false;
        }
        resolved_definition(expression).is_some_and(is_go_import)
            || self.go_bound_requirement(expression).is_some()
            || self
                .function_type_origin(&expression.get_type(), SlotOrigin::Lisette)
                .declared_by_go()
    }

    pub(in crate::lowering) fn call_target_is_go(&self, expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Call { expression: callee, .. }
                if self.is_go_callable(callee)
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
        let parameters = instantiated.get_function_params().unwrap_or(&[]);
        let variadic_element = instantiated.is_variadic();
        parameters
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

                let origin = catalog_slot.map_or_else(
                    || {
                        if matches!(callable_origin, CallableOrigin::GoInterop) {
                            SlotOrigin::of_go_declaration(
                                SlotOrigin::GoParameter,
                                declared.as_ref().unwrap_or(&instantiated.ty),
                                |id| self.facts.definition(id),
                            )
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

                let element = (index + 1 == parameters.len())
                    .then(|| variadic_element.clone())
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

fn callee_return_slot<'r>(
    facts: &'r EmitFacts<'_>,
    origin: &CallableOrigin<'_>,
    id: Option<&str>,
    declared_return: Option<&'r Type>,
    return_type: &Type,
) -> (SlotOrigin, Option<&'r Type>) {
    if !matches!(origin, CallableOrigin::GoInterop) {
        return (SlotOrigin::Lisette, declared_return);
    }
    match id.and_then(|id| facts.go_callable_return_slot(id)) {
        Some(slot) => (slot.origin, Some(&slot.declared_type)),
        None => (
            SlotOrigin::of_go_declaration(
                SlotOrigin::GoReturn,
                declared_return.unwrap_or(return_type),
                |id| facts.definition(id),
            ),
            declared_return,
        ),
    }
}

pub(in crate::lowering) fn is_prelude_dispatch(
    id: Option<&str>,
    origin: &CallableOrigin<'_>,
) -> bool {
    id.is_some_and(is_prelude_symbol)
        || matches!(
            origin,
            CallableOrigin::Source(
                CallOperation::NativeConstructor { .. } | CallOperation::Native { .. }
            )
        )
}

pub(in crate::lowering) fn callable_declaration<'a>(
    facts: &EmitFacts<'a>,
    id: &str,
) -> Option<CallableDeclaration<'a>> {
    facts
        .definition(id)
        .map(CallableDeclaration::Definition)
        .or_else(|| {
            let (owner, name) = id.rsplit_once('.')?;
            facts.method(owner, name).map(CallableDeclaration::Method)
        })
}
