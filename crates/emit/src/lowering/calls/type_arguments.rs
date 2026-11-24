use crate::Planner;
use crate::lowering::calls::call_plan::{ResolvedCallee, callable_declaration};
use crate::lowering::plan::context::ExpressionContext;
use crate::target::ir::prelude::{PreludeVariant, prelude_constructor};
use crate::target::types::go_type::GoType;
use syntax::EcoString;
use syntax::ast::Expression;
use syntax::program::GoTypeRecipe;
use syntax::types::{CompoundKind, FunctionParameter, FunctionType, SubstitutionMap, Type};

pub(in crate::lowering) struct CallTypeArgsRequest<'e, 'c> {
    pub(in crate::lowering) function: &'e Expression,
    pub(in crate::lowering) callee: &'e ResolvedCallee<'c>,
    pub(in crate::lowering) written_type_arguments: usize,
    pub(in crate::lowering) call_ty: Option<&'e Type>,
    pub(in crate::lowering) arg_shape: CallArgShape,
    pub(in crate::lowering) slot: Option<&'e Type>,
    pub(in crate::lowering) receiver_passed: bool,
}

#[derive(Clone, Copy)]
pub(in crate::lowering) struct CallArgShape {
    pub value_count: usize,
    pub has_spread: bool,
}

fn all_type_params_inferrable(
    vars: &[EcoString],
    params: &[FunctionParameter],
    receiver_count: usize,
    arg_shape: CallArgShape,
) -> bool {
    let variadic_idx = params
        .iter()
        .position(|p| p.ty.is_native(CompoundKind::VarArgs))
        .filter(|&i| i == params.len() - 1);
    let variadic_has_args = arg_shape.has_spread
        || variadic_idx.is_some_and(|i| arg_shape.value_count + receiver_count > i);

    vars.iter().all(|var| {
        let param_ty = Type::Parameter(var.clone());
        params.iter().enumerate().any(|(i, pt)| {
            pt.ty.contains_type(&param_ty) && (Some(i) != variadic_idx || variadic_has_args)
        })
    })
}

