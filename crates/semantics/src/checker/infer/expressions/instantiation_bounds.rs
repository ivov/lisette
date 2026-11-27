use syntax::ast::{Annotation, Expression, Generic, Span};
use syntax::types::Type;

use super::call_effects::CallKind;
use crate::checker::infer::InferCtx;

impl InferCtx<'_> {
    pub(crate) fn check_instantiation_bounds(&mut self, items: &[Expression]) {
        for item in items {
            self.visit_instantiations(item);
        }
    }

    fn visit_instantiations(&mut self, expression: &Expression) {
        match expression {
            Expression::Function {
                generics,
                return_type,
                ..
            } => {
                self.with_scope(|this| {
                    this.put_generic_bounds_in_scope(generics);
                    this.scopes.set_fn_return_type(return_type.clone());
                    this.visit_children(expression);
                });
                return;
            }
            Expression::Lambda {
                ty,
                return_annotation,
                ..
            } => {
                let return_type = match return_annotation {
                    Annotation::Unknown => Type::unit(),
                    _ => ty.get_function_ret().cloned().unwrap_or(Type::Error),
                };
                self.with_scope(|this| {
                    this.scopes.mark_lambda_scope();
                    this.scopes.set_fn_return_type(return_type);
                    this.visit_children(expression);
                });
                return;
            }
            Expression::Task { .. } => {
                self.with_scope(|this| {
                    this.scopes.mark_lambda_scope();
                    this.scopes.set_fn_return_type(Type::unit());
                    this.visit_children(expression);
                });
                return;
            }
            Expression::ImplBlock { generics, ty, .. } => {
                let store = self.store;
                self.with_scope(|this| {
                    this.put_generic_bounds_in_scope(generics);
                    if let Type::Nominal { id, .. } = ty
                        && this.impl_has_simple_type_params(ty, generics)
                    {
                        this.register_receiver_type_bounds(store, id, generics);
                    }
                    this.visit_children(expression);
                });
                return;
            }
            Expression::Call {
                expression: callee,
                ty,
                span,
                ..
            } => {
                self.check_tuple_struct_instantiation(callee, ty, *span);
            }
            Expression::StructCall { name, ty, span, .. } => {
                self.check_struct_call_instantiation(name, ty, *span);
            }
            Expression::Identifier {
                value, ty, span, ..
            } => {
                let store = self.store;
                if let Some(enum_id) = self.enum_of_variant(store, value) {
                    let constructed = match ty {
                        Type::Function(function) => function.return_type.as_ref(),
                        other => other,
                    };
                    if matches!(constructed, Type::Nominal { .. }) {
                        self.register_construction_obligations(&enum_id, constructed, *span);
                    }
                }
                self.check_function_value_instantiation(value, ty, *span);
            }
            Expression::DotAccess {
                member, ty, span, ..
            } => {
                self.check_function_value_instantiation(member, ty, *span);
            }
            _ => {}
        }
        self.visit_children(expression);
    }

    fn visit_children(&mut self, expression: &Expression) {
        expression.for_each_child(|child| {
            self.visit_instantiations(child);
        });
    }

    fn put_generic_bounds_in_scope(&mut self, generics: &[Generic]) {
        self.put_in_scope(generics);
        for generic in generics {
            for bound in generic.resolved_bounds().into_iter().flatten() {
                self.scopes.insert_trait_bound(&generic.name, bound.clone());
            }
        }
    }

    fn check_tuple_struct_instantiation(
        &mut self,
        callee: &Expression,
        return_type: &Type,
        span: Span,
    ) {
        if self.classify_call(callee) == CallKind::TupleStructConstructor
            && let Type::Nominal { id, .. } = self.store.peel_alias(return_type)
        {
            let written_name = callee
                .unwrap_parens()
                .as_dotted_path()
                .unwrap_or_else(|| id.to_string());
            self.register_construction_obligations(&written_name, return_type, span);
        }
    }

    fn check_struct_call_instantiation(&mut self, name: &str, ty: &Type, span: Span) {
        let store = self.store;
        let Type::Nominal { id, .. } = store.peel_alias(ty) else {
            return;
        };
        let written_name = if store.variants_of(&id).is_some() {
            id.to_string()
        } else {
            name.to_string()
        };
        self.register_construction_obligations(&written_name, ty, span);
    }

    fn check_function_value_instantiation(&mut self, written_name: &str, ty: &Type, span: Span) {
        if self.function_values_without_context.contains(&span) {
            self.register_function_value_obligations(written_name, ty, span);
        }
    }
}
