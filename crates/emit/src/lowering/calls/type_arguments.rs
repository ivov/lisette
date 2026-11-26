use crate::lowering::calls::call_plan::ResolvedCallee;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::types::go_type::GoType;
use syntax::EcoString;
use syntax::ast::Expression;
use syntax::program::GoTypeRecipe;
use syntax::types::{CompoundKind, FunctionParameter, FunctionType, SubstitutionMap, Type};

pub(in crate::lowering) struct CallTypeArgumentsRequest<'e, 'c> {
    pub(in crate::lowering) function: &'e Expression,
    pub(in crate::lowering) callee: &'e ResolvedCallee<'c>,
    pub(in crate::lowering) written_type_arguments: usize,
    pub(in crate::lowering) call_ty: Option<&'e Type>,
    pub(in crate::lowering) arg_shape: CallArgumentShape,
    pub(in crate::lowering) slot: Option<&'e Type>,
    pub(in crate::lowering) receiver_passed: bool,
}

#[derive(Clone, Copy)]
pub(in crate::lowering) struct CallArgumentShape {
    pub value_count: usize,
    pub has_spread: bool,
}

fn all_type_params_inferrable(
    vars: &[EcoString],
    params: &[FunctionParameter],
    receiver_count: usize,
    arg_shape: CallArgumentShape,
) -> bool {
    let variadic_index = params
        .iter()
        .position(|p| p.ty.is_native(CompoundKind::VarArgs))
        .filter(|&i| i == params.len() - 1);
    let variadic_has_args = arg_shape.has_spread
        || variadic_index.is_some_and(|i| arg_shape.value_count + receiver_count > i);

    vars.iter().all(|var| {
        let param_ty = Type::Parameter(var.clone());
        params.iter().enumerate().any(|(i, pt)| {
            pt.ty.contains_type(&param_ty) && (Some(i) != variadic_index || variadic_has_args)
        })
    })
}

fn generic_function(declared: Option<&Type>) -> Option<(&[EcoString], &FunctionType)> {
    let Type::Forall { vars, body } = declared? else {
        return None;
    };
    if vars.is_empty() {
        return None;
    }
    let Type::Function(f) = body.as_ref() else {
        return None;
    };
    Some((vars, f))
}

