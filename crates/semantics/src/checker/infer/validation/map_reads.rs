use diagnostics::infer::MapReadNoZeroCause;
use ecow::EcoString;
use syntax::ast::{Expression, Span};
use syntax::program::DefinitionBody;
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::zero::{self, MapZero, NoZeroReason};

impl InferCtx<'_> {
    /// Reject map bracket reads whose value type has no usable zero value. A
    /// missing key surfaces the Go zero value, which for `Ref<T>` is a nil
    /// pointer, and for a contained `Map` is a nil map that panics on write.
    pub fn check_map_bracket_reads(&mut self, items: &[Expression]) {
        let mut bounds = Vec::new();
        for item in items {
            self.walk_map_bracket_reads(item, false, &mut bounds);
        }
    }

    fn walk_map_bracket_reads(
        &mut self,
        expression: &Expression,
        is_write_target: bool,
        bounds: &mut Vec<(EcoString, Vec<Type>)>,
    ) {
        let enclosing = bounds.len();
        if let Expression::Function { generics, .. } | Expression::ImplBlock { generics, .. } =
            expression
        {
            bounds.extend(generics.iter().map(|generic| {
                let resolved = generic
                    .resolved_bounds()
                    .map(|bounds| bounds.cloned().collect())
                    .unwrap_or_default();
                (generic.name.clone(), resolved)
            }));
        }
        // An impl also inherits the bounds its receiver type declares.
        if let Expression::ImplBlock { ty, .. } = expression
            && let Type::Nominal { id, params, .. } = ty
            && let Some(
                DefinitionBody::Struct {
                    generics: declared, ..
                }
                | DefinitionBody::Enum {
                    generics: declared, ..
                },
            ) = self.store.get_definition(id.as_str()).map(|d| &d.body)
        {
            for (declared, param) in declared.iter().zip(params) {
                if let Type::Parameter(name) = param {
                    let inherited = declared
                        .resolved_bounds()
                        .map(|bounds| bounds.cloned().collect())
                        .unwrap_or_default();
                    bounds.push((name.clone(), inherited));
                }
            }
        }
        match expression {
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                // `m[k] = v` never reads the entry. Compound assignments do.
                self.walk_map_bracket_reads(target, compound_operator.is_none(), bounds);
                self.walk_map_bracket_reads(value, false, bounds);
            }
            Expression::Paren { expression, .. } => {
                self.walk_map_bracket_reads(expression, is_write_target, bounds);
            }
            Expression::IndexedAccess {
                expression: collection,
                index,
                span,
                ..
            } => {
                if !is_write_target {
                    self.check_map_bracket_read(collection, *span, bounds);
                }
                self.walk_map_bracket_reads(collection, false, bounds);
                self.walk_map_bracket_reads(index, false, bounds);
            }
            _ => {
                for child in expression.children() {
                    self.walk_map_bracket_reads(child, false, bounds);
                }
            }
        }
        bounds.truncate(enclosing);
    }

    fn check_map_bracket_read(
        &mut self,
        collection: &Expression,
        span: Span,
        bounds: &[(EcoString, Vec<Type>)],
    ) {
        let store = self.store;
        let collection_ty = store.peel_alias(&collection.get_type().resolve_in(&self.env));
        let Some((CompoundKind::Map, args)) = collection_ty.as_compound() else {
            return;
        };
        let Some(value_ty) = args.get(1) else {
            return;
        };
        if value_ty.is_error() || value_ty.is_variable() {
            return;
        }
        if store.peel_alias(value_ty).is_map() {
            self.report_map_read(collection, span, value_ty, MapReadNoZeroCause::NilMap);
            return;
        }
        let from_package = self.cursor.package_id().to_string();
        let Err(no_zero) =
            zero::has_zero_in_scope(store, value_ty, &from_package, MapZero::Nil, bounds)
        else {
            return;
        };
        let cause = match no_zero.reason {
            NoZeroReason::NilMap => MapReadNoZeroCause::ContainsNilMap(&no_zero.leaf_ty),
            _ => MapReadNoZeroCause::NoZero,
        };
        self.report_map_read(collection, span, value_ty, cause);
    }

    fn report_map_read(
        &mut self,
        collection: &Expression,
        span: Span,
        value_ty: &Type,
        cause: MapReadNoZeroCause<'_>,
    ) {
        let receiver = collection.root_identifier().unwrap_or("m");
        let full_span = collection.get_span().merge(span);
        self.sink.push(diagnostics::infer::map_read_no_zero(
            value_ty, receiver, cause, full_span,
        ));
    }
}
