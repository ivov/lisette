use crate::checker::EnvResolve;
use crate::store::Store;
use syntax::ast::{Expression, Span};
use syntax::types::{CompoundKind, Type, peel_to_range_type};

use super::super::super::TaskState;

impl TaskState<'_> {
    /// Returns `true` if valid (no error emitted), `false` if an error was emitted.
    pub(crate) fn check_slice_index_type(
        &mut self,
        type_name: &str,
        index_ty: &Type,
        span: Span,
    ) -> bool {
        if type_name != "Slice" || index_ty.is_variable() || index_ty.is_error() {
            return true;
        }
        if index_ty.get_name().is_some_and(|n| n == "int") {
            return true;
        }
        self.sink
            .push(diagnostics::infer::slice_index_type_mismatch(
                index_ty, span,
            ));
        false
    }

    pub(super) fn infer_indexed_access(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        index: Box<Expression>,
        span: syntax::ast::Span,
        expected_ty: &Type,
    ) -> Expression {
        if index.is_range() {
            return self.infer_slice_range_access(store, expression, index, span, expected_ty);
        }

        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(store, *expression, &collection_ty_var));

        let resolved_collection_ty = store.peel_alias(&collection_ty_var.resolve_in(&self.env));

        let index_expected_ty = match resolved_collection_ty.as_compound() {
            Some((CompoundKind::Map, args)) => {
                args.first().cloned().unwrap_or_else(|| self.new_type_var())
            }
            _ => self.new_type_var(),
        };

        let index_expression =
            self.with_value_context(|s| s.infer_expression(store, *index, &index_expected_ty));

        let resolved_index_ty = index_expected_ty.resolve_in(&self.env);

        if resolved_collection_ty.is_error() {
            self.unify(store, expected_ty, &Type::Error, &span);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: index_expression.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        }

        let Some(type_name) = resolved_collection_ty.get_name() else {
            self.sink.push(diagnostics::infer::type_must_be_known(
                collection_expression.get_span(),
            ));
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: index_expression.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        };

        // Handle a stored range used as an index (e.g. `let r = 2..5; items[r]`).
        // Inline ranges are caught by `index.is_range()` above. Strings are
        // rejected; slices return a sub-slice.
        match (peel_to_range_type(&resolved_index_ty), type_name) {
            (Some(_), "string") => {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                let full_span = collection_expression.get_span().merge(span);
                self.sink.push(diagnostics::infer::string_not_sliceable(
                    full_span, receiver,
                ));
                return Expression::IndexedAccess {
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
            (Some(peeled), "Slice") => {
                if let Some(bound_ty) = peeled.get_type_params().and_then(|p| p.first()) {
                    let int_ty = self.type_int();
                    self.unify(store, &int_ty, bound_ty, &span);
                }

                let element_ty = resolved_collection_ty
                    .get_type_params()
                    .and_then(|params| params.first().cloned())
                    .unwrap_or_else(|| self.new_type_var());
                let result_ty = self.type_slice(element_ty);
                self.unify(store, expected_ty, &result_ty, &span);

                return Expression::IndexedAccess {
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: result_ty,
                    span,
                    from_colon_syntax: false,
                };
            }
            _ => {}
        }

        let element_ty = self.new_type_var();

        let (expected_index_ty, expected_collection_ty) = match type_name {
            "Slice" => (self.type_int(), self.type_slice(element_ty.clone())),
            "Map" => {
                let Some(type_params) = resolved_collection_ty.get_type_params() else {
                    self.unify(store, expected_ty, &Type::Error, &span);
                    return Expression::IndexedAccess {
                        expression: collection_expression.into(),
                        index: index_expression.into(),
                        ty: Type::Error,
                        span,
                        from_colon_syntax: false,
                    };
                };
                let key_ty = &type_params[0];
                (
                    key_ty.clone(),
                    self.type_map(key_ty.clone(), element_ty.clone()),
                )
            }
            "string" => {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                self.sink.push(diagnostics::infer::string_not_indexable(
                    collection_expression.get_span(),
                    receiver,
                ));
                return Expression::IndexedAccess {
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
            _ => {
                self.sink
                    .push(diagnostics::infer::only_slices_and_maps_indexable(
                        &resolved_collection_ty,
                        collection_expression.get_span(),
                    ));
                return Expression::IndexedAccess {
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
        };

        self.unify(
            store,
            &expected_collection_ty,
            &resolved_collection_ty,
            &span,
        );

        let index_ok =
            self.check_slice_index_type(type_name, &resolved_index_ty, index_expression.get_span());

        if index_ok {
            self.unify(store, &expected_index_ty, &resolved_index_ty, &span);
        }
        self.unify(store, expected_ty, &element_ty, &span);

        Expression::IndexedAccess {
            expression: collection_expression.into(),
            index: index_expression.into(),
            ty: element_ty,
            span,
            from_colon_syntax: false,
        }
    }

    /// Emits a type-aware diagnostic for Go-style `expr[a:b]` colon syntax.
    pub(super) fn infer_colon_subscript(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        index: Box<Expression>,
        span: syntax::ast::Span,
    ) -> Expression {
        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(store, *expression, &collection_ty_var));
        let resolved_collection_ty = store.peel_alias(&collection_ty_var.resolve_in(&self.env));

        let receiver = collection_expression.root_identifier().unwrap_or("s");
        let full_span = collection_expression.get_span().merge(span);
        let type_name = resolved_collection_ty.get_name();
        self.sink.push(diagnostics::infer::colon_in_subscript(
            full_span, receiver, type_name,
        ));

        Expression::IndexedAccess {
            expression: collection_expression.into(),
            index,
            ty: Type::Error,
            span,
            from_colon_syntax: true,
        }
    }

    fn infer_slice_range_access(
        &mut self,
        store: &mut Store,
        expression: Box<Expression>,
        range: Box<Expression>,
        span: syntax::ast::Span,
        expected_ty: &Type,
    ) -> Expression {
        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(store, *expression, &collection_ty_var));
        let resolved_collection_ty = store.peel_alias(&collection_ty_var.resolve_in(&self.env));

        if resolved_collection_ty.is_error() {
            self.unify(store, expected_ty, &Type::Error, &span);
            let inferred_range = self.infer_range_bounds_only(store, range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        }

        let Some(type_name) = resolved_collection_ty.get_name() else {
            self.sink.push(diagnostics::infer::type_must_be_known(
                collection_expression.get_span(),
            ));
            let inferred_range = self.infer_range_bounds_only(store, range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        };

        if type_name != "Slice" {
            let diagnostic = if type_name == "string" {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                let full_span = collection_expression.get_span().merge(span);
                diagnostics::infer::string_not_sliceable(full_span, receiver)
            } else {
                diagnostics::infer::only_slices_indexable_by_range(
                    &resolved_collection_ty,
                    &collection_expression.get_span(),
                )
            };
            self.sink.push(diagnostic);
            let inferred_range = self.infer_range_bounds_only(store, range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        }

        let element_ty = resolved_collection_ty
            .get_type_params()
            .and_then(|params| params.first().cloned())
            .unwrap_or_else(|| self.new_type_var());

        let range_expression = match *range {
            Expression::Range {
                start,
                end,
                inclusive,
                span: range_span,
                ..
            } => {
                let int_ty = self.type_int();

                let (new_start, new_end) = self.with_value_context(|s| {
                    let start = start.map(|expression| {
                        Box::new(s.infer_expression(store, *expression, &int_ty))
                    });
                    let end = end.map(|expression| {
                        Box::new(s.infer_expression(store, *expression, &int_ty))
                    });
                    (start, end)
                });

                let range_ty = match (&new_start, &new_end) {
                    (Some(_), Some(_)) if inclusive => self.type_range_inclusive(store, int_ty),
                    (Some(_), Some(_)) => self.type_range(store, int_ty),
                    (Some(_), None) => self.type_range_from(store, int_ty),
                    (None, Some(_)) if inclusive => self.type_range_to_inclusive(store, int_ty),
                    (None, Some(_)) => self.type_range_to(store, int_ty),
                    (None, None) => self.new_type_var(),
                };

                Expression::Range {
                    start: new_start,
                    end: new_end,
                    inclusive,
                    ty: range_ty,
                    span: range_span,
                }
            }
            _ => unreachable!("infer_slice_range_access called with non-range expression"),
        };

        let result_ty = self.type_slice(element_ty);

        self.unify(store, expected_ty, &result_ty, &span);

        Expression::IndexedAccess {
            expression: collection_expression.into(),
            index: range_expression.into(),
            ty: result_ty,
            span,
            from_colon_syntax: false,
        }
    }

    fn infer_range_bounds_only(&mut self, store: &mut Store, range: Box<Expression>) -> Expression {
        match *range {
            Expression::Range {
                start,
                end,
                inclusive,
                span,
                ..
            } => {
                let int_ty = self.type_int();
                let new_start = start.map(|s| Box::new(self.infer_expression(store, *s, &int_ty)));
                let new_end = end.map(|e| Box::new(self.infer_expression(store, *e, &int_ty)));

                Expression::Range {
                    start: new_start,
                    end: new_end,
                    inclusive,
                    ty: self.new_type_var(),
                    span,
                }
            }
            other => other,
        }
    }
}
