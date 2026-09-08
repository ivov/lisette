use syntax::parse::TUPLE_FIELDS;
use syntax::types::{CompoundKind, Type};

use crate::Planner;
use crate::control_flow::propagation::plain_return;
use crate::names::go_name::GeneratedPackage;
use crate::plan::bodies::{LoweredBlock, LoweredStatement, assign};
use crate::plan::go_expression::FunctionLiteralLayout;
use crate::plan::values::GoExpression;

impl Planner<'_> {
    pub(crate) fn clone_expression(&mut self, value: GoExpression, ty: &Type) -> GoExpression {
        let peeled = self.facts.peel_alias(ty);
        let prelude = |name: &str| GoExpression::generated(GeneratedPackage::Prelude, name);
        let (function, closure) = match &peeled {
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice,
                args,
                ..
            } => match args.first().and_then(|elem| self.element_clone(elem)) {
                Some(clone) => (prelude("SliceCloneFunc"), Some(clone)),
                None => (
                    GoExpression::generated(GeneratedPackage::Slices, "Clone"),
                    None,
                ),
            },
            Type::Compound {
                kind: CompoundKind::Map,
                args,
                ..
            } => match args.get(1).and_then(|v| self.element_clone(v)) {
                Some(clone) => (prelude("MapCloneFunc"), Some(clone)),
                None => (prelude("MapClone"), None),
            },
            _ => return value,
        };
        let mut arguments = vec![value];
        arguments.extend(closure);
        GoExpression::call(function, arguments)
    }

    fn element_clone(&mut self, ty: &Type) -> Option<GoExpression> {
        if !self.needs_clone(ty) {
            return None;
        }
        let peeled = self.facts.peel_alias(ty);
        let go_ty = self.use_go_type(ty);
        let var = self.fresh_var(Some("e"));
        let parameters = format!("{var} {go_ty}");
        let element = GoExpression::name(var);
        match &peeled {
            Type::Tuple(elems) => {
                let mut statements = self.tuple_clone_statements(&element, elems);
                statements.push(plain_return(element));
                Some(GoExpression::function_literal(
                    parameters,
                    go_ty,
                    LoweredBlock { statements },
                    FunctionLiteralLayout::MultiLine,
                ))
            }
            _ => {
                let body = self.clone_expression(element, ty);
                Some(GoExpression::function_literal(
                    parameters,
                    go_ty,
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
        elems: &[Type],
    ) -> Vec<LoweredStatement> {
        let mut statements = Vec::new();
        for (index, elem) in elems.iter().enumerate() {
            let Some(field) = TUPLE_FIELDS.get(index) else {
                break;
            };
            let field_place = GoExpression::selector(place.clone(), field.to_string());
            let peeled = self.facts.peel_alias(elem);
            match &peeled {
                Type::Compound {
                    kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                    ..
                } => {
                    let clone = self.clone_expression(field_place.clone(), elem);
                    statements.push(assign(field_place, clone));
                }
                Type::Tuple(inner) if self.needs_clone(elem) => {
                    statements.extend(self.tuple_clone_statements(&field_place, inner))
                }
                _ => {}
            }
        }
        statements
    }

    fn needs_clone(&self, ty: &Type) -> bool {
        let peeled = self.facts.peel_alias(ty);
        match &peeled {
            Type::Compound {
                kind: CompoundKind::Slice | CompoundKind::EnumeratedSlice | CompoundKind::Map,
                ..
            } => true,
            Type::Tuple(elems) => elems.iter().any(|e| self.needs_clone(e)),
            _ => false,
        }
    }
}
