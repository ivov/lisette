use syntax::ast::{Expression, Span};
use syntax::types::CompoundKind;

use crate::checker::EnvResolve;
use crate::checker::infer::InferCtx;

impl InferCtx<'_, '_> {
    /// Reject map bracket reads whose value type has no zero value. A missing
    /// key surfaces the Go zero value, which for types like `Ref<T>` is a nil
    /// pointer with no Lisette equivalent.
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
        let from_module = self.cursor.module_id.clone();
        if self.has_zero(value_ty, &from_module).is_err() {
            let receiver = collection.root_identifier().unwrap_or("m");
            let full_span = collection.get_span().merge(span);
            self.sink.push(diagnostics::infer::map_read_no_zero(
                value_ty, receiver, full_span,
            ));
        }
    }
}
