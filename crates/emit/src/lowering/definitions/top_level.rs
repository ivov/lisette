use crate::Planner;
use crate::lowering::definitions::top_items::item_documentation;
use crate::lowering::planner::queries::PlannerQueries;
use crate::lowering::value_plans::context::ExpressionContext;
use crate::target::ir::bodies::{ConstPlan, LoweredStatement};
use crate::target::ir::declarations::{DeclarationKind, GoDeclaration};
use crate::target::names::generics::generic_bounds;
use crate::target::names::go_name;
use crate::target::names::source_name::SourceName;
use syntax::EcoString;
use syntax::ast::BindingId;
use syntax::ast::{Expression, Generic};
use syntax::constant;
use syntax::types::{SimpleKind, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ConstPlacement {
    Package,
    Local,
}

impl Planner<'_> {
    pub(in crate::lowering) fn lower_const(
        &mut self,
        identifier: &EcoString,
        binding: Option<BindingId>,
        expression: &Expression,
        ty: &Type,
        placement: ConstPlacement,
    ) -> ConstPlan {
        let name = SourceName::constant(identifier);
        let preferred = name.preferred();
        let hides_package_name =
            placement == ConstPlacement::Local && self.names.is_package_block_name(&preferred);
        let go_name = if !hides_package_name && self.scope.try_declare_go_name(&preferred) {
            self.scope.new_local_identifier(preferred)
        } else {
            self.fresh_declared(&name)
        };
        let go_type = (!matches!(ty, Type::Simple(SimpleKind::String | SimpleKind::Bool)))
            .then(|| self.queries().go_type(ty));

        if let Some(binding) = binding
            && let Some(constant) = constant::local_constant(expression, ty, &self.queries())
        {
            self.scope.record_local_constant(binding, constant);
        }
        let raw_value = self.plan_value(expression, ExpressionContext::value());
        let value = if raw_value.is_empty() {
            self.unit_value(Some(expression), Some(ty))
        } else {
            raw_value.into_parts().1
        };
        ConstPlan {
            name: self
                .scope
                .bind_new_const(identifier, binding.as_slice(), go_name),
            go_type,
            value,
        }
    }

    pub(in crate::lowering) fn emit_const(
        &mut self,
        identifier: &EcoString,
        expression: &Expression,
        ty: &Type,
        doc: &Option<String>,
    ) -> GoDeclaration {
        let plan = self.lower_const(identifier, None, expression, ty, ConstPlacement::Package);
        let documentation = item_documentation(doc, identifier, plan.name.spelling());
        let mut declaration =
            GoDeclaration::new(DeclarationKind::Binding(LoweredStatement::Const(plan)));
        declaration.documentation = documentation;

        declaration
    }
}

impl PlannerQueries<'_, '_> {
    pub(in crate::lowering) fn emit_type_alias(
        &self,
        name: &str,
        generics: &[Generic],
        ty: &Type,
    ) -> GoDeclaration {
        let params: Vec<Type> = generics
            .iter()
            .map(|generic| Type::Parameter(generic.name.clone()))
            .collect();
        let declared_target = self
            .facts
            .definition(&self.facts.qualified_current(name))
            .and_then(|definition| definition.instantiate_alias_target(&params, false));
        let underlying_type = self.component_go_type(declared_target.as_ref().unwrap_or(ty));

        let generics = self.plan_generics(&generics.iter().map(generic_bounds).collect::<Vec<_>>());

        GoDeclaration::new(DeclarationKind::Alias {
            name: go_name::escape_type_name(name).into_owned(),
            parameters: generics,
            underlying: underlying_type,
        })
    }
}
