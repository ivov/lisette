use crate::lowering::planner::queries::PlannerQueries;
use crate::output::finish::function_literals::function_literal;
use crate::target::names::go_name;
use syntax::types::{CompoundKind, Type};

use crate::Planner;
use crate::lowering::control_flow::propagation::plain_return;
use crate::target::ir::bodies::{LoweredBlock, Statement, assign};
use crate::target::ir::go_expression::{FunctionLiteralLayout, GoParameter};
use crate::target::ir::values::GoExpression;
use crate::target::names::go_name::GeneratedPackage;

impl Planner<'_> {
    pub(in crate::lowering) fn clone_expression(
        &mut self,
        value: GoExpression,
        ty: &Type,
    ) -> GoExpression {
        let prelude = |name: &str| GoExpression::generated(GeneratedPackage::Prelude, name);
        let (function, closure) = match self.facts.compound_parts(ty) {
            Some((CompoundKind::Slice | CompoundKind::EnumeratedSlice, args)) => {
                match args.first().and_then(|element| self.element_clone(element)) {
                    Some(clone) => (prelude("SliceCloneFunc"), Some(clone)),
                    None => (
                        GoExpression::generated(GeneratedPackage::Slices, "Clone"),
                        None,
                    ),
                }
            }
            Some((CompoundKind::Map, args)) => {
                match args.get(1).and_then(|v| self.element_clone(v)) {
                    Some(clone) => (prelude("MapCloneFunc"), Some(clone)),
                    None => (prelude("MapClone"), None),
                }
            }
            _ => return value,
        };

        let mut arguments = vec![value];
        arguments.extend(closure);
        GoExpression::call(function, arguments)
    }

    fn element_clone(&mut self, ty: &Type) -> Option<GoExpression> {
        if !self.queries().needs_clone(ty) {
            return None;
        }
        let go_ty = self.queries().go_type(ty);
        let var = self.fresh_local("e");
        let parameters = vec![GoParameter::new(var.clone(), go_ty.clone())];
        let element = GoExpression::identifier(var);
        match self.facts.tuple_elements(ty) {
            Some(elements) => {
                let mut statements = self.tuple_clone_statements(&element, &elements);
                statements.push(plain_return(element));
                Some(function_literal(
                    parameters,
                    Some(go_ty),
                    LoweredBlock { statements },
                    FunctionLiteralLayout::MultiLine,
                ))
            }
            _ => {
                let body = self.clone_expression(element, ty);
                Some(function_literal(
                    parameters,
                    Some(go_ty),
                    LoweredBlock {
                        statements: vec![plain_return(body)],
                    },
                    FunctionLiteralLayout::Inline,
                ))
            }
        }
    }

    fn tuple_clone_statements(
        &mut self,
        place: &GoExpression,
        elements: &[Type],
    ) -> Vec<Statement> {
        let mut statements = Vec::new();
        for (index, element) in elements.iter().enumerate() {
            let field_place =
                GoExpression::selector(place.clone(), go_name::tuple_field(index).to_string());
            if self.facts.is_clone_container(element) {
                let clone = self.clone_expression(field_place.clone(), element);
                statements.push(assign(field_place, clone));
            } else if let Some(inner) = self.facts.tuple_elements(element)
                && self.queries().needs_clone(element)
            {
                statements.extend(self.tuple_clone_statements(&field_place, &inner))
            }
        }
        statements
    }
}

impl PlannerQueries<'_, '_> {
    fn needs_clone(&self, ty: &Type) -> bool {
        match self.facts.tuple_elements(ty) {
            Some(elements) => elements.iter().any(|e| self.needs_clone(e)),
            None => self.facts.is_clone_container(ty),
        }
    }
}
