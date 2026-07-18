use diagnostics::LocalSink;
use rustc_hash::FxHashSet as HashSet;

use semantics::facts::Facts;
use semantics::store::Store;
use syntax::types::{CompoundKind, TypeVarId};

pub(crate) fn run(store: &Store, facts: &mut Facts, sink: &LocalSink) {
    let mut reported_vars: HashSet<(String, TypeVarId)> = HashSet::default();
    let mut collected = Vec::new();
    let mut report_vars = |ty: &syntax::types::Type, module_id: &str| {
        collected.clear();
        ty.collect_unbound_variables(&mut collected);
        reported_vars.extend(collected.iter().map(|v| (module_id.to_string(), *v)));
    };
    for check in std::mem::take(&mut facts.generic_call_checks) {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::cannot_infer_type_argument(check.span));
            report_vars(&check.ty, &check.module_id);
        }
    }
    for check in std::mem::take(&mut facts.struct_bound_checks) {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::cannot_infer_struct_type_argument(
                &check.struct_name,
                &check.param_name,
                &check.bound,
                check.span,
            ));
            report_vars(&check.ty, &check.module_id);
        }
    }
    for check in std::mem::take(&mut facts.empty_collection_checks) {
        if check.ty.has_unbound_variables() {
            sink.push(diagnostics::infer::uninferred_binding(
                &check.name,
                check.span,
            ));
            report_vars(&check.ty, &check.module_id);
        }
    }
    let mut reported_literal_spans = HashSet::default();
    for check in std::mem::take(&mut facts.empty_literal_checks) {
        if !check.ty.has_unbound_variables() {
            continue;
        }
        let mut literal_vars = Vec::new();
        check.ty.collect_unbound_variables(&mut literal_vars);
        if literal_vars
            .iter()
            .any(|v| reported_vars.contains(&(check.module_id.clone(), *v)))
        {
            continue;
        }
        if reported_literal_spans.insert(check.span) {
            sink.push(diagnostics::infer::empty_slice_no_element_type(check.span));
        }
    }
    for check in std::mem::take(&mut facts.slice_make_checks) {
        let slice_ty = store.peel_alias(&check.ty);
        let Some((CompoundKind::Slice, args)) = slice_ty.as_compound() else {
            continue;
        };
        let Some(element_ty) = args.first() else {
            continue;
        };
        if element_ty.is_error() || element_ty.is_variable() {
            continue;
        }
        if let Err(no_zero) = semantics::zero::has_zero(store, element_ty, &check.module_id) {
            sink.push(diagnostics::infer::slice_make_no_zero(
                &no_zero.leaf_ty.stringify(),
                check.span,
            ));
        }
    }
    for check in std::mem::take(&mut facts.statement_tail_checks) {
        if !check.expected_ty.is_unit()
            && !check.expected_ty.is_variable()
            && !check.expected_ty.is_ignored()
            && !check.expected_ty.is_error()
        {
            sink.push(diagnostics::infer::statement_as_tail(
                check.span,
                &check.expected_ty,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semantics::facts::{BindingIdAllocator, EmptyLiteralCheck, GenericCallCheck};
    use std::sync::Arc;
    use syntax::ast::Span;
    use syntax::types::{CompoundKind, Type};

    fn unbound_slice(id: u32) -> Type {
        Type::Compound {
            kind: CompoundKind::Slice,
            args: vec![Type::Var {
                id: TypeVarId(id),
                hint: Some("T".into()),
            }],
        }
    }

    fn run_checks(call_module: &str, literal_module: &str) -> Vec<String> {
        let mut facts = Facts::new(Arc::new(BindingIdAllocator::default()));
        facts.generic_call_checks.push(GenericCallCheck {
            ty: unbound_slice(5),
            span: Span::new(0, 0, 10),
            module_id: call_module.to_string(),
        });
        facts.empty_literal_checks.push(EmptyLiteralCheck {
            ty: unbound_slice(5),
            span: Span::new(1, 4, 2),
            module_id: literal_module.to_string(),
        });
        let sink = LocalSink::new();
        run(&Store::new(), &mut facts, &sink);
        sink.take()
            .iter()
            .filter_map(|d| d.code_str().map(str::to_string))
            .collect()
    }

    #[test]
    fn same_module_shared_var_suppresses_literal() {
        assert_eq!(run_checks("a", "a"), vec!["infer.missing_type_argument"]);
    }

    #[test]
    fn same_var_id_across_modules_does_not_suppress() {
        assert_eq!(
            run_checks("a", "b"),
            vec![
                "infer.missing_type_argument",
                "infer.empty_slice_no_element_type"
            ]
        );
    }
}
