use crate::checker::EnvResolve;
use syntax::ast::{Expression, Span};
use syntax::types::{AliasPeeled, CompoundKind, SimpleKind, Type, peel_to_range_type};

use crate::checker::infer::InferCtx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexKind {
    Slice,
    Array,
    Map,
    String,
    NotIndexable,
}

impl IndexKind {
    fn of(peeled: &AliasPeeled) -> Option<Self> {
        Some(match peeled.ty() {
            Type::Array { .. } => Self::Array,
            Type::Simple(SimpleKind::String) => Self::String,
            Type::Compound {
                kind: CompoundKind::Slice,
                ..
            } => Self::Slice,
            Type::Compound {
                kind: CompoundKind::Map,
                ..
            } => Self::Map,
            Type::Simple(_)
            | Type::Compound { .. }
            | Type::Nominal { .. }
            | Type::ImportNamespace(_) => Self::NotIndexable,
            _ => return None,
        })
    }
}

impl InferCtx<'_> {
    fn index_kind(&self, peeled: &AliasPeeled) -> Option<IndexKind> {
        match peeled.pointee(|id| self.store.get_definition(id)) {
            Some(pointee) => self.index_kind(&pointee),
            None => IndexKind::of(peeled),
        }
    }

    /// Returns `true` if valid (no error emitted), `false` if an error was emitted.
    fn check_slice_index_type(&mut self, kind: IndexKind, index_ty: &Type, span: Span) -> bool {
        if kind != IndexKind::Slice || index_ty.is_variable() || index_ty.is_error() {
            return true;
        }
        // A `Ref<int>` passes, so that unification reports the missing `.*`.
        let index = self.store.peeled(index_ty);
        let index = index
            .pointee(|id| self.store.get_definition(id))
            .unwrap_or(index);
        if index.is_simple(SimpleKind::Int) {
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
        expression: Box<Expression>,
        index: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        if index.is_range() {
            return self.infer_slice_range_access(expression, index, span, expected_ty);
        }

        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &collection_ty_var));

        let resolved_collection_ty = self.peel_resolved(&collection_ty_var);

        let index_expected_ty = match resolved_collection_ty.as_compound() {
            Some((CompoundKind::Map, args)) => {
                args.first().cloned().unwrap_or_else(|| self.new_type_var())
            }
            _ => self.new_type_var(),
        };

        let index_expression =
            self.with_value_context(|s| s.infer_expression(*index, &index_expected_ty));

        let resolved_index_ty = index_expected_ty.resolve_in(&self.env);

        if resolved_collection_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            return Expression::IndexedAccess {
                id: self.fresh_expression_id(),
                expression: collection_expression.into(),
                index: index_expression.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        }

        let Some(index_kind) = self.index_kind(&store.peeled(&resolved_collection_ty)) else {
            self.sink.push(diagnostics::infer::type_must_be_known(
                collection_expression.get_span(),
            ));
            return Expression::IndexedAccess {
                id: self.fresh_expression_id(),
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
        match (
            peel_to_range_type(&resolved_index_ty, |id| store.get_definition(id)),
            index_kind,
        ) {
            (Some(_), IndexKind::String) => {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                self.sink
                    .push(diagnostics::infer::string_not_sliceable(span, receiver));
                return Expression::IndexedAccess {
                    id: self.fresh_expression_id(),
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
            (Some(peeled), IndexKind::Slice) => {
                if let Some(bound_ty) = peeled.get_type_params().and_then(|p| p.first()) {
                    let int_ty = Type::int();
                    self.unify(&int_ty, bound_ty, &span);
                }

                let element_ty = resolved_collection_ty
                    .get_type_params()
                    .and_then(|params| params.first().cloned())
                    .unwrap_or_else(|| self.new_type_var());
                let result_ty = Type::qualified_compound(
                    CompoundKind::Slice,
                    vec![element_ty],
                    resolved_collection_ty.is_writable(),
                );
                self.unify(expected_ty, &result_ty, &span);

                return Expression::IndexedAccess {
                    id: self.fresh_expression_id(),
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

        let (expected_index_ty, expected_collection_ty) = match index_kind {
            IndexKind::Slice => (Type::int(), self.type_slice(element_ty.clone())),
            IndexKind::Array => {
                let len = resolved_collection_ty.array_len().unwrap_or(0);
                (Type::int(), self.type_array(len, element_ty.clone()))
            }
            IndexKind::Map => {
                let Some(type_params) = resolved_collection_ty.get_type_params() else {
                    self.unify(expected_ty, &Type::Error, &span);
                    return Expression::IndexedAccess {
                        id: self.fresh_expression_id(),
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
            IndexKind::String => {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                self.sink.push(diagnostics::infer::string_not_indexable(
                    collection_expression.get_span(),
                    receiver,
                ));
                return Expression::IndexedAccess {
                    id: self.fresh_expression_id(),
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
            IndexKind::NotIndexable => {
                self.sink.push(diagnostics::infer::not_indexable(
                    &resolved_collection_ty,
                    collection_expression.get_span(),
                ));
                return Expression::IndexedAccess {
                    id: self.fresh_expression_id(),
                    expression: collection_expression.into(),
                    index: index_expression.into(),
                    ty: Type::Error,
                    span,
                    from_colon_syntax: false,
                };
            }
        };

        self.unify(&expected_collection_ty, &resolved_collection_ty, &span);

        let index_ok = self.check_slice_index_type(
            index_kind,
            &resolved_index_ty,
            index_expression.get_span(),
        );

        if index_ok {
            self.unify(&expected_index_ty, &resolved_index_ty, &span);
        }
        self.unify(expected_ty, &element_ty, &span);

        Expression::IndexedAccess {
            id: self.fresh_expression_id(),
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
        expression: Box<Expression>,
        index: Box<Expression>,
        span: Span,
    ) -> Expression {
        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &collection_ty_var));
        let resolved_collection_ty = self.peel_resolved(&collection_ty_var);

        let receiver = collection_expression.root_identifier().unwrap_or("s");
        let type_name = resolved_collection_ty
            .display_name()
            .map(|name| name.to_string());
        self.sink.push(diagnostics::infer::colon_in_subscript(
            span,
            receiver,
            type_name.as_deref(),
        ));

        Expression::IndexedAccess {
            id: self.fresh_expression_id(),
            expression: collection_expression.into(),
            index,
            ty: Type::Error,
            span,
            from_colon_syntax: true,
        }
    }

    fn infer_slice_range_access(
        &mut self,
        expression: Box<Expression>,
        range: Box<Expression>,
        span: Span,
        expected_ty: &Type,
    ) -> Expression {
        let store = self.store;
        let collection_ty_var = self.new_type_var();
        let collection_expression =
            self.with_value_context(|s| s.infer_expression(*expression, &collection_ty_var));
        let resolved_collection_ty = self.peel_resolved(&collection_ty_var);

        if resolved_collection_ty.is_error() {
            self.unify(expected_ty, &Type::Error, &span);
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                id: self.fresh_expression_id(),
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        }

        // No look behind `Ref`: Go cannot slice a pointer to a slice.
        let Some(index_kind) = IndexKind::of(&store.peeled(&resolved_collection_ty)) else {
            self.sink.push(diagnostics::infer::type_must_be_known(
                collection_expression.get_span(),
            ));
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                id: self.fresh_expression_id(),
                expression: collection_expression.into(),
                index: inferred_range.into(),
                ty: Type::Error,
                span,
                from_colon_syntax: false,
            };
        };

        if index_kind != IndexKind::Slice {
            let diagnostic = if index_kind == IndexKind::String {
                let receiver = collection_expression.root_identifier().unwrap_or("s");
                diagnostics::infer::string_not_sliceable(span, receiver)
            } else {
                diagnostics::infer::only_slices_indexable_by_range(
                    &resolved_collection_ty,
                    collection_expression.get_span(),
                )
            };
            self.sink.push(diagnostic);
            let inferred_range = self.infer_range_bounds_only(range);
            return Expression::IndexedAccess {
                id: self.fresh_expression_id(),
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
                let int_ty = Type::int();

                let (new_start, new_end) = self.with_value_context(|s| {
                    let start =
                        start.map(|expression| Box::new(s.infer_expression(*expression, &int_ty)));
                    let end =
                        end.map(|expression| Box::new(s.infer_expression(*expression, &int_ty)));
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
                    id: self.fresh_expression_id(),
                    start: new_start,
                    end: new_end,
                    inclusive,
                    ty: range_ty,
                    span: range_span,
                }
            }
            _ => unreachable!("infer_slice_range_access called with non-range expression"),
        };
        self.record_value_use(&range_expression, &range_expression.get_type(), Vec::new());

        let result_ty = Type::qualified_compound(
            CompoundKind::Slice,
            vec![element_ty],
            resolved_collection_ty.is_writable(),
        );

        self.unify(expected_ty, &result_ty, &span);

        Expression::IndexedAccess {
            id: self.fresh_expression_id(),
            expression: collection_expression.into(),
            index: range_expression.into(),
            ty: result_ty,
            span,
            from_colon_syntax: false,
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
                let int_ty = Type::int();
                let new_start = start.map(|s| Box::new(self.infer_expression(*s, &int_ty)));
                let new_end = end.map(|e| Box::new(self.infer_expression(*e, &int_ty)));

                Expression::Range {
                    id: self.fresh_expression_id(),
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
