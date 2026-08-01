use super::*;

fn canonical_const_literal(expression: &Expression) -> Option<syntax::ast::Literal> {
    use syntax::ast::{Literal, UnaryOperator};
    match expression.unwrap_parens() {
        Expression::Literal { literal, .. } => match literal {
            Literal::Integer { value, .. } => Some(Literal::Integer {
                value: *value,
                text: None,
            }),
            Literal::Float { value, .. } => Some(Literal::Float {
                value: *value,
                text: None,
            }),
            Literal::Boolean(b) => Some(Literal::Boolean(*b)),
            Literal::String { value, .. } => Some(Literal::String {
                value: value.clone(),
                raw: false,
            }),
            Literal::Char(c) => Some(Literal::Char(c.clone())),
            _ => None,
        },
        Expression::Unary {
            operator: UnaryOperator::Negative,
            expression,
            ..
        } => match canonical_const_literal(expression)? {
            Literal::Integer { value, .. } => Some(Literal::Integer {
                value: value.wrapping_neg(),
                text: None,
            }),
            Literal::Float { value, .. } => Some(Literal::Float {
                value: -value,
                text: None,
            }),
            _ => None,
        },
        _ => None,
    }
}

impl TaskState {
    fn compute_item_visibility(
        &self,
        store: &Store,
        syntactic: &SyntacticVisibility,
        scope: &Visibility,
    ) -> Visibility {
        match scope {
            Visibility::Local => Visibility::Local,
            _ if *syntactic == SyntacticVisibility::Public || self.is_d_lis(store) => {
                Visibility::Public
            }
            _ => Visibility::Private,
        }
    }

    pub(crate) fn register_values(
        &mut self,
        store: &mut Store,
        items: &[Expression],
        visibility: &Visibility,
    ) {
        for item in items {
            match item {
                Expression::Function { .. } => {
                    self.register_function_value(store, item, visibility)
                }
                Expression::Const { .. } => self.register_const_value(store, item, visibility),
                Expression::VariableDeclaration { .. } => {
                    self.register_variable_declaration(store, item, visibility)
                }
                Expression::Struct {
                    fields: StructFields::Tuple(_),
                    ..
                } => self.register_tuple_struct_constructor(store, item),
                _ => (),
            }
        }
    }

    fn register_function_value(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::Function {
            name,
            name_span,
            attributes,
            generics,
            params,
            return_annotation,
            span,
            body,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        if body.definition().is_none() && self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::bodyless_function_outside_typedef(*span));
        }

        let qualified_name = self.qualify_name(name);

