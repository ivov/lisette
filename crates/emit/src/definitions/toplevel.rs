use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::names::source_name::SourceName;
use crate::plan::bodies::{ConstPlan, LoweredStatement};
use crate::plan::declarations::{DeclarationKind, GoDeclaration};
use crate::plan::values::GoExpression;
use crate::types::go_type::GoType;
use syntax::EcoString;
use syntax::ast::BindingId;
use syntax::ast::{Expression, Generic};
use syntax::program::SlotOrigin;
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
        let function_target = declared_target
            .as_ref()
            .and_then(|target| self.facts.resolve_to_function_type(target));
        let is_fn_alias = function_target.is_some();
        let underlying = function_target
            .as_ref()
            .or(declared_target.as_ref())
            .unwrap_or(ty);
        let underlying_type = if is_fn_alias {
            let target = declared_target.as_ref().unwrap_or(ty);
            let layout = self.value_layout(target, SlotOrigin::Lisette);
            layout.go_type(self)
        } else {
            self.go_type(underlying)
        };

        let generics = self.plan_generics(
            &generics
                .iter()
                .map(|generic| {
                    (
                        generic.name.clone(),
                        generic
                            .resolved_bounds()
                            .expect("generic bounds are resolved before emission")
                            .cloned()
                            .collect(),
                    )
                })
                .collect::<Vec<_>>(),
        );

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
        let is_const = self.is_go_constant_expression(expression);
        // An untyped string or bool constant also assigns to named types.
        let go_type = (!is_const
            || !matches!(ty, Type::Simple(SimpleKind::String | SimpleKind::Bool)))
        .then(|| self.go_type(ty));

        // `is_go_constant_expression` admits only literals, identifiers, and
        // constexpr unary/binary, none of which carry setup statements.
        let raw_value = self.lower_value(expression, ExpressionContext::value());
        let value = if raw_value.is_empty() {
            GoExpression::empty_composite(GoType::new("struct{}"))
        } else {
            raw_value.into_parts().1
        };
        if is_const {
            LoweredStatement::Const(ConstPlan {
                name: self
                    .scope
                    .bind_go_const(identifier, binding.as_slice(), go_name),
                go_type,
                value,
            })
        } else {
            LoweredStatement::VarDecl {
                name: self
                    .scope
                    .bind_source(identifier, binding.as_slice(), go_name),
                go_type: go_type.expect("a non-constant `var` names its type"),
                value: Some(value),
            }
        }
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &EcoString,
        expression: &Expression,
        ty: &Type,
    ) -> GoDeclaration {
        let binding = self.lower_const(identifier, None, expression, ty, ConstPlacement::Package);
        GoDeclaration::new(DeclarationKind::Binding(binding))
    }
}
