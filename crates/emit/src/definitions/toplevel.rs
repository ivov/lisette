use crate::Planner;
use crate::Renderer;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use crate::plan::bodies::ConstPlan;
use crate::plan::values::ValuePlan;
use syntax::ast::{Expression, Generic};
use syntax::types::Type;

#[derive(Clone, Copy)]
pub(crate) enum ConstScope {
    Package,
    Local,
}

impl Planner<'_> {
    pub(crate) fn emit_type_alias(
        &mut self,
        name: &str,
        generics: &[Generic],
        ty: &Type,
    ) -> String {
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
        let ty_string = self.use_go_type(underlying);

        let generics_string = self.generics_to_string(generics);

        let separator = if is_fn_alias { " " } else { " = " };
        format!(
            "type {}{}{}{}",
            go_name::escape_type_name(name),
            generics_string,
            separator,
            ty_string
        )
    }

    pub(crate) fn build_const_plan(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
        scope: ConstScope,
    ) -> ConstPlan {
        let target_name = self
            .package
            .escape_remap(identifier)
            .map(str::to_string)
            .unwrap_or_else(|| go_name::screaming_snake_to_camel(identifier));
        let initial_go_name = self.scope.bind(identifier, target_name);
        let go_identifier = if self.try_declare(&initial_go_name) {
            initial_go_name
        } else {
            let fresh = self.fresh_var(Some(identifier));
            self.scope.bind(identifier, &fresh);
            self.try_declare(&fresh);
            fresh
        };
        let ty_str = self.use_go_type(ty);

        // `is_go_constant_expression` admits only literals, identifiers, and
        // constexpr unary/binary, none of which carry setup statements.
        let raw_value = self.lower_value(expression, ExpressionContext::value());
        let value_text = raw_value.rendered();
        let value = if value_text.is_empty() {
            ValuePlan::opaque("struct{}{}".to_string())
        } else {
            ValuePlan::opaque(value_text)
        };
        let is_const = self.is_go_constant_expression(expression);
        if is_const && matches!(scope, ConstScope::Local) {
            self.scope.mark_go_const(identifier);
        }
        ConstPlan {
            is_const,
            name: go_identifier,
            ty_str,
            value,
        }
    }

    pub(crate) fn emit_const(
        &mut self,
        identifier: &str,
        expression: &Expression,
        ty: &Type,
    ) -> String {
        let plan = self.build_const_plan(identifier, expression, ty, ConstScope::Package);
        let mut out = String::new();
        Renderer.render_const_declaration(&mut out, &plan);
        out.trim_end_matches('\n').to_string()
    }
}
