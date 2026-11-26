use crate::lookup::facts::callables::CallableDeclaration;
use crate::lookup::facts::emit_facts::EmitFacts;
use crate::lookup::facts::value_layout::{FunctionLayout, ResultProducer, ValueLayout};
use crate::lowering::abi::CalleeSignature;
use crate::lowering::abi::callable::{CallableAbi, CallableParamAbi};
use crate::lowering::abi::coercion_plan::LayoutChange;
use crate::lowering::calls::call_site::CallSite;
use crate::lowering::planner::queries::PlannerQueries;
use syntax::ast::Expression;
use syntax::program::SlotOrigin;
use syntax::program::{CallOperation, CheckedCallee, resolved_definition};
use syntax::program::{is_go_import, is_prelude_symbol};
use syntax::types::{CompoundKind, FunctionParameter, Type};

#[derive(Debug)]
pub(in crate::lowering) struct CallPlan<'a, 'e> {
    pub(in crate::lowering) site: CallSite<'e>,
    pub(in crate::lowering) resolved: ResolvedCallee<'a>,
    pub(in crate::lowering) arguments: Vec<ArgumentPlan>,
}

impl CallPlan<'_, '_> {
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

    pub(in crate::lowering) fn function_layout(&self, facts: &EmitFacts<'_>) -> FunctionLayout {
        FunctionLayout {
            producer: self.origin.producer(),
            ..self.abi.function_layout(facts)
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

struct ParamAbiSource<'s> {
    declared: Option<&'s [FunctionParameter]>,
    receiver_offset: usize,
    callee_id: Option<&'s str>,
    origin: CallableOrigin<'s>,
}

struct ParamSlot {
    declared: Option<Type>,
    origin: SlotOrigin,
    layout: ValueLayout,
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
                |id| Some(facts.definition(id)),
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

impl<'a> PlannerQueries<'_, 'a> {
    pub(in crate::lowering) fn plan_call<'e>(&self, site: CallSite<'e>) -> CallPlan<'a, 'e> {
        let callee = site.callee;
        let args = site.args;

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

        CallPlan {
            site,
            resolved,
            arguments,
        }
    }

    fn resolve_callee(
        &self,
        function: &Expression,
        origin: CallableOrigin<'a>,
        id: Option<String>,
        receiver_offset: usize,
    ) -> ResolvedCallee<'a> {
        let callee_ty = function
            .type_ref()
            .expect("internal error: a callee has a type");
        let declaration = id.as_deref().map(|id| self.facts.callable(id));
        let alias_declaration = self.facts.function_alias_declaration(callee_ty);
        let declared_type = declaration
            .map(CallableDeclaration::ty)
            .or(alias_declaration.as_ref());
        let instantiated = self
            .facts
            .resolve_to_function_type(callee_ty.unwrap_forall())
            .expect("a callee must have a function type");
        let instantiated_function = instantiated
            .as_function_type()
            .expect("a callee must have a function type");
        let declared_function = declared_type.and_then(Type::as_function_type);
        let declared_params = declared_function.map(|function| function.params.as_slice());
        let declared_return = declared_function.map(|function| function.return_type.as_ref());
        let (params, return_type) =
            match self.go_named_function_type_param(callee_ty, declaration, id.as_deref()) {
                Some(param) => (vec![param], callee_ty),
                None => (
                    self.build_param_abi(
                        &instantiated,
                        ParamAbiSource {
                            declared: declared_params,
                            receiver_offset,
                            callee_id: id.as_deref(),
                            origin,
                        },
                    ),
                    instantiated_function.return_type.as_ref(),
                ),
            };
        let abi = self.callable_abi(
            params,
            CalleeSignature {
                callee: function,
                origin: &origin,
                named: id.as_deref().zip(declaration),
                function: instantiated_function,
                declared_return,
                return_type,
            },
        );
        let is_prelude_dispatch = is_prelude_dispatch(id.as_deref(), &origin);

        ResolvedCallee {
            origin,
            declaration,
            instantiated,
            receiver_offset,
            abi,
            is_prelude_dispatch,
        }
    }

    fn go_named_function_type_param(
        &self,
        callee_ty: &Type,
        declaration: Option<CallableDeclaration<'_>>,
        id: Option<&str>,
    ) -> Option<CallableParamAbi> {
        if !declaration.is_some_and(CallableDeclaration::is_type_definition)
            || !self.facts.is_go_named_function_type(callee_ty)
        {
            return None;
        }
        let Type::Nominal { id: type_id, .. } = callee_ty.unwrap_forall() else {
            return None;
        };
        if id != Some(type_id.as_str()) {
            return None;
        }
        Some(CallableParamAbi {
            instantiated: callee_ty.clone(),
            declared: None,
            origin: SlotOrigin::Lisette,
            layout: self
                .facts
                .value_layout(callee_ty, SlotOrigin::Lisette)
                .underlying()
                .clone(),
            variadic: None,
        })
    }

