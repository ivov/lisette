use diagnostics::infer::MapReadNoZeroCause;
use syntax::ast::{Expression, Span};
use syntax::types::{CompoundKind, Type};

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;
use crate::zero::{MapZero, NoZeroReason};

impl InferCtx<'_> {
    /// Reject map bracket reads whose value type has no usable zero value. A
    /// missing key surfaces the Go zero value, which for `Ref<T>` is a nil
    /// pointer, and for a contained `Map` is a nil map that panics on write.
    pub fn check_map_bracket_reads(&mut self, items: &[Expression]) {
        for item in items {
            self.walk_map_bracket_reads(item, false);
        }
    }

    fn walk_map_bracket_reads(&mut self, expression: &Expression, is_write_target: bool) {
        match expression {
            Expression::Assignment {
                target,
                value,
                compound_operator,
                ..
            } => {
                // `m[k] = v` never reads the entry. Compound assignments do.
                self.walk_map_bracket_reads(target, compound_operator.is_none());
                self.walk_map_bracket_reads(value, false);
            }
            Expression::Paren { expression, .. } => {
                self.walk_map_bracket_reads(expression, is_write_target);
            }
            Expression::IndexedAccess {
                expression: collection,
                index,
                span,
                ..
            } => {
                if !is_write_target {
                    self.check_map_bracket_read(collection, *span);
                }
                self.walk_map_bracket_reads(collection, false);
                self.walk_map_bracket_reads(index, false);
            }
            _ => {
                for child in expression.children() {
                    self.walk_map_bracket_reads(child, false);
                }
            }
        }
    }

    fn check_map_bracket_read(&mut self, collection: &Expression, span: Span) {
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
        let Err(no_zero) = self.has_zero(value_ty, &from_package, MapZero::Nil) else {
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