impl Planner<'_> {
    pub(in crate::lowering) fn prelude_container_type_args(&mut self, ty: &Type) -> Option<GoType> {
        let params = self.facts.prelude_container_arguments(ty)?;
        if params.is_empty() {
            return None;
        }
        params
            .iter()
            .any(|p| {
                self.facts.is_interface_or_unknown(p) || self.facts.is_go_named_function_type(p)
            })
            .then(|| self.format_type_args(&params))
    }

    pub(in crate::lowering) fn call_type_arguments(
        &mut self,
        request: CallTypeArgsRequest<'_, '_>,
    ) -> GoType {
        let CallTypeArgsRequest {
            function,
            callee,
            written_type_arguments,
            call_ty,
            arg_shape,
            slot,
            receiver_passed,
        } = request;
        if !receiver_passed && callee_curries_receiver(callee) {
            return GoType::default();
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
            let has_value_args = arg_shape.value_count > 0 || arg_shape.has_spread;
            if has_value_args
                && generic_function(callee.declared_type()).is_some_and(|(vars, f)| {
                    all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
                })
            {
                return GoType::default();
            }
            let type_arguments = &self.facts.checked(function).callee().type_arguments;
            return self
                .reconstruct_collapsed_type_args(recipe, type_arguments)
                .unwrap_or_default();
        }

        if written_type_arguments > 0 {
            if receiver_passed {
                let type_arguments = &self.facts.checked(function).callee().type_arguments;
                return callee
                    .declared_type()
                    .and_then(|declared| {
                        self.format_generic_instantiation(declared, type_arguments)
                    })
                    .unwrap_or_default();
            }
            let written = self
                .facts
                .checked(function)
                .callee()
                .written_type_arguments(written_type_arguments);
            return self.format_type_arg_iter(written);
        }

        if let Some(inferred) = self.uninferable_type_args(
            function,
            callee.declared_type(),
            receiver_count,
            arg_shape,
            !receiver_passed,
        ) {
            return match slot.filter(|_| !receiver_passed) {
                Some(slot) => self.prelude_container_type_args(slot).unwrap_or(inferred),
                None => inferred,
            };
        }

        if matches!(
            prelude_constructor(function),
            Some(
                PreludeVariant::Some
                    | PreludeVariant::Ok
                    | PreludeVariant::Err
                    | PreludeVariant::Partial(_)
            )
        ) {
            return call_ty
                .and_then(|ty| self.prelude_container_type_args(ty))
                .or_else(|| slot.and_then(|ty| self.prelude_container_type_args(ty)))
                .unwrap_or_default();
        }
        GoType::default()
    }

    pub(in crate::lowering) fn constructor_reference_type_arguments(
        &mut self,
        fn_params: &[FunctionParameter],
        ret_params: &[Type],
        ctx: ExpressionContext<'_>,
    ) -> GoType {
        let return_params_are_inferrable = ret_params.len() <= fn_params.len()
            && ret_params
                .iter()
                .all(|ret| fn_params.iter().any(|param| param.ty.contains_type(ret)));
        let parameters_support_go_inference = fn_params
            .iter()
            .all(|param| !self.facts.is_go_named_function_type(&param.ty));
        if ctx.is_callee() && return_params_are_inferrable && parameters_support_go_inference {
            GoType::default()
        } else {
            self.format_type_args(ret_params)
        }
    }

    fn uninferable_type_args(
        &self,
        function: &Expression,
        declared: Option<&Type>,
        receiver_count: usize,
        arg_shape: CallArgShape,
        function_alias_pins: bool,
    ) -> Option<GoType> {
        let (vars, f) = generic_function(declared)?;
        let mapping = &self.facts.checked(function).callee().type_arguments;
        if all_type_params_inferrable(vars, &f.params, receiver_count, arg_shape)
            && !(function_alias_pins
                && vars.iter().any(|var| {
                    mapping
                        .get(var)
                        .is_some_and(|ty| self.facts.is_go_named_function_type(ty))
                }))
        {
            return None;
        }
        self.instantiated_type_args(vars, mapping)
    }

    pub(in crate::lowering) fn format_value_type_args(
        &mut self,
        expression: &Expression,
    ) -> Option<GoType> {
        let recorded = self.facts.checked(expression).function_value()?;
        let declaration = callable_declaration(&self.facts, recorded.definition.as_deref()?)?;
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
        let resolved = vars
            .iter()
            .map(|var| mapping.get(var.as_str()))
            .collect::<Option<Vec<_>>>()?;
        Some(self.format_type_arg_iter(resolved))
    }

    pub(in crate::lowering) fn format_type_args(&self, parameters: &[Type]) -> GoType {
        self.format_type_arg_iter(parameters)
    }

    pub(in crate::lowering) fn format_type_arg_iter<'t>(
        &self,
        parameters: impl IntoIterator<Item = &'t Type>,
    ) -> GoType {
        let arguments: Vec<GoType> = parameters
            .into_iter()
            .map(|param| self.component_go_type(param))
            .collect();
        GoType::type_arguments(&arguments)
    }

    pub(in crate::lowering) fn reconstruct_collapsed_type_args(
        &self,
        recipe: &[GoTypeRecipe],
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        let parts = recipe
            .iter()
            .map(|entry| self.render_recipe_entry(entry, mapping))
            .collect::<Option<Vec<_>>>()?;
        (!parts.is_empty()).then(|| GoType::type_arguments(&parts))
    }

    fn render_recipe_entry(
        &self,
        entry: &GoTypeRecipe,
        mapping: &SubstitutionMap,
    ) -> Option<GoType> {
        Some(match entry {
            GoTypeRecipe::Parameter(name) => self.component_go_type(mapping.get(name)?),
            GoTypeRecipe::Slice(element) => {
                GoType::slice_of(&self.render_recipe_entry(element, mapping)?)
            }
            GoTypeRecipe::Map(key, value) => GoType::map_of(
                &self.render_recipe_entry(key, mapping)?,
                &self.render_recipe_entry(value, mapping)?,
            ),
            GoTypeRecipe::Array(element, length) => GoType::array_of(
                length.to_string(),
                &self.render_recipe_entry(element, mapping)?,
            ),
        })
    }

    pub(in crate::lowering) fn format_type_args_with_receiver<'t>(
        &self,
        receiver_ty: &Type,
        type_args: impl Iterator<Item = &'t Type>,
    ) -> GoType {
        let receiver_params = receiver_ty.get_type_params().unwrap_or_default();
        let arguments: Vec<GoType> = receiver_params
            .iter()
            .map(|ty| self.component_go_type(ty))
            .chain(type_args.map(|ty| self.component_go_type(ty)))
            .collect();
        GoType::type_arguments(&arguments)
    }
}

fn generic_function(declared: Option<&Type>) -> Option<(&[EcoString], &FunctionType)> {
    let Type::Forall { vars, body } = declared? else {
        return None;
    };
    let Type::Function(f) = body.as_ref() else {
        return None;
    };
    Some((vars, f))
}

fn callee_curries_receiver(callee: &ResolvedCallee<'_>) -> bool {
    let Some(Type::Forall { body, .. }) = callee.declared_type() else {
        return false;
    };
    matches!(body.as_ref(), Type::Function(_))
        && callee.instantiated.as_function_type().is_some()
        && callee.receiver_offset > 0
}