    fn callable_abi(
        &self,
        params: Vec<CallableParamAbi>,
        signature: CalleeSignature<'_, '_>,
    ) -> CallableAbi {
        let (return_origin, return_declaration) = callee_return_slot(
            self.facts,
            signature.origin,
            signature.named.map(|(id, _)| id),
            signature.declared_return,
            signature.return_type,
        );
        let return_type = signature.return_type;
        let result = self.callee_return_abi(signature);
        let return_layout = self.facts.callable_result_layout(
            return_type,
            return_origin,
            return_declaration,
            &result,
        );
        CallableAbi {
            params,
            result,
            return_layout,
            result_declaration: return_declaration.cloned(),
        }
    }

    pub(in crate::lowering) fn resolve_callable_value(
        &self,
        expression: &Expression,
    ) -> Option<ResolvedCallee<'a>> {
        self.facts
            .resolve_to_function_type(expression.get_type().unwrap_forall())?;
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
        let id = matches!(
            function,
            Expression::Identifier { .. } | Expression::DotAccess { .. }
        )
        .then(|| self.facts.checked(function).callee().definition.as_ref())
        .flatten()
        .map(ToString::to_string);

        let declaration = id.as_deref().map(|id| self.facts.callable(id));
        (id, declaration)
    }

    pub(in crate::lowering) fn is_go_callable(&self, expression: &Expression) -> bool {
        if self.bound_dispatch_requirement(expression).is_some() {
            return false;
        }
        resolved_definition(expression).is_some_and(is_go_import)
            || self.go_bound_requirement(expression).is_some()
            || self
                .facts
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
        source: ParamAbiSource<'_>,
    ) -> Vec<CallableParamAbi> {
        let parameters = &instantiated
            .as_function_type()
            .expect("a callee must have a function type")
            .params;
        let variadic_element = instantiated.is_variadic();
        parameters
            .iter()
            .enumerate()
            .map(|(index, instantiated)| {
                let slot = self.param_slot(&source, index, &instantiated.ty);
                let element = (index + 1 == parameters.len())
                    .then(|| variadic_element.clone())
                    .flatten();
                match element {
                    Some(element) => self.variadic_param_abi(slot, element),
                    None => CallableParamAbi {
                        instantiated: instantiated.ty.clone(),
                        declared: slot.declared,
                        origin: slot.origin,
                        layout: slot.layout,
                        variadic: None,
                    },
                }
            })
            .collect()
    }

    fn param_slot(
        &self,
        source: &ParamAbiSource<'_>,
        index: usize,
        instantiated: &Type,
    ) -> ParamSlot {
        let position = source.receiver_offset + index;
        let declared = source
            .declared
            .and_then(|params| params.get(position))
            .map(|param| param.ty.clone());
        let from_go = matches!(source.origin, CallableOrigin::GoInterop);
        let catalog_slot = if from_go {
            source
                .callee_id
                .and_then(|id| self.facts.go_callable_parameter(id, position))
        } else {
            None
        };

        let origin = catalog_slot.map_or_else(
            || {
                if from_go {
                    SlotOrigin::of_go_declaration(
                        SlotOrigin::GoParameter,
                        declared.as_ref().unwrap_or(instantiated),
                        |id| Some(self.facts.definition(id)),
                    )
                } else {
                    SlotOrigin::Lisette
                }
            },
            |slot| slot.origin,
        );

        let layout = catalog_slot
            .map(|slot| {
                self.facts
                    .value_layout_with_declaration(instantiated, origin, &slot.declared_type)
            })
            .or_else(|| {
                declared.as_ref().map(|declared| {
                    self.facts
                        .value_layout_with_declaration(instantiated, origin, declared)
                })
            })
            .unwrap_or_else(|| self.facts.value_layout(instantiated, origin));

        ParamSlot {
            declared,
            origin,
            layout,
        }
    }

    fn variadic_param_abi(&self, slot: ParamSlot, element: Type) -> CallableParamAbi {
        let declared_element = slot.declared.map(|declared| {
            if declared.is_native(CompoundKind::VarArgs) {
                declared
                    .inner()
                    .expect("a variadic parameter has an element type")
            } else {
                declared
            }
        });

        let element_layout = declared_element.as_ref().map_or_else(
            || self.facts.value_layout(&element, slot.origin),
            |declared| {
                self.facts
                    .value_layout_with_declaration(&element, slot.origin, declared)
            },
        );
        CallableParamAbi {
            instantiated: element,
            declared: declared_element,
            origin: slot.origin,
            layout: element_layout,
            variadic: Some(slot.layout),
        }
    }
}
