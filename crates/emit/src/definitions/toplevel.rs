use crate::Planner;
use crate::expressions::top_items::item_doc_text;
use crate::ir::bodies::{ConstPlan, LoweredStatement};
use crate::ir::declarations::{DeclarationKind, GoDeclaration};
use crate::ir::values::GoExpression;
use crate::names::generics::generic_bounds;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use crate::state::expression::ExpressionContext;
use syntax::EcoString;
use syntax::ast::BindingId;
use syntax::ast::{Expression, Generic};
use syntax::constant;
use syntax::types::{SimpleKind, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstPlacement {
    Package,
    Local,
}

impl Planner<'_> {
    pub(crate) fn emit_type_alias(
        &mut self,
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
        let underlying_type = self.go_type(declared_target.as_ref().unwrap_or(ty));

        let generics = self.plan_generics(&generics.iter().map(generic_bounds).collect::<Vec<_>>());

        GoDeclaration::new(DeclarationKind::Alias {
            name: go_name::escape_type_name(name).into_owned(),
            parameters: generics,
            underlying: underlying_type,
        })
    }

    pub(crate) fn lower_const(
        &mut self,
        identifier: &EcoString,
        binding: Option<BindingId>,
        expression: &Expression,
        ty: &Type,
        placement: ConstPlacement,
    ) -> LoweredStatement {
        let name = SourceName::constant(identifier);
        let preferred = name.preferred();
        let hides_package_name =
            placement == ConstPlacement::Local && self.package.is_package_block_name(&preferred);
        let go_name = if !hides_package_name && self.try_declare(&preferred) {
            self.scope.new_local_identifier(preferred)
        } else {
            self.fresh_declared_source(&name)
        };
        let go_type = (!matches!(ty, Type::Simple(SimpleKind::String | SimpleKind::Bool)))
            .then(|| self.go_type(ty));

        if let Some(binding) = binding
            && let Some(constant) = constant::local_constant(expression, ty, self)
        {
            self.scope.record_local_constant(binding, constant);
        }
        let raw_value = self.plan_value(expression, ExpressionContext::value());
        let value = if raw_value.is_empty() {
            GoExpression::unit()
        } else {
            raw_value.into_parts().1
        };
        LoweredStatement::Const(ConstPlan {
            name: self
                .scope
                .bind_go_const(identifier, binding.as_slice(), go_name),
            go_type,
            value,
        })
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &EcoString,
        expression: &Expression,
        ty: &Type,
        doc: &Option<String>,
    ) -> GoDeclaration {
        let binding = self.lower_const(identifier, None, expression, ty, ConstPlacement::Package);
        let mut declaration = GoDeclaration::new(DeclarationKind::Binding(binding));
        let go_name = self
            .scope
            .bound_go_identifier(identifier)
            .expect("`lower_const` binds the identifier");
        declaration.documentation = item_doc_text(doc, identifier, go_name.spelling()).into();
        declaration
    }
}