fn callee_curries_receiver(callee: &ResolvedCallee<'_>) -> bool {
    let Some(Type::Forall { body, .. }) = callee.declared_type() else {
        return false;
    };
    matches!(body.as_ref(), Type::Function(_)) && callee.receiver_offset > 0
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn prelude_container_type_args(&self, ty: &Type) -> Option<GoType> {
        let params = self.facts.prelude_container_arguments(ty)?;
        params
            .iter()
            .any(|p| {
                self.facts.is_interface_or_unknown(p) || self.facts.is_go_named_function_type(p)
            })
            .then(|| self.format_type_args(&params))
            .flatten()
    }

    pub(in crate::lowering) fn call_type_arguments(
        &self,
        request: CallTypeArgumentsRequest<'_, '_>,
    ) -> Option<GoType> {
        let CallTypeArgumentsRequest {
            function,
            callee,
            written_type_arguments,
            arg_shape,
            slot,
            receiver_passed,
            ..
        } = request;

        if !receiver_passed && callee_curries_receiver(callee) {
            return None;
        }
        let receiver_count = if receiver_passed {
            callee.receiver_offset.min(1)
        } else {
            0
        };
        if let Some(recipe) = callee
            .declaration
            .and_then(|declaration| declaration.go_type_param_recipe())
        {
            return self.recipe_type_arguments(&request, recipe, receiver_count);
        }

        if written_type_arguments > 0 {
            return self.written_call_type_arguments(&request);
        }

        if let Some(pinned) = self.uninferable_type_args(
            function,
            callee.declared_type(),
            receiver_count,
            arg_shape,
            !receiver_passed,
        ) {
            return slot
                .filter(|_| !receiver_passed)
                .and_then(|slot| self.prelude_container_type_args(slot))
                .or(Some(pinned));
        }

        self.prelude_constructor_type_arguments(&request)
    }

    fn recipe_type_arguments(
        &self,
        request: &CallTypeArgumentsRequest<'_, '_>,
        recipe: &[GoTypeRecipe],
        receiver_count: usize,
    ) -> Option<GoType> {
        let arg_shape = request.arg_shape;
        let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
        if has_value_args
            && generic_function(request.callee.declared_type()).is_some_and(|(vars, f)| {
                all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
            })
        {
            return None;
        }
        let type_arguments = &self.facts.checked(request.function).callee().type_arguments;
        self.reconstruct_collapsed_type_args(recipe, type_arguments)
    }

    fn written_call_type_arguments(
        &self,
        request: &CallTypeArgumentsRequest<'_, '_>,
    ) -> Option<GoType> {
        let callee = self.facts.checked(request.function).callee();
        if request.receiver_passed {
            return request.callee.declared_type().and_then(|declared| {
                self.format_generic_instantiation(declared, &callee.type_arguments)
            });
        }
        self.format_type_arg_iter(callee.written_type_arguments(request.written_type_arguments))
    }

    fn prelude_constructor_type_arguments(
        &self,
        request: &CallTypeArgumentsRequest<'_, '_>,
    ) -> Option<GoType> {
        if !matches!(
            prelude_constructor(request.function),
            Some(
                PreludeVariant::Some
                    | PreludeVariant::Ok
                    | PreludeVariant::Err
                    | PreludeVariant::Partial(_)
            )
        ) {
            return None;
        }
        request
            .call_ty
            .and_then(|ty| self.prelude_container_type_args(ty))
            .or_else(|| {
                request
                    .slot
                    .and_then(|ty| self.prelude_container_type_args(ty))
            })
    }

    pub(in crate::lowering) fn constructor_reference_type_arguments(
        &self,
        fn_params: &[FunctionParameter],
        return_type_arguments: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> Option<GoType> {
        let return_params_are_inferrable = return_type_arguments.len() <= fn_params.len()
            && return_type_arguments.iter().all(|argument| {
                fn_params
                    .iter()
                    .any(|param| param.ty.contains_type(argument))
            });
        let parameters_support_go_inference = fn_params
            .iter()
            .all(|param| !self.facts.is_go_named_function_type(&param.ty));
        if ctx.is_callee() && return_params_are_inferrable && parameters_support_go_inference {
            None
        } else {
            self.format_type_args(return_type_arguments)
        }
    }

    fn uninferable_type_args(
        &self,
        function: &Expression,
        declared: Option<&Type>,
        receiver_count: usize,
        arg_shape: CallArgumentShape,
        function_alias_pins: bool,
    ) -> Option<GoType> {
        let (vars, f) = generic_function(declared)?;
        let mapping = &self.facts.checked(function).callee().type_arguments;
        if all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
            && !(function_alias_pins
                && vars.iter().any(|var| {
                    self.facts
                        .is_go_named_function_type(checked_type_argument(mapping, var))
                }))
        {
            return None;
        }
        Some(
            self.instantiated_type_args(vars, mapping)
                .expect("a generic function has type parameters"),
        )
    }

    pub(in crate::lowering) fn format_value_type_args(
        &self,
        expression: &Expression,
    ) -> Option<GoType> {
        let recorded = self.facts.checked(expression).function_value()?;
        let declaration = self.facts.callable(recorded.definition.as_deref()?);
        let instantiation = &recorded.type_arguments;
        match declaration.go_type_param_recipe() {
            Some(recipe) => self.reconstruct_collapsed_type_args(recipe, instantiation),
            None => self.format_generic_instantiation(declaration.ty(), instantiation),
        }
    }

    fn format_generic_instantiation(
        &self,
        definition_ty: &Type,
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let Type::Forall { vars, .. } = definition_ty else {
            return None;
        };
        if vars.is_empty() {
            return None;
        }
        self.instantiated_type_args(vars, mapping)
    }

    fn instantiated_type_args(
        &self,
        vars: &[EcoString],
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        self.format_type_arg_iter(vars.iter().map(|var| checked_type_argument(mapping, var)))
    }

    pub(in crate::lowering) fn format_type_args(&self, parameters: &[Type]) -> Option<GoType> {
        self.format_type_arg_iter(parameters)
    }

    pub(in crate::lowering) fn format_type_arg_iter<'t>(
        &self,
        parameters: impl IntoIterator<Item = &'t Type>,
    ) -> Option<GoType> {
        let arguments: Vec<GoType> = parameters
            .into_iter()
            .map(|param| self.go_type(param))
            .collect();
        GoType::type_arguments(&arguments)
    }

    pub(in crate::lowering) fn reconstruct_collapsed_type_args(
        &self,
        recipe: &[GoTypeRecipe],
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let parts: Vec<GoType> = recipe
            .iter()
            .map(|entry| self.render_recipe_entry(entry, mapping))
            .collect();
        GoType::type_arguments(&parts)
    }

    fn render_recipe_entry(&self, entry: &GoTypeRecipe, mapping: &SubstitutionMap) -> GoType {
        match entry {
            GoTypeRecipe::Parameter(name) => self.go_type(checked_type_argument(mapping, name)),
            GoTypeRecipe::Slice(element) => {
                GoType::slice_of(&self.render_recipe_entry(element, mapping))
            }
            GoTypeRecipe::Map(key, value) => GoType::map_of(
                &self.render_recipe_entry(key, mapping),
                &self.render_recipe_entry(value, mapping),
            ),
            GoTypeRecipe::Array(element, length) => GoType::array_of(
                length.to_string(),
                &self.render_recipe_entry(element, mapping),
            ),
        }
    }

    pub(in crate::lowering) fn format_type_args_with_receiver<'t>(
        &self,
        receiver_ty: &Type,
        type_args: impl Iterator<Item = &'t Type>,
    ) -> Option<GoType> {
        let receiver_params = receiver_ty.get_type_params().unwrap_or_default();
        let arguments: Vec<GoType> = receiver_params
            .iter()
            .map(|ty| self.go_type(ty))
            .chain(type_args.map(|ty| self.go_type(ty)))
            .collect();
        GoType::type_arguments(&arguments)
    }
}

fn checked_type_argument<'m>(mapping: &'m SubstitutionMap, var: &str) -> &'m Type {
    mapping
        .get(var)
        .unwrap_or_else(|| panic!("the checker records the type argument `{var}`"))
}