        let fn_ty = self.with_scope(|this| {
            this.put_in_scope(generics);

            let test_params;
            let params: &[Binding] = if syntax::attributes::has_test_attribute(attributes) {
                test_params = test_functions::normalize_test_params(params.clone(), true);
                &test_params
            } else {
                params
            };

            let fn_ty =
                this.extract_signature_parts(store, generics, params, return_annotation, span);

            let (signature_pairs, signature_bounds) =
                function_signature_pairs(&fn_ty, params, *span);
            for bound in &signature_bounds {
                this.record_generic_bound(&bound.param_name, bound.ty.clone());
            }
            this.check_value_position_bounds(store, &[], &signature_pairs);
            fn_ty
        });

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "function", name, *name_span,
            ));
        }

        let module = self.current_module_mut(store);
        module.definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: fn_ty,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: extract_attribute_flags(attributes, "allow"),
                    go_hints: extract_attribute_flags(attributes, "go"),
                    go_name: extract_go_name(attributes),
                    go_type_param_recipe: extract_go_type_param_recipe(attributes),
                },
            },
        );
    }

    fn register_const_value(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::Const {
            identifier,
            identifier_span,
            annotation: maybe_annotation,
            expression,
            span,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        let has_value = expression.value().is_some();

        if !has_value && self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::valueless_const_outside_typedef(*span));
        }

        if !has_value && maybe_annotation.is_none() && self.is_d_lis(&*store) {
            self.sink
                .push(diagnostics::infer::valueless_const_missing_annotation(
                    *span,
                ));
        }

        let qualified_name = self.qualify_name(identifier);

        let const_ty = self.without_diagnostics(|this| {
            if let Some(annotation) = maybe_annotation {
                this.convert_to_type(store, annotation, span)
            } else {
                expression
                    .value()
                    .and_then(|value| this.type_from_literal_expression(value))
                    .unwrap_or_else(|| this.new_type_var())
            }
        });

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        if self.is_lis(&*store) && self.definition_exists(&*store, &qualified_name) {
            self.sink.push(diagnostics::infer::duplicate_definition(
                "constant",
                identifier,
                *identifier_span,
            ));
        }

        let kind = match expression.value().and_then(canonical_const_literal) {
            Some(value) => syntax::program::ValueKind::Constant(value),
            None => syntax::program::ValueKind::ConstantDeclaration,
        };

        self.current_module_mut(store).definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: const_ty,
                name_span: Some(*identifier_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            },
        );
    }

    fn register_variable_declaration(
        &mut self,
        store: &mut Store,
        item: &Expression,
        visibility: &Visibility,
    ) {
        let Expression::VariableDeclaration {
            name,
            name_span,
            annotation,
            span,
            visibility: syntactic_visibility,
            doc,
            ..
        } = item
        else {
            return;
        };

        if self.is_lis(&*store) {
            self.sink
                .push(diagnostics::infer::variable_declaration_outside_typedef(
                    *span,
                ));
        }

        let qualified_name = self.qualify_name(name);
        let var_ty = self.convert_to_type(&*store, annotation, span);

        let item_visibility =
            self.compute_item_visibility(&*store, syntactic_visibility, visibility);

        let module = self.current_module_mut(store);
        module.definitions.insert(
            qualified_name,
            Definition {
                visibility: item_visibility,
                ty: var_ty,
                name_span: Some(*name_span),
                doc: doc.clone(),
                body: DefinitionBody::Value {
                    kind: syntax::program::ValueKind::Runtime,
                    allowed_lints: vec![],
                    go_hints: vec![],
                    go_name: None,
                    go_type_param_recipe: None,
                },
            },
        );
    }

    fn register_tuple_struct_constructor(&mut self, store: &mut Store, item: &Expression) {
        let Expression::Struct {
            name,
            fields: StructFields::Tuple(_),
            ..
        } = item
        else {
            return;
        };

        let qualified_name = self.qualify_name(name);
        let constructor_ty = store
            .get_definition(&qualified_name)
            .and_then(Definition::constructor_type)
            .expect("tuple struct definition must have a constructor type");

        let scope = self.scopes.current_mut();
        scope.insert_value(qualified_name.to_string(), constructor_ty.clone());
        scope.insert_value(name.to_string(), constructor_ty.clone());
    }

    pub(crate) fn extract_signature_parts(
        &mut self,
        store: &Store,
        generics: &[Generic],
        params: &[Binding],
        return_annotation: &Annotation,
        span: &Span,
    ) -> Type {
        let (generics, bounds, param_types, return_ty) = self.with_scope(|this| {
            this.put_in_scope(generics);
            let generics = this.resolve_generic_bounds(store, generics, span);
            this.check_transitive_generic_bounds(store, &generics, *span);
            let bounds = resolved_generic_bounds(&generics);

            let (param_types, return_ty) = this.without_diagnostics(|this| {
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|binding| match &binding.annotation {
                        Some(a) => this.convert_variadic_to_type(store, a, span),
                        None if !binding.ty.is_uninferred() => binding.ty.clone(),
                        None => this.new_type_var(),
                    })
                    .collect();

                let return_ty = match return_annotation {
                    Annotation::Unknown => this.type_unit(),
                    _ => this.convert_to_type(store, return_annotation, span),
                };
                (param_types, return_ty)
            });
            (generics, bounds, param_types, return_ty)
        });

        let function_params = param_types
            .into_iter()
            .zip(params)
            .map(|(ty, binding)| {
                FunctionParameter::named(ty, binding.pattern.get_identifier(), binding.is_mutable())
            })
            .collect();

        let base_fn_ty = Type::function(function_params, bounds, return_ty.into());

        if generics.is_empty() {
            base_fn_ty
        } else {
            Type::Forall {
                vars: generics.iter().map(|g| g.name.clone()).collect(),
                body: Box::new(base_fn_ty),
            }
        }
    }
}
