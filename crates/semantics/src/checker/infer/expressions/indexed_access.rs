use syntax::ast::Expression;
use syntax::types::Type;

use super::super::super::Checker;

impl Checker<'_, '_> {
    pub(super) fn infer_indexed_access(
        &mut self,
        expression: Box<Expression>,
        index: Box<Expression>,
        span: syntax::ast::Span,
        expected_ty: &Type,
    ) -> Expression {
        if index.is_range() {
            return self.infer_slice_range_access(expression, index, span, expected_ty);
        }

        let index_ty_var = self.new_type_var();
        let collection_ty_var = self.new_type_var();

        let (index_expression, collection_expression) = self.with_value_context(|s| {
            let index_expression = s.infer_expression(*index, &index_ty_var);
            let collection_expression = s.infer_expression(*expression, &collection_ty_var);
            (index_expression, collection_expression)
        });

        self.check_not_temp_producing(&index_expression);

        let resolved_index_ty = index_ty_var.resolve();
        let resolved_collection_ty = self.peel_alias(&collection_ty_var.resolve());

        if resolved_collection_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: index_expression.into(),
                ty: Type::Error,
                span,
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
            };
        };

        // Handle range-typed variables used as slice indices (e.g. `let r = 2..5; items[r]`).
        // Inline ranges are caught by `index.is_range()` above; this handles the case where
        // the range is stored in a variable.
        let is_range_index = (type_name == "Slice" || type_name == "string")
            && resolved_index_ty.get_name().is_some_and(|n| {
                matches!(
                    n,
                    "Range" | "RangeInclusive" | "RangeFrom" | "RangeTo" | "RangeToInclusive"
                )
            });

        if is_range_index {
            if let Some(bound_ty) = resolved_index_ty.get_type_params().and_then(|p| p.first()) {
                let int_ty = self.type_int();
                self.unify(&int_ty, bound_ty, &span);
            }

            let result_ty = if type_name == "string" {
                self.type_string()
            } else {
                let element_ty = resolved_collection_ty
                    .get_type_params()
                    .and_then(|params| params.first().cloned())
                    .unwrap_or_else(|| self.new_type_var());
                self.type_slice(element_ty)
            };
            self.unify(expected_ty, &result_ty, &span);

            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: index_expression.into(),
                ty: result_ty,
                span,
            };
        }

        let element_ty = self.new_type_var();

        let (expected_index_ty, expected_collection_ty) = match type_name {
            "Slice" => (self.type_int(), self.type_slice(element_ty.clone())),
            "Map" => {
                let Some(type_params) = resolved_collection_ty.get_type_params() else {
                    self.unify(expected_ty, &Type::Error, &span);
                    return Expression::IndexedAccess {
                        expression: collection_expression.into(),
                        index: index_expression.into(),
                        ty: Type::Error,
                        span,
                    };
                };
                let key_ty = &type_params[0];
                (
                    key_ty.clone(),
                    self.type_map(key_ty.clone(), element_ty.clone()),
                )
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
                };
            }
        };

        self.unify(&expected_collection_ty, &resolved_collection_ty, &span);

        let index_ok =
            self.check_slice_index_type(type_name, &resolved_index_ty, index_expression.get_span());

        if index_ok {
            self.unify(&expected_index_ty, &resolved_index_ty, &span);
        }
        self.unify(expected_ty, &element_ty, &span);

        Expression::IndexedAccess {
            expression: collection_expression.into(),
            index: index_expression.into(),
            ty: element_ty,
            span,
        }
    }

    fn infer_slice_range_access(
        &mut self,
        expression: Box<Expression>,
        range: Box<Expression>,
        span: syntax::ast::Span,
        expected_ty: &Type,
    ) -> Expression {
        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &collection_ty_var));
        let resolved_collection_ty = self.peel_alias(&collection_ty_var.resolve());

        if resolved_collection_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
            };
        }

        let Some(type_name) = resolved_collection_ty.get_name() else {
            self.sink.push(diagnostics::infer::type_must_be_known(
                collection_expression.get_span(),
            ));
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
            };
        };

        if type_name != "Slice" && type_name != "string" {
            self.sink
                .push(diagnostics::infer::only_slices_indexable_by_range(
                    &resolved_collection_ty,
                    &collection_expression.get_span(),
                ));
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
            };
        }

        let is_string = type_name == "string";

        let element_ty = if is_string {
            self.new_type_var() // not used for result, but range bounds still need inferring
        } else {
            resolved_collection_ty
                .get_type_params()
                .and_then(|params| params.first().cloned())
                .unwrap_or_else(|| self.new_type_var())
        };

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
                    let start =
                        start.map(|expression| Box::new(s.infer_expression(*expression, &int_ty)));
                    let end =
                        end.map(|expression| Box::new(s.infer_expression(*expression, &int_ty)));
                    (start, end)
                });

                if let Some(s) = &new_start {
                    self.check_not_temp_producing(s);
                }
                if let Some(e) = &new_end {
                    self.check_not_temp_producing(e);
                }

                let range_ty = match (&new_start, &new_end) {
                    (Some(_), Some(_)) if inclusive => self.type_range_inclusive(int_ty),
                    (Some(_), Some(_)) => self.type_range(int_ty),
                    (Some(_), None) => self.type_range_from(int_ty),
                    (None, Some(_)) if inclusive => self.type_range_to_inclusive(int_ty),
                    (None, Some(_)) => self.type_range_to(int_ty),
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

        let result_ty = if is_string {
            self.type_string()
        } else {
            self.type_slice(element_ty)
        };

        self.unify(expected_ty, &result_ty, &span);

        Expression::IndexedAccess {
            expression: collection_expression.into(),
            index: range_expression.into(),
            ty: result_ty,
            span,
        }
    }

    fn infer_range_bounds_only(&mut self, range: Box<Expression>) -> Expression {
        match *range {
            Expression::Range {
                start,
                end,
                inclusive,
                span,
                ..
            } => {
                let int_ty = self.type_int();
                let new_start = start.map(|s| Box::new(self.infer_expression(*s, &int_ty)));
                let new_end = end.map(|e| Box::new(self.infer_expression(*e, &int_ty)));

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
